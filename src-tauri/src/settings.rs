//! A small fixed-location settings store. Source PDFs never enter this path.
use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs::{File, OpenOptions}, io::{Read, Write}, os::windows::{ffi::OsStrExt, fs::{MetadataExt, OpenOptionsExt}}, path::{Path, PathBuf}};
use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, FILE_FLAG_OPEN_REPARSE_POINT};

const LIMIT: u64 = 64 * 1024;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub enum DefaultZoom {
    #[default]
    #[serde(rename = "page-width")] Width,
    #[serde(rename = "page-fit")] Page,
    #[serde(rename = "actual-size")] Actual,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderSettings { pub default_zoom: DefaultZoom }
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot { pub schema_version: u32, pub reader: ReaderSettings, pub writable: bool, pub notice: &'static str }
pub struct Settings {
    path: PathBuf, _lock: Option<File>, original: Option<Vec<u8>>, document: Value, snapshot: Snapshot,
}
fn read(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let mut file = match OpenOptions::new().read(true).share_mode(1 | 4).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path) {
        Ok(file) => file, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None), Err(e) => return Err(e),
    };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > LIMIT || meta.file_attributes() & 0x400 != 0 {
        return Err(std::io::Error::other("Invalid settings file"));
    }
    let mut bytes = Vec::new(); (&mut file).take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT { return Err(std::io::Error::other("Settings too large")); }
    Ok(Some(bytes))
}
impl Settings {
    // Directory comes only from Tauri's application config path, never from IPC.
    pub fn load(directory: PathBuf) -> Self {
        let mut store = Self { path: directory.join("settings.json"), _lock: None, original: None,
            document: json!({"schemaVersion": 1, "reader": {"defaultZoom": "page-width"}}),
            snapshot: Snapshot { schema_version: 1, reader: ReaderSettings::default(), writable: false,
                notice: "设置暂时不可写，本次使用默认值，已有内容不会覆盖。" } };
        if std::fs::create_dir_all(&directory).is_err() { return store; }
        if !std::fs::symlink_metadata(&directory).is_ok_and(|m| m.is_dir() && m.file_attributes() & 0x400 == 0) { return store; }
        store._lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(directory.join("settings.lock")).ok()
            .filter(|f| f.metadata().is_ok_and(|m| m.file_attributes() & 0x400 == 0));
        if store._lock.is_none() { store.snapshot.notice = "设置被另一窗口占用，本次只读。"; return store; }
        let original = match read(&store.path) { Ok(bytes) => bytes, Err(_) => return store };
        if let Some(bytes) = &original {
            let document: Value = match serde_json::from_slice(bytes) { Ok(value) => value, Err(_) => {
                store.snapshot.notice = "设置文件损坏，保留原文件并使用默认值。"; return store;
            } };
            if document.get("schemaVersion").and_then(Value::as_u64) != Some(1) {
                store.snapshot.notice = "设置版本不受支持，保留原文件并使用默认值。"; return store;
            }
            // Validate known fields while preserving unknown fields in the stored document.
            let zoom = document.get("reader").and_then(|v| v.get("defaultZoom")).cloned();
            let Some(zoom) = zoom.and_then(|v| serde_json::from_value(v).ok()) else { return store; };
            store.snapshot.reader.default_zoom = zoom;
            store.document = document;
        }
        store.original = original; store.snapshot.writable = true; store.snapshot.notice = ""; store
    }
    pub fn snapshot(&self) -> Snapshot { self.snapshot.clone() }
    // Kept solely for the existing legacy-format regression suite. Desktop
    // settings updates now use the current private SQLite data directory.
    #[allow(dead_code)]
    pub fn update(&mut self, reader: ReaderSettings) -> Result<Snapshot> {
        if !self.snapshot.writable { return Err(settings_error()); }
        if read(&self.path).map_err(|_| settings_error())? != self.original {
            self.snapshot.writable = false; self.snapshot.notice = "设置已被外部修改，保留外部内容；重开应用后再试。";
            return Err(settings_error());
        }
        let mut document = self.document.clone();
        document["reader"]["defaultZoom"] = serde_json::to_value(reader.default_zoom).map_err(|_| settings_error())?;
        let bytes = serde_json::to_vec_pretty(&document).map_err(|_| settings_error())?;
        if bytes.len() as u64 > LIMIT { return Err(settings_error()); }
        let temporary = self.path.with_file_name(format!("settings-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|_| settings_error())?;
            file.write_all(&bytes).map_err(|_| settings_error())?; file.sync_all().map_err(|_| settings_error())?; drop(file);
            if read(&self.path).map_err(|_| settings_error())? != self.original {
                self.snapshot.writable = false; self.snapshot.notice = "设置已被外部修改，保留外部内容；重开应用后再试。";
                return Err(settings_error());
            }
            let wide = |path: &Path| path.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
            let (from, to) = (wide(&temporary), wide(&self.path));
            if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) } == 0 { return Err(settings_error()); }
            Ok(())
        })();
        // Remove only this call's unique temporary file, never existing settings or user files.
        if result.is_err() { let _ = std::fs::remove_file(&temporary); }
        result?; self.document = document; self.original = Some(bytes); self.snapshot.reader = reader; Ok(self.snapshot())
    }
}
#[allow(dead_code)]
fn settings_error() -> AppError { AppError { code: "settings_unavailable", message: "无法保存设置；已有内容未被覆盖。" } }

#[cfg(test)]
mod tests {
    use super::*;
    fn directory() -> PathBuf {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests/work/m2-b/settings-tests").join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&path).unwrap(); path
    }
    #[test] fn defaults_persist_unknown_fields_and_lifecycle_lock() {
        let dir = directory(); let path = dir.join("settings.json");
        std::fs::write(&path, br#"{"schemaVersion":1,"future":{"x":3},"reader":{"defaultZoom":"page-width","extra":true}}"#).unwrap();
        let mut settings = Settings::load(dir.clone()); assert!(settings.snapshot().writable);
        assert!(!Settings::load(dir.clone()).snapshot().writable);
        settings.update(ReaderSettings { default_zoom: DefaultZoom::Page }).unwrap(); drop(settings);
        let stored: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(stored["future"]["x"], 3); assert_eq!(stored["reader"]["extra"], true);
        let settings = Settings::load(dir); assert!(settings.snapshot().writable); assert_eq!(settings.snapshot().reader.default_zoom, DefaultZoom::Page);
    }
    #[test] fn future_corrupt_oversized_and_external_changes_are_never_overwritten() {
        for bytes in [br#"{"schemaVersion":2}"#.to_vec(), b"corrupt".to_vec(), vec![b' '; LIMIT as usize + 1]] {
            let dir = directory(); let path = dir.join("settings.json"); std::fs::write(&path, &bytes).unwrap();
            let mut store = Settings::load(dir); assert!(!store.snapshot().writable); assert!(store.update(ReaderSettings::default()).is_err());
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        let dir = directory(); let mut store = Settings::load(dir.clone());
        let path = dir.join("settings.json"); std::fs::write(&path, b"external edit").unwrap();
        assert!(store.update(ReaderSettings::default()).is_err()); assert_eq!(std::fs::read(path).unwrap(), b"external edit");
        assert!(!store.snapshot().writable);
    }
    #[test] fn invalid_command_fields_fail_deserialization() {
        assert!(serde_json::from_value::<ReaderSettings>(json!({"defaultZoom":"unknown"})).is_err());
        assert!(serde_json::from_value::<ReaderSettings>(json!({"defaultZoom":"page-fit", "path":"arbitrary"})).is_err());
    }
}
