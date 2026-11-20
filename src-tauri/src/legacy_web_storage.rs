use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::Path;

use fs2::FileExt;
use rusty_leveldb::{LdbIterator, Options, DB};

const KEYS: [&str; 7] = [
    "tokbar-lang", "tokbar-range", "tokbar-theme-mode", "tokbar-accent",
    "tokbar-subscriptions", "tokbar-cost-mode", "tokbar-token-unit",
];

fn decode_string(bytes: &[u8]) -> Result<String, String> {
    let (&encoding, contents) = bytes.split_first().ok_or("empty storage value")?;
    match encoding {
        1 => Ok(contents.iter().map(|&byte| char::from(byte)).collect()),
        0 if contents.len() % 2 == 0 => {
            let units: Vec<u16> = contents.chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16(&units).map_err(|e| e.to_string())
        }
        _ => Err("unsupported Chromium storage encoding".into()),
    }
}

/// Read a consistent copy while holding the same lock used by LevelDB.
/// The source database is never opened by the LevelDB implementation.
pub fn read(source: &Path, target_dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let lock = OpenOptions::new().read(true).write(true).open(source.join("LOCK"))
        .map_err(|e| format!("Cannot open the previous application's storage lock: {e}"))?;
    lock.try_lock_exclusive().map_err(|_| "Quit the previous application from its tray menu, then restart Willow to import the saved preferences.".to_string())?;
    let snapshot = target_dir.join(format!(".web-storage-snapshot-{}", std::process::id()));
    if snapshot.exists() {
        fs::remove_dir_all(&snapshot).map_err(|e| e.to_string())?;
    }
    fs::create_dir(&snapshot).map_err(|e| e.to_string())?;
    let result = (|| -> Result<BTreeMap<String, String>, String> {
        for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_file()
                && entry.file_name() != "LOCK"
            {
                fs::copy(entry.path(), snapshot.join(entry.file_name())).map_err(|e| e.to_string())?;
            }
        }
        // Once copied, Chromium can safely resume writing its own profile.
        FileExt::unlock(&lock).map_err(|e| e.to_string())?;
        let options = Options { create_if_missing: false, ..Options::default() };
        let mut db = DB::open(&snapshot, options).map_err(|e| e.to_string())?;
        let mut iter = db.new_iter().map_err(|e| e.to_string())?;
        let mut production = BTreeMap::new();
        let mut development = BTreeMap::new();
        while let Some((key, value)) = iter.next() {
            let Some(separator) = key.iter().position(|&byte| byte == 0) else { continue };
            let origin = &key[..separator];
            let preferences = match origin {
                b"_http://tauri.localhost" | b"_https://tauri.localhost" => &mut production,
                b"_http://localhost:1420" | b"_http://127.0.0.1:1420" => &mut development,
                _ => continue,
            };
            let name = decode_string(&key[separator + 1..])?;
            if KEYS.contains(&name.as_str()) {
                preferences.insert(name, decode_string(&value)?);
            }
        }
        development.extend(production);
        Ok(development)
    })();
    drop(lock);
    let _ = fs::remove_dir_all(snapshot);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_chromium_latin1_and_utf16() {
        assert_eq!(decode_string(b"\x01hello").unwrap(), "hello");
        assert_eq!(decode_string(&[0, 0x2d, 0x4e, 0x87, 0x65]).unwrap(), "中文");
        assert!(decode_string(&[0, 1]).is_err());
    }

    #[test]
    fn reads_snapshot_and_rejects_a_live_profile() {
        let root = std::env::temp_dir().join(format!(
            "willow-web-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let source = root.join("source");
        let target = root.join("target");
        fs::create_dir_all(&target).unwrap();
        let mut db = DB::open(&source, Options::default()).unwrap();
        db.put(b"_http://tauri.localhost\0\x01tokbar-theme-mode", b"\x01dark").unwrap();
        db.put(b"_http://tauri.localhost\0\x01tokbar-subscriptions", b"\x01[]").unwrap();
        db.put(b"_http://other.example\0\x01tokbar-theme-mode", b"\x01light").unwrap();
        db.flush().unwrap();
        assert!(read(&source, &target).is_err());
        drop(db);
        let preferences = read(&source, &target).unwrap();
        assert_eq!(preferences.get("tokbar-theme-mode").map(String::as_str), Some("dark"));
        assert_eq!(preferences.get("tokbar-subscriptions").map(String::as_str), Some("[]"));
        assert_eq!(preferences.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }

}
