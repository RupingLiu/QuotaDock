use crate::models::{
    AppState, ConfidenceState, ManualField, ManualUpdateInput, SnapshotSource, StorageStatus,
    StoredState, UsageSnapshot, DEFAULT_HISTORY_LIMIT, STATE_VERSION,
};
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

    pub fn save_snapshot(&self, snapshot: UsageSnapshot) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let status = mutation_status(&loaded.status);
        let backup_path = loaded.backup_path;
        let mut state = loaded.state;
        state.latest_snapshot = Some(snapshot.clone());
        state.history.insert(0, snapshot);
        truncate_history(&mut state.history);
        self.save_state(&state)?;
        Ok(self.outcome(state, status, backup_path))
    }

    pub fn update_settings(
        &self,
        settings: crate::models::Settings,
    ) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let status = mutation_status(&loaded.status);
        let backup_path = loaded.backup_path;
        let mut state = loaded.state;
        state.settings = settings;
        self.save_state(&state)?;
        Ok(self.outcome(state, status, backup_path))
    }

    pub fn update_manual_fields(
        &self,
        input: ManualUpdateInput,
    ) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let status = mutation_status(&loaded.status);
        let backup_path = loaded.backup_path;
        let mut state = loaded.state;
        let mut snapshot = state
            .latest_snapshot
            .clone()
            .unwrap_or_else(new_manual_snapshot);

        snapshot.source = SnapshotSource::Manual;
        snapshot.confidence = ConfidenceState::Manual;

        snapshot.remaining_percent = input.remaining_percent;
        snapshot.reset_at = input.reset_at;
        snapshot.credits_balance = input.credits_balance;
        snapshot.notes = input.notes.unwrap_or_default();
        snapshot.manual_fields = vec![
            ManualField::RemainingPercent,
            ManualField::ResetAt,
            ManualField::CreditsBalance,
            ManualField::Notes,
        ];

        state.latest_snapshot = Some(snapshot.clone());
        state.history.insert(0, snapshot);
        truncate_history(&mut state.history);
        self.save_state(&state)?;
        Ok(self.outcome(state, status, backup_path))
    }

    pub fn backup_and_reset(&self) -> Result<LoadOutcome, StoreError> {
        let backup_path = if self.path.exists() {
            Some(self.backup_existing_file("manual-reset")?)
        } else {
            None
        };
        let state = StoredState::default();
        self.save_state(&state)?;
        Ok(self.outcome(state, StorageStatus::Ready, backup_path))
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

fn truncate_history(history: &mut Vec<UsageSnapshot>) {
    if history.len() > DEFAULT_HISTORY_LIMIT {
        history.truncate(DEFAULT_HISTORY_LIMIT);
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

fn new_manual_snapshot() -> UsageSnapshot {
    UsageSnapshot {
        id: format!("manual-{}", unix_seconds()),
        source: SnapshotSource::Manual,
        parsed_at: unix_timestamp_string(),
        remaining_percent: None,
        reset_at: None,
        reset_countdown_seconds: None,
        credits_balance: None,
        model: None,
        context_window: None,
        confidence: ConfidenceState::Manual,
        raw_text: String::new(),
        manual_fields: Vec::new(),
        warnings: Vec::new(),
        notes: String::new(),
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn unix_timestamp_string() -> String {
    format!("unix:{}", unix_seconds())
}

#[cfg(test)]
mod tests {
    use crate::models::{
        ConfidenceState, ManualField, Settings, SnapshotSource, StorageStatus, StoredState,
        UsageSnapshot, DEFAULT_HISTORY_LIMIT, STATE_VERSION,
    };
    use crate::usage_store::UsageStore;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_time(seconds: i64) -> String {
        format!("2027-01-15T08:{:02}:00Z", seconds % 60)
    }

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

    fn snapshot(id: &str, percent: u8) -> UsageSnapshot {
        UsageSnapshot {
            id: id.to_string(),
            source: SnapshotSource::PastedStatus,
            parsed_at: test_time(1_800_000_000),
            remaining_percent: Some(percent),
            reset_at: None,
            reset_countdown_seconds: None,
            credits_balance: None,
            model: Some("gpt-5.5".to_string()),
            context_window: None,
            confidence: ConfidenceState::Fresh,
            raw_text: "remaining 72%".to_string(),
            manual_fields: Vec::new(),
            warnings: Vec::new(),
            notes: String::new(),
        }
    }

    #[test]
    fn missing_file_loads_default_state_without_creating_file() {
        let dir = TestDir::new("missing");
        let path = dir.path().join("state.json");
        let store = UsageStore::new(path.clone());

        let outcome = store.load().unwrap();

        assert_eq!(outcome.status, StorageStatus::Missing);
        assert_eq!(outcome.state.version, STATE_VERSION);
        assert_eq!(outcome.state.settings, Settings::default());
        assert!(outcome.state.latest_snapshot.is_none());
        assert!(!path.exists());
    }

    #[test]
    fn valid_v1_file_loads_state() {
        let dir = TestDir::new("valid");
        let path = dir.path().join("state.json");
        let store = UsageStore::new(path.clone());
        let state = StoredState {
            latest_snapshot: Some(snapshot("snap-1", 72)),
            ..StoredState::default()
        };
        store.save_state(&state).unwrap();

        let outcome = store.load().unwrap();

        assert_eq!(outcome.status, StorageStatus::Ready);
        assert_eq!(
            outcome.state.latest_snapshot.unwrap().remaining_percent,
            Some(72)
        );
    }

    #[test]
    fn corrupt_json_is_backed_up_and_recovered_to_default() {
        let dir = TestDir::new("corrupt");
        let path = dir.path().join("state.json");
        std::fs::write(&path, "{not valid json").unwrap();
        let store = UsageStore::new(path.clone());

        let outcome = store.load().unwrap();

        assert_eq!(outcome.status, StorageStatus::Recovered);
        let backup_path = outcome
            .backup_path
            .expect("corrupt file should be backed up");
        assert!(backup_path.exists());
        assert_eq!(
            std::fs::read_to_string(backup_path).unwrap(),
            "{not valid json"
        );
        assert_eq!(outcome.state, StoredState::default());
        assert!(path.exists());
    }

    #[test]
    fn invalid_utf8_state_is_backed_up_and_recovered_to_default() {
        let dir = TestDir::new("invalid-utf8");
        let path = dir.path().join("state.json");
        std::fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();
        let store = UsageStore::new(path.clone());

        let outcome = store.load().unwrap();

        assert_eq!(outcome.status, StorageStatus::Recovered);
        let backup_path = outcome
            .backup_path
            .expect("invalid bytes should be backed up");
        assert_eq!(std::fs::read(backup_path).unwrap(), [0xff, 0xfe, 0xfd]);
        assert_eq!(outcome.state, StoredState::default());
        assert!(path.exists());
    }

    #[test]
    fn unsupported_version_is_backed_up_and_recovered_to_default() {
        let dir = TestDir::new("unsupported");
        let path = dir.path().join("state.json");
        std::fs::write(
            &path,
            r#"{"version":99,"settings":{"staleAfterMinutes":60,"notifyBelowPercent":[20,10],"clipboardMonitoring":false},"latestSnapshot":null,"history":[]}"#,
        )
        .unwrap();
        let store = UsageStore::new(path);

        let outcome = store.load().unwrap();

        assert_eq!(outcome.status, StorageStatus::UnsupportedVersion);
        assert!(outcome.backup_path.unwrap().exists());
        assert_eq!(outcome.state, StoredState::default());
    }

    #[test]
    fn save_snapshot_updates_latest_and_truncates_history() {
        let dir = TestDir::new("truncate");
        let path = dir.path().join("state.json");
        let store = UsageStore::new(path);

        for index in 0..(DEFAULT_HISTORY_LIMIT + 5) {
            store
                .save_snapshot(snapshot(&format!("snap-{index}"), (index % 100) as u8))
                .unwrap();
        }

        let outcome = store.load().unwrap();
        assert_eq!(outcome.state.latest_snapshot.unwrap().id, "snap-104");
        assert_eq!(outcome.state.history.len(), DEFAULT_HISTORY_LIMIT);
        assert_eq!(outcome.state.history.first().unwrap().id, "snap-104");
        assert_eq!(outcome.state.history.last().unwrap().id, "snap-5");
    }

    #[test]
    fn backup_and_reset_preserves_existing_state_file() {
        let dir = TestDir::new("reset");
        let path = dir.path().join("state.json");
        let store = UsageStore::new(path.clone());
        store.save_snapshot(snapshot("snap-1", 44)).unwrap();

        let outcome = store.backup_and_reset().unwrap();

        assert_eq!(outcome.status, StorageStatus::Ready);
        assert!(outcome.backup_path.unwrap().exists());
        assert_eq!(outcome.state, StoredState::default());

        let reloaded = store.load().unwrap();
        assert_eq!(reloaded.state, StoredState::default());
    }

    #[test]
    fn repeated_backups_do_not_overwrite_each_other() {
        let dir = TestDir::new("backup-unique");
        let path = dir.path().join("state.json");
        let store = UsageStore::new(path.clone());

        std::fs::write(&path, "first corrupt content").unwrap();
        let first = store.load().unwrap().backup_path.unwrap();
        std::fs::write(&path, "second corrupt content").unwrap();
        let second = store.load().unwrap().backup_path.unwrap();

        assert_ne!(first, second);
        assert_eq!(
            std::fs::read_to_string(first).unwrap(),
            "first corrupt content"
        );
        assert_eq!(
            std::fs::read_to_string(second).unwrap(),
            "second corrupt content"
        );
    }

    #[test]
    fn save_snapshot_reports_recovery_metadata_after_corrupt_file() {
        let dir = TestDir::new("save-recovery");
        let path = dir.path().join("state.json");
        std::fs::write(&path, "{not valid json").unwrap();
        let store = UsageStore::new(path);

        let outcome = store.save_snapshot(snapshot("snap-1", 82)).unwrap();

        assert_eq!(outcome.status, StorageStatus::Recovered);
        assert!(outcome.backup_path.unwrap().exists());
        assert_eq!(outcome.state.latest_snapshot.unwrap().id, "snap-1");
    }

    #[test]
    fn manual_update_replaces_fields_and_can_clear_existing_values() {
        let dir = TestDir::new("manual");
        let path = dir.path().join("state.json");
        let store = UsageStore::new(path);
        store.save_snapshot(snapshot("snap-1", 44)).unwrap();

        let updated = store
            .update_manual_fields(crate::models::ManualUpdateInput {
                remaining_percent: Some(33),
                reset_at: Some(test_time(1_800_010_000)),
                credits_balance: Some(12.5),
                notes: Some("corrected from dashboard".to_string()),
            })
            .unwrap();

        let latest = updated.state.latest_snapshot.unwrap();
        assert_eq!(latest.confidence, ConfidenceState::Manual);
        assert_eq!(latest.remaining_percent, Some(33));
        assert_eq!(latest.credits_balance, Some(12.5));
        assert_eq!(latest.notes, "corrected from dashboard");
        assert_eq!(
            latest.manual_fields,
            vec![
                ManualField::RemainingPercent,
                ManualField::ResetAt,
                ManualField::CreditsBalance,
                ManualField::Notes
            ]
        );

        let cleared = store
            .update_manual_fields(crate::models::ManualUpdateInput {
                remaining_percent: None,
                reset_at: None,
                credits_balance: None,
                notes: None,
            })
            .unwrap();

        let latest = cleared.state.latest_snapshot.unwrap();
        assert_eq!(latest.remaining_percent, None);
        assert_eq!(latest.reset_at, None);
        assert_eq!(latest.credits_balance, None);
        assert_eq!(latest.notes, "");
        assert_eq!(
            latest.manual_fields,
            vec![
                ManualField::RemainingPercent,
                ManualField::ResetAt,
                ManualField::CreditsBalance,
                ManualField::Notes
            ]
        );
    }
}
