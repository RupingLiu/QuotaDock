use crate::models::{AppState, QuotaSnapshot, StorageStatus, StoredState, STATE_VERSION};
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct StoreError {
    message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadOutcome {
    pub state: StoredState,
    pub status: StorageStatus,
    pub path: PathBuf,
    pub backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct UsageStore {
    path: PathBuf,
}

impl UsageStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<LoadOutcome, StoreError> {
        if !self.path.exists() {
            return Ok(self.outcome(StoredState::default(), StorageStatus::Missing, None));
        }

        let raw_bytes = std::fs::read(&self.path)?;
        let raw = match String::from_utf8(raw_bytes) {
            Ok(raw) => raw,
            Err(_) => {
                let backup_path = self.backup_existing_file("corrupt")?;
                let state = StoredState::default();
                self.save_state(&state)?;
                return Ok(self.outcome(state, StorageStatus::Recovered, Some(backup_path)));
            }
        };
        let state = match serde_json::from_str::<StoredState>(&raw) {
            Ok(state) => state,
            Err(_) => {
                let backup_path = self.backup_existing_file("corrupt")?;
                let state = StoredState::default();
                self.save_state(&state)?;
                return Ok(self.outcome(state, StorageStatus::Recovered, Some(backup_path)));
            }
        };

        if state.version != STATE_VERSION {
            let backup_path = self.backup_existing_file("unsupported")?;
            let state = StoredState::default();
            self.save_state(&state)?;
            return Ok(self.outcome(state, StorageStatus::UnsupportedVersion, Some(backup_path)));
        }

        Ok(self.outcome(state, StorageStatus::Ready, None))
    }

    pub fn save_state(&self, state: &StoredState) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(state)?;
        let temp_path = self.path.with_extension(format!("tmp-{}", unix_nanos()));
        {
            let mut file = std::fs::File::create(&temp_path)?;
            file.write_all(json.as_bytes())?;
            file.flush()?;
            file.sync_all()?;
        }
        if let Err(error) = atomic_replace(&temp_path, &self.path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(error.into());
        }
        Ok(())
    }

    pub fn save_snapshot(&self, snapshot: QuotaSnapshot) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let status = mutation_status(&loaded.status);
        let backup_path = loaded.backup_path;
        let state = StoredState {
            version: STATE_VERSION,
            latest_snapshot: Some(snapshot),
        };
        self.save_state(&state)?;
        Ok(self.outcome(state, status, backup_path))
    }

    pub fn clear_snapshot(&self) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let status = mutation_status(&loaded.status);
        let backup_path = loaded.backup_path;
        let state = StoredState::default();
        self.save_state(&state)?;
        Ok(self.outcome(state, status, backup_path))
    }

    fn outcome(
        &self,
        state: StoredState,
        status: StorageStatus,
        backup_path: Option<PathBuf>,
    ) -> LoadOutcome {
        LoadOutcome {
            state,
            status,
            path: self.path.clone(),
            backup_path,
        }
    }

    fn backup_existing_file(&self, reason: &str) -> Result<PathBuf, StoreError> {
        let backup_path = self
            .path
            .with_extension(format!("{reason}-{}.bak", unix_nanos()));
        std::fs::copy(&self.path, &backup_path)?;
        Ok(backup_path)
    }
}

impl LoadOutcome {
    pub fn into_app_state(self) -> AppState {
        AppState::from_stored(
            self.state,
            self.status,
            Some(self.path.display().to_string()),
            self.backup_path.map(|path| path.display().to_string()),
        )
    }
}

impl StoreError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for StoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(value: std::io::Error) -> Self {
        Self::new(value.to_string())
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::new(value.to_string())
    }
}

#[cfg(windows)]
fn atomic_replace(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let from_wide: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to_wide: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            from_wide.as_ptr(),
            to_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

fn mutation_status(status: &StorageStatus) -> StorageStatus {
    match status {
        StorageStatus::Recovered => StorageStatus::Recovered,
        StorageStatus::UnsupportedVersion => StorageStatus::UnsupportedVersion,
        StorageStatus::Ready | StorageStatus::Missing => StorageStatus::Ready,
    }
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use crate::models::{QuotaReading, QuotaSnapshot, SnapshotSource, StorageStatus, StoredState};
    use crate::usage_store::UsageStore;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let unique = format!(
                "quotadock-{name}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn snapshot(percent: u8) -> QuotaSnapshot {
        QuotaSnapshot {
            id: "snap-1".to_string(),
            source: SnapshotSource::PastedStatus,
            captured_at: "unix:1000".to_string(),
            five_hour: QuotaReading {
                remaining_percent: Some(percent),
                reset_at: None,
                reset_countdown_seconds: Some(3600),
            },
            weekly: QuotaReading {
                remaining_percent: Some(46),
                reset_at: Some("2026-06-23T09:00:00Z".to_string()),
                reset_countdown_seconds: None,
            },
            raw_text: "status".to_string(),
            status_message: "已更新 5 小时与 1 周额度。".to_string(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn missing_file_loads_default_state_without_creating_file() {
        let dir = TestDir::new("missing");
        let path = dir.path().join("state.json");
        let store = UsageStore::new(path.clone());

        let outcome = store.load().unwrap();

        assert_eq!(outcome.status, StorageStatus::Missing);
        assert_eq!(outcome.state, StoredState::default());
        assert!(!path.exists());
    }

    #[test]
    fn valid_v2_file_loads_state() {
        let dir = TestDir::new("valid");
        let path = dir.path().join("state.json");
        let store = UsageStore::new(path.clone());
        store.save_snapshot(snapshot(72)).unwrap();

        let outcome = store.load().unwrap();

        assert_eq!(outcome.status, StorageStatus::Ready);
        assert_eq!(
            outcome
                .state
                .latest_snapshot
                .unwrap()
                .five_hour
                .remaining_percent,
            Some(72)
        );
    }

    #[test]
    fn corrupt_json_is_backed_up_and_recovered_to_default() {
        let dir = TestDir::new("corrupt");
        let path = dir.path().join("state.json");
        std::fs::write(&path, "{not valid json").unwrap();
        let store = UsageStore::new(path);

        let outcome = store.load().unwrap();

        assert_eq!(outcome.status, StorageStatus::Recovered);
        assert!(outcome.backup_path.unwrap().exists());
        assert_eq!(outcome.state, StoredState::default());
    }

    #[test]
    fn unsupported_version_is_backed_up_and_reset() {
        let dir = TestDir::new("unsupported");
        let path = dir.path().join("state.json");
        std::fs::write(&path, r#"{"version":1,"latestSnapshot":null}"#).unwrap();
        let store = UsageStore::new(path);

        let outcome = store.load().unwrap();

        assert_eq!(outcome.status, StorageStatus::UnsupportedVersion);
        assert!(outcome.backup_path.unwrap().exists());
        assert_eq!(outcome.state, StoredState::default());
    }

    #[test]
    fn save_and_clear_snapshot() {
        let dir = TestDir::new("clear");
        let path = dir.path().join("state.json");
        let store = UsageStore::new(path);

        let saved = store.save_snapshot(snapshot(88)).unwrap();
        assert!(saved.state.latest_snapshot.is_some());

        let cleared = store.clear_snapshot().unwrap();
        assert!(cleared.state.latest_snapshot.is_none());
    }
}
