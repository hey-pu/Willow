use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, DatabaseName, OpenFlags};

const FILES: [&str; 3] = ["tokbar.db", "settings.json", "litellm-pricing.json"];
const MARKER: &str = ".tokbar-migration-in-progress";

/// Import once into an empty Willow data directory. The marker makes a
/// partially published migration resumable after a crash or an I/O error.
pub fn migrate(
    old_dir: &Path,
    new_dir: &Path,
    old_web_storage: Option<&Path>,
) -> Result<(), String> {
    fs::create_dir_all(new_dir).map_err(|e| e.to_string())?;
    let lock = Connection::open(new_dir.join(".migration-lock.db")).map_err(|e| e.to_string())?;
    lock.busy_timeout(Duration::from_secs(30))
        .map_err(|e| e.to_string())?;
    lock.execute_batch("BEGIN EXCLUSIVE")
        .map_err(|e| e.to_string())?;

    let marker = new_dir.join(MARKER);
    if !marker.exists() {
        if FILES.iter().any(|name| new_dir.join(name).exists())
            || (!FILES.iter().any(|name| old_dir.join(name).exists())
                && !old_web_storage.is_some_and(Path::is_dir))
        {
            return Ok(());
        }
        fs::write(&marker, b"Legacy data migration in progress\n").map_err(|e| e.to_string())?;
    }
    if !old_dir.is_dir() && !old_web_storage.is_some_and(Path::is_dir) {
        return Err(
            "The previous data directory is unavailable; restart Willow after restoring it".into(),
        );
    }

    #[cfg(any(windows, target_os = "macos"))]
    if let Some(source) = old_web_storage.filter(|path| path.is_dir()) {
        let target = new_dir.join("legacy-web-preferences.json");
        if !target.exists() {
            #[cfg(windows)]
            let preferences = crate::legacy_web_storage::read(source, new_dir)?;
            #[cfg(target_os = "macos")]
            let preferences = crate::legacy_webkit_storage::read(source)?;
            let temporary = new_dir.join(format!(".legacy-web-preferences-{}", std::process::id()));
            let result = (|| -> Result<(), String> {
                let json = serde_json::to_vec(&preferences).map_err(|e| e.to_string())?;
                fs::write(&temporary, json).map_err(|e| e.to_string())?;
                fs::hard_link(&temporary, &target).map_err(|e| e.to_string())?;
                Ok(())
            })();
            let _ = fs::remove_file(temporary);
            result?;
        }
    }

    // Publish the database last. Existing files are never replaced on retry.
    for name in ["settings.json", "litellm-pricing.json", "tokbar.db"] {
        let source = old_dir.join(name);
        let target = new_dir.join(name);
        if !source.exists() || target.exists() {
            continue;
        }
        let temporary = new_dir.join(format!(".{name}.migration-{}", std::process::id()));
        if temporary.exists() {
            fs::remove_file(&temporary).map_err(|e| e.to_string())?;
        }
        let result = (|| -> Result<(), String> {
            if name == "tokbar.db" {
                let source_db =
                    Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY)
                        .map_err(|e| e.to_string())?;
                source_db
                    .busy_timeout(Duration::from_secs(30))
                    .map_err(|e| e.to_string())?;
                source_db
                    .backup(DatabaseName::Main, &temporary, None)
                    .map_err(|e| e.to_string())?;
                let snapshot = Connection::open(&temporary).map_err(|e| e.to_string())?;
                let check: String = snapshot
                    .query_row("PRAGMA quick_check", [], |row| row.get(0))
                    .map_err(|e| e.to_string())?;
                if check != "ok" {
                    return Err(format!("database integrity check failed: {check}"));
                }
                snapshot
                    .execute_batch("PRAGMA journal_mode=DELETE;")
                    .map_err(|e| e.to_string())?;
            } else if name == "settings.json" {
                let bytes = fs::read(&source).map_err(|e| e.to_string())?;
                let settings = merge_legacy_token_unit(new_dir, &bytes)?;
                fs::write(&temporary, settings).map_err(|e| e.to_string())?;
            } else {
                let bytes = fs::read(&source).map_err(|e| e.to_string())?;
                serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| e.to_string())?;
                fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
            }
            // A hard link publishes a complete file atomically and fails if a
            // target already exists, including one created by another process.
            fs::hard_link(&temporary, &target).map_err(|e| e.to_string())?;
            Ok(())
        })();
        let _ = fs::remove_file(&temporary);
        for suffix in ["-wal", "-shm", "-journal"] {
            let _ = fs::remove_file(
                new_dir.join(format!(".{name}.migration-{}{suffix}", std::process::id())),
            );
        }
        result.map_err(|error| format!("Could not migrate {name}: {error}"))?;
    }
    if !new_dir.join("settings.json").exists() {
        let settings = merge_legacy_token_unit(new_dir, b"{}")?;
        if settings != b"{}" {
            // The migration lock serializes writers, and the marker stays in place
            // until this settings file and the other imports are published.
            let temporary = new_dir.join(".settings.json.migration-token-unit");
            fs::write(&temporary, settings).map_err(|e| e.to_string())?;
            let result = fs::hard_link(&temporary, new_dir.join("settings.json"));
            let _ = fs::remove_file(temporary);
            result.map_err(|e| e.to_string())?;
        }
    }
    fs::remove_file(marker).map_err(|e| e.to_string())?;
    Ok(())
}

