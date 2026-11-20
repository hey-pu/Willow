use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{types::ValueRef, Connection, OpenFlags};

const KEYS: [&str; 7] = [
    "tokbar-lang",
    "tokbar-range",
    "tokbar-theme-mode",
    "tokbar-accent",
    "tokbar-subscriptions",
    "tokbar-cost-mode",
    "tokbar-token-unit",
];

fn collect_databases(directory: &Path, databases: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        let path = entry.path();
        if kind.is_dir() {
            collect_databases(&path, databases)?;
        } else if kind.is_file()
            && (entry.file_name() == "localstorage.sqlite3"
                || path
                    .extension()
                    .is_some_and(|extension| extension == "localstorage"))
        {
            databases.push(path);
        }
    }
    Ok(())
}

/// WebKit stores values as UTF-16 blobs and keys as SQLite text. Querying
/// through SQLite includes committed WAL pages without copying a live file.
pub fn read(directory: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut databases = Vec::new();
    collect_databases(directory, &mut databases)?;
    databases.sort();
    let mut preferences = BTreeMap::new();
    for path in databases {
        let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| format!("Cannot read legacy WebKit storage: {error}"))?;
        db.busy_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        let mut statement = db
            .prepare("SELECT key, value FROM ItemTable")
            .map_err(|error| format!("Cannot read legacy WebKit preferences: {error}"))?;
        let mut rows = statement.query([]).map_err(|error| error.to_string())?;
        while let Some(row) = rows.next().map_err(|error| error.to_string())? {
            let key: String = row.get(0).map_err(|error| error.to_string())?;
            if !KEYS.contains(&key.as_str()) {
                continue;
            }
            let value = match row.get_ref(1).map_err(|error| error.to_string())? {
                ValueRef::Blob(bytes) if bytes.len() % 2 == 0 => {
                    let units: Vec<u16> = bytes
                        .chunks_exact(2)
                        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                        .collect();
                    String::from_utf16(&units).map_err(|error| error.to_string())?
                }
                ValueRef::Text(bytes) => {
                    String::from_utf8(bytes.to_vec()).map_err(|error| error.to_string())?
                }
                _ => return Err(format!("Invalid WebKit preference encoding for {key}")),
            };
            if preferences
                .get(&key)
                .is_some_and(|previous| previous != &value)
            {
                return Err(format!("Conflicting legacy WebKit preferences for {key}. Quit the previous application and retry migration."));
            }
            preferences.insert(key, value);
        }
    }
    Ok(preferences)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "willow-webkit-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn create_database(path: &Path, key: &str, value: &str) -> Connection {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let db = Connection::open(path).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE ItemTable (key TEXT UNIQUE, value BLOB NOT NULL);").unwrap();
        let bytes: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
        db.execute(
            "INSERT INTO ItemTable VALUES (?1, ?2)",
            rusqlite::params![key, bytes],
        )
        .unwrap();
        db
    }

    #[test]
    fn reads_legacy_and_modern_layouts_including_live_wal_and_unicode() {
        let root = fixture();
        let legacy = create_database(
            &root.join("LocalStorage/tauri_localhost_0.localstorage"),
            "tokbar-theme-mode",
            "dark",
        );
        let subscriptions =
            r#"[{"id":"sample","name":"柳的套餐","monthlyUsd":20,"agent":"codex"}]"#;
        let modern = create_database(
            &root.join("Default/top/client/LocalStorage/localstorage.sqlite3"),
            "tokbar-subscriptions",
            subscriptions,
        );
        let ignored = create_database(
            &root.join("Default/other/LocalStorage/localstorage.sqlite3"),
            "unrelated",
            "private",
        );
        let preferences = read(&root).unwrap();
        assert_eq!(preferences.len(), 2);
        assert_eq!(preferences["tokbar-theme-mode"], "dark");
        assert_eq!(preferences["tokbar-subscriptions"], subscriptions);
        drop((legacy, modern, ignored));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_ambiguous_values_and_malformed_utf16() {
        let root = fixture();
        let first = create_database(
            &root.join("first.localstorage"),
            "tokbar-theme-mode",
            "dark",
        );
        let second = create_database(
            &root.join("second.localstorage"),
            "tokbar-theme-mode",
            "light",
        );
        assert!(read(&root).unwrap_err().contains("Conflicting"));
        second
            .execute("UPDATE ItemTable SET value=x'01'", [])
            .unwrap();
        assert!(read(&root).unwrap_err().contains("encoding"));
        drop((first, second));
        fs::remove_dir_all(root).unwrap();
    }
}
