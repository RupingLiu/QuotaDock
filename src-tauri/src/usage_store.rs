use crate::models::{
    AppSettings, AppState, QuotaSnapshot, RecoveryNotice, SettingsPatch, StorageStatus,
    StoredState, UsageHistoryPoint, STATE_VERSION,
};
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const PREVIOUS_STATE_VERSIONS: [u32; 2] = [2, 3];
const MAX_HISTORY_POINTS: usize = 672;
const HISTORY_SAMPLE_INTERVAL_SECONDS: i64 = 15 * 60;
const MAX_BACKUP_FILES: usize = 3;

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
                let state = recovered_state(StorageStatus::Recovered, &backup_path);
                self.save_state(&state)?;
                return Ok(self.outcome(state, StorageStatus::Recovered, Some(backup_path)));
            }
        };
        let mut state = match serde_json::from_str::<StoredState>(&raw) {
            Ok(state) => state,
            Err(_) => {
                let backup_path = self.backup_existing_file("corrupt")?;
                let state = recovered_state(StorageStatus::Recovered, &backup_path);
                self.save_state(&state)?;
                return Ok(self.outcome(state, StorageStatus::Recovered, Some(backup_path)));
            }
        };

        if PREVIOUS_STATE_VERSIONS.contains(&state.version) {
            migrate_previous_state(&mut state);
            self.save_state(&state)?;
        } else if state.version != STATE_VERSION {
            let backup_path = self.backup_existing_file("unsupported")?;
            let state = recovered_state(StorageStatus::UnsupportedVersion, &backup_path);
            self.save_state(&state)?;
            return Ok(self.outcome(state, StorageStatus::UnsupportedVersion, Some(backup_path)));
        }

        let status = state
            .recovery_notice
            .as_ref()
            .map(|notice| notice.status.clone())
            .unwrap_or(StorageStatus::Ready);
        let backup_path = state
            .recovery_notice
            .as_ref()
            .map(|notice| PathBuf::from(&notice.backup_path));
        Ok(self.outcome(state, status, backup_path))
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
        let status = status_after_mutation(&loaded.status);
        let backup_path = loaded.backup_path;
        let mut state = loaded.state;
        append_history(&mut state.history, &snapshot);
        state.version = STATE_VERSION;
        state.latest_snapshot = Some(snapshot);
        self.save_state(&state)?;
        Ok(self.outcome(state, status, backup_path))
    }

    pub fn update_settings(&self, patch: SettingsPatch) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let status = status_after_mutation(&loaded.status);
        let backup_path = loaded.backup_path;
        let mut state = loaded.state;
        apply_settings_patch(&mut state.settings, patch);
        self.save_state(&state)?;
        Ok(self.outcome(state, status, backup_path))
    }

    pub fn acknowledge_recovery(&self) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let mut state = loaded.state;
        state.recovery_notice = None;
        self.save_state(&state)?;
        Ok(self.outcome(state, StorageStatus::Ready, None))
    }

    #[cfg(test)]
    pub fn clear_snapshot(&self) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let status = status_after_mutation(&loaded.status);
        let backup_path = loaded.backup_path;
        let mut state = loaded.state;
        state.latest_snapshot = None;
        state.history.clear();
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
        self.prune_old_backups(&backup_path)?;
        Ok(backup_path)
    }

    fn prune_old_backups(&self, newest: &Path) -> Result<(), StoreError> {
        let Some(parent) = self.path.parent() else {
            return Ok(());
        };
        let Some(stem) = self.path.file_stem().and_then(|stem| stem.to_str()) else {
            return Ok(());
        };
        let mut backups = std::fs::read_dir(parent)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path != newest
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with(stem) && name.ends_with(".bak"))
            })
            .filter_map(|path| {
                let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
                Some((modified, path))
            })
            .collect::<Vec<_>>();
        backups.sort_by_key(|(modified, _)| *modified);
        let remove_count = backups
            .len()
            .saturating_add(1)
            .saturating_sub(MAX_BACKUP_FILES);
        for (_, path) in backups.into_iter().take(remove_count) {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

fn migrate_previous_state(state: &mut StoredState) {
    state.version = STATE_VERSION;
    state
        .history
        .retain(|point| point.weekly_remaining_percent.is_some());
    if state
        .latest_snapshot
        .as_ref()
        .is_some_and(|snapshot| !snapshot.has_usage())
    {
        state.latest_snapshot = None;
    } else if let Some(snapshot) = state.latest_snapshot.as_mut() {
        snapshot.raw_text.clear();
        snapshot.warnings.clear();
        snapshot.status_message = "已更新 1 周额度。".to_string();
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

fn status_after_mutation(status: &StorageStatus) -> StorageStatus {
    match status {
        StorageStatus::Recovered => StorageStatus::Recovered,
        StorageStatus::UnsupportedVersion => StorageStatus::UnsupportedVersion,
        StorageStatus::Ready | StorageStatus::Missing => StorageStatus::Ready,
    }
}

fn recovered_state(status: StorageStatus, backup_path: &Path) -> StoredState {
    let message = match status {
        StorageStatus::Recovered => "本地状态文件损坏，已备份并恢复默认状态。",
        StorageStatus::UnsupportedVersion => "本地状态版本不兼容，已备份并重建状态。",
        StorageStatus::Ready | StorageStatus::Missing => "本地状态已恢复。",
    };
    StoredState {
        recovery_notice: Some(RecoveryNotice {
            status,
            message: message.to_string(),
            backup_path: backup_path.display().to_string(),
        }),
        ..StoredState::default()
    }
}

fn apply_settings_patch(settings: &mut AppSettings, patch: SettingsPatch) {
    if let Some(enabled) = patch.automatic_update_checks {
        settings.automatic_update_checks = enabled;
    }
    if let Some(enabled) = patch.low_quota_notifications {
        settings.low_quota_notifications = enabled;
    }
}

fn append_history(history: &mut Vec<UsageHistoryPoint>, snapshot: &QuotaSnapshot) {
    if !snapshot.has_usage() {
        return;
    }
    let next = UsageHistoryPoint::from(snapshot);
    if history
        .last()
        .is_some_and(|previous| !should_record_history(previous, &next))
    {
        return;
    }
    history.push(next);
    if history.len() > MAX_HISTORY_POINTS {
        history.drain(..history.len() - MAX_HISTORY_POINTS);
    }
}

fn should_record_history(previous: &UsageHistoryPoint, next: &UsageHistoryPoint) -> bool {
    if previous.weekly_remaining_percent != next.weekly_remaining_percent {
        return true;
    }
    match (
        unix_seconds(&previous.captured_at),
        unix_seconds(&next.captured_at),
    ) {
        (Some(previous), Some(next)) => next - previous >= HISTORY_SAMPLE_INTERVAL_SECONDS,
        _ => previous.captured_at != next.captured_at,
    }
}

fn unix_seconds(value: &str) -> Option<i64> {
    value.strip_prefix("unix:")?.trim().parse().ok()
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use crate::models::{
        QuotaReading, QuotaSnapshot, SettingsPatch, SnapshotSource, StorageStatus, StoredState,
        STATE_VERSION,
    };
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
            weekly: QuotaReading {
                remaining_percent: Some(percent),
                reset_at: Some("2026-06-23T09:00:00Z".to_string()),
                reset_countdown_seconds: None,
            },
            plan_type: None,
            credits_balance: None,
            reset_credits_available: None,
            raw_text: "status".to_string(),
            status_message: "已更新 1 周额度。".to_string(),
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
    fn valid_current_file_loads_state() {
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
                .weekly
                .remaining_percent,
            Some(72)
        );
    }

    #[test]
    fn previous_state_versions_are_migrated_without_losing_weekly_usage() {
        for version in [2, 3] {
            let dir = TestDir::new(&format!("migrate-v{version}"));
            let path = dir.path().join("state.json");
            let json = format!(
                r#"{{
                  "version": {version},
                  "latestSnapshot": {{
                    "id": "legacy",
                    "source": "codex-cli",
                    "capturedAt": "unix:1000",
                    "removedWindow": {{"remainingPercent": 71}},
                    "weekly": {{"remainingPercent": 45, "resetAt": null, "resetCountdownSeconds": null}},
                    "rawText": "legacy terminal output",
                    "statusMessage": "legacy status",
                    "warnings": [{{"code": "removed-capability", "message": "legacy warning"}}]
                  }}
                }}"#
            );
            std::fs::write(&path, json).unwrap();
            let store = UsageStore::new(path.clone());

            let outcome = store.load().unwrap();

            assert_eq!(outcome.status, StorageStatus::Ready);
            assert_eq!(outcome.state.version, STATE_VERSION);
            let migrated_snapshot = outcome.state.latest_snapshot.as_ref().unwrap();
            assert_eq!(migrated_snapshot.weekly.remaining_percent, Some(45));
            assert_eq!(migrated_snapshot.status_message, "已更新 1 周额度。");
            assert!(migrated_snapshot.raw_text.is_empty());
            assert!(migrated_snapshot.warnings.is_empty());
            let persisted_text = std::fs::read_to_string(path).unwrap();
            assert!(!persisted_text.contains("removedWindow"));
            assert!(!persisted_text.contains("legacy terminal output"));
            assert!(!persisted_text.contains("legacy warning"));
            let persisted: StoredState = serde_json::from_str(&persisted_text).unwrap();
            assert_eq!(persisted.version, STATE_VERSION);
        }
    }

    #[test]
    fn migration_drops_snapshots_and_history_without_weekly_usage() {
        let dir = TestDir::new("migrate-empty-weekly");
        let path = dir.path().join("state.json");
        std::fs::write(
            &path,
            r#"{
              "version": 3,
              "latestSnapshot": {
                "id": "legacy-empty",
                "source": "codex-cli",
                "capturedAt": "unix:1000",
                "weekly": {"remainingPercent": null, "resetAt": null, "resetCountdownSeconds": null},
                "rawText": "legacy terminal output",
                "statusMessage": "legacy status",
                "warnings": [{"code": "removed-capability", "message": "legacy warning"}]
              },
              "history": [
                {"capturedAt": "unix:1000", "weeklyRemainingPercent": null}
              ]
            }"#,
        )
        .unwrap();
        let store = UsageStore::new(path);

        let outcome = store.load().unwrap();

        assert_eq!(outcome.state.version, STATE_VERSION);
        assert!(outcome.state.latest_snapshot.is_none());
        assert!(outcome.state.history.is_empty());
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
        assert!(outcome.state.latest_snapshot.is_none());
        assert_eq!(
            outcome.state.recovery_notice.as_ref().unwrap().status,
            StorageStatus::Recovered
        );

        let still_visible = store.load().unwrap();
        assert_eq!(still_visible.status, StorageStatus::Recovered);
        assert!(still_visible.state.recovery_notice.is_some());
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
        assert!(outcome.state.latest_snapshot.is_none());
        assert_eq!(
            outcome.state.recovery_notice.as_ref().unwrap().status,
            StorageStatus::UnsupportedVersion
        );
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
        assert!(cleared.state.history.is_empty());
    }

    #[test]
    fn settings_and_recovery_acknowledgement_are_persisted() {
        let dir = TestDir::new("settings");
        let path = dir.path().join("state.json");
        std::fs::write(&path, "{broken").unwrap();
        let store = UsageStore::new(path);
        store.load().unwrap();

        let updated = store
            .update_settings(SettingsPatch {
                automatic_update_checks: Some(false),
                low_quota_notifications: Some(true),
            })
            .unwrap();
        assert!(!updated.state.settings.automatic_update_checks);
        assert!(updated.state.settings.low_quota_notifications);
        assert!(updated.state.recovery_notice.is_some());

        let acknowledged = store.acknowledge_recovery().unwrap();
        assert_eq!(acknowledged.status, StorageStatus::Ready);
        assert!(acknowledged.state.recovery_notice.is_none());
        let reloaded = store.load().unwrap();
        assert!(!reloaded.state.settings.automatic_update_checks);
        assert!(reloaded.state.settings.low_quota_notifications);
        assert!(reloaded.state.recovery_notice.is_none());
    }

    #[test]
    fn history_samples_changes_and_periodic_unchanged_values() {
        let dir = TestDir::new("history");
        let store = UsageStore::new(dir.path().join("state.json"));

        store.save_snapshot(snapshot(72)).unwrap();
        let unchanged_too_soon = {
            let mut value = snapshot(72);
            value.id = "snap-2".to_string();
            value.captured_at = "unix:1500".to_string();
            value
        };
        store.save_snapshot(unchanged_too_soon).unwrap();
        let changed = {
            let mut value = snapshot(71);
            value.id = "snap-3".to_string();
            value.captured_at = "unix:1600".to_string();
            value
        };
        store.save_snapshot(changed).unwrap();
        let periodic = {
            let mut value = snapshot(71);
            value.id = "snap-4".to_string();
            value.captured_at = "unix:2500".to_string();
            value
        };
        let outcome = store.save_snapshot(periodic).unwrap();

        assert_eq!(outcome.state.history.len(), 3);
        assert_eq!(outcome.state.history[1].weekly_remaining_percent, Some(71));
    }
}