fn merge_legacy_token_unit(directory: &Path, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut settings: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if settings.contains_key("tokenUnit") {
        return Ok(bytes.to_vec());
    }
    let preferences = match fs::read(directory.join("legacy-web-preferences.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(bytes.to_vec()),
        Err(error) => return Err(error.to_string()),
    };
    let preferences: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&preferences).map_err(|e| e.to_string())?;
    if let Some(unit) = preferences
        .get("tokbar-token-unit")
        .filter(|unit| matches!(unit.as_str(), "chinese" | "international"))
    {
        settings.insert("tokenUnit".into(), unit.as_str().into());
        return serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string());
    }
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn dirs() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "willow-migration-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let old = root.join("old");
        let new = root.join("new");
        fs::create_dir_all(&old).unwrap();
        (root, old, new)
    }

    #[test]
    fn migrates_live_wal_and_preserves_target_on_repeat() {
        let (root, old, new) = dirs();
        let db = Connection::open(old.join("tokbar.db")).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE sample (value TEXT); INSERT INTO sample VALUES ('history');").unwrap();
        fs::write(old.join("settings.json"), br#"{"language":"zh"}"#).unwrap();
        migrate(&old, &new, None).unwrap();
        let snapshot = Connection::open(new.join("tokbar.db")).unwrap();
        let value: String = snapshot
            .query_row("SELECT value FROM sample", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "history");
        fs::write(new.join("settings.json"), br#"{"language":"en"}"#).unwrap();
        migrate(&old, &new, None).unwrap();
        assert_eq!(
            fs::read(new.join("settings.json")).unwrap(),
            br#"{"language":"en"}"#
        );
        drop(snapshot);
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_migration_retries_without_overwriting_published_files() {
        let (root, old, new) = dirs();
        fs::write(old.join("settings.json"), br#"{"language":"zh"}"#).unwrap();
        fs::write(old.join("litellm-pricing.json"), b"invalid").unwrap();
        assert!(migrate(&old, &new, None).is_err());
        assert!(new.join(MARKER).exists());
        fs::write(old.join("settings.json"), br#"{"language":"en"}"#).unwrap();
        fs::write(old.join("litellm-pricing.json"), b"{}").unwrap();
        migrate(&old, &new, None).unwrap();
        assert_eq!(
            fs::read(new.join("settings.json")).unwrap(),
            br#"{"language":"zh"}"#
        );
        assert!(!new.join(MARKER).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_willow_data_takes_precedence() {
        let (root, old, new) = dirs();
        fs::create_dir_all(&new).unwrap();
        fs::write(old.join("settings.json"), b"{}").unwrap();
        fs::write(new.join("tokbar.db"), b"existing").unwrap();
        migrate(&old, &new, None).unwrap();
        assert!(!new.join("settings.json").exists());
        assert_eq!(fs::read(new.join("tokbar.db")).unwrap(), b"existing");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imports_legacy_web_token_unit_only_when_native_setting_is_missing() {
        let (root, old, new) = dirs();
        fs::create_dir_all(&new).unwrap();
        fs::write(
            new.join("legacy-web-preferences.json"),
            br#"{"tokbar-token-unit":"international"}"#,
        )
        .unwrap();
        fs::write(old.join("settings.json"), br#"{"language":"zh"}"#).unwrap();
        migrate(&old, &new, None).unwrap();
        let settings: serde_json::Value =
            serde_json::from_slice(&fs::read(new.join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings["language"], "zh");
        assert_eq!(settings["tokenUnit"], "international");
        let native = br#"{"tokenUnit":"chinese"}"#;
        assert_eq!(merge_legacy_token_unit(&new, native).unwrap(), native);
        fs::remove_dir_all(root).unwrap();
    }
}
