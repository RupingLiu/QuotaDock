use crate::models::{
    AppSettings, AppState, ParseWarning, ProviderErrorCategory, ProviderHealth, ProviderId,
    ProviderSnapshot, ProviderStates, QuotaReading, QuotaSnapshot, RecoveryNotice, SettingsPatch,
    SnapshotSource, StorageStatus, StoredState, UsageHistoryPoint, STATE_VERSION,
};
use serde::Deserialize;
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const PREVIOUS_STATE_VERSIONS: [u32; 3] = [2, 3, 4];
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderRefreshMutationKind {
    Updated(ProviderSnapshot),
    Failed {
        category: ProviderErrorCategory,
        configured: Option<bool>,
    },
    NotConfigured,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRefreshMutation {
    pub provider_id: ProviderId,
    pub attempted_at: String,
    pub kind: ProviderRefreshMutationKind,
}

#[derive(Deserialize)]
struct StateVersion {
    version: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyStoredState {
    version: u32,
    #[serde(default)]
    latest_snapshot: Option<LegacyQuotaSnapshot>,
    #[serde(default)]
    history: Vec<UsageHistoryPoint>,
    #[serde(default)]
    settings: AppSettings,
    #[serde(default)]
    recovery_notice: Option<RecoveryNotice>,
}

// v2/v3 snapshots included fields that no longer exist. Keep migration input
// intentionally tolerant while the current v5 persisted DTOs remain strict.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyQuotaSnapshot {
    id: String,
    source: SnapshotSource,
    captured_at: String,
    weekly: QuotaReading,
    #[serde(default)]
    plan_type: Option<String>,
    #[serde(default)]
    credits_balance: Option<String>,
    #[serde(default)]
    reset_credits_available: Option<u32>,
    #[serde(default)]
    raw_text: String,
    #[serde(default)]
    status_message: String,
    #[serde(default)]
    warnings: Vec<ParseWarning>,
}

impl LegacyQuotaSnapshot {
    fn has_usage(&self) -> bool {
        self.weekly.has_usage()
    }
}

impl From<LegacyQuotaSnapshot> for QuotaSnapshot {
    fn from(snapshot: LegacyQuotaSnapshot) -> Self {
        Self {
            id: snapshot.id,
            source: snapshot.source,
            captured_at: snapshot.captured_at,
            weekly: snapshot.weekly,
            plan_type: snapshot.plan_type,
            credits_balance: snapshot.credits_balance,
            reset_credits_available: snapshot.reset_credits_available,
            raw_text: snapshot.raw_text,
            status_message: snapshot.status_message,
            warnings: snapshot.warnings,
        }
    }
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
        let version = match serde_json::from_str::<StateVersion>(&raw) {
            Ok(version) => version.version,
            Err(_) => {
                let backup_path = self.backup_existing_file("corrupt")?;
                let state = recovered_state(StorageStatus::Recovered, &backup_path);
                self.save_state(&state)?;
                return Ok(self.outcome(state, StorageStatus::Recovered, Some(backup_path)));
            }
        };

        if version != STATE_VERSION && !PREVIOUS_STATE_VERSIONS.contains(&version) {
            let backup_path = self.backup_existing_file("unsupported")?;
            let state = recovered_state(StorageStatus::UnsupportedVersion, &backup_path);
            self.save_state(&state)?;
            return Ok(self.outcome(state, StorageStatus::UnsupportedVersion, Some(backup_path)));
        }

        let (mut state, migrated) = if version == STATE_VERSION {
            match serde_json::from_str::<StoredState>(&raw) {
                Ok(state) => (state, false),
                Err(_) => {
                    let backup_path = self.backup_existing_file("corrupt")?;
                    let state = recovered_state(StorageStatus::Recovered, &backup_path);
                    self.save_state(&state)?;
                    return Ok(self.outcome(state, StorageStatus::Recovered, Some(backup_path)));
                }
            }
        } else {
            match serde_json::from_str::<LegacyStoredState>(&raw) {
                Ok(legacy) => (migrate_previous_state(legacy), true),
                Err(_) => {
                    let backup_path = self.backup_existing_file("corrupt")?;
                    let state = recovered_state(StorageStatus::Recovered, &backup_path);
                    self.save_state(&state)?;
                    return Ok(self.outcome(state, StorageStatus::Recovered, Some(backup_path)));
                }
            }
        };

        let before_normalization = state.clone();
        normalize_state(&mut state);
        if migrated || state != before_normalization {
            self.save_state(&state)?;
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
        let mut normalized = state.clone();
        normalize_state(&mut normalized);
        let json = serde_json::to_string_pretty(&normalized)?;
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

    #[cfg(test)]
    pub fn save_snapshot(&self, mut snapshot: QuotaSnapshot) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let status = status_after_mutation(&loaded.status);
        let backup_path = loaded.backup_path;
        let mut state = loaded.state;
        snapshot.raw_text.clear();
        append_history(&mut state.history, &snapshot);
        state.version = STATE_VERSION;
        state.providers.codex.configured = true;
        state.providers.codex.last_attempt_at = Some(snapshot.captured_at.clone());
        state.providers.codex.latest_snapshot = Some(ProviderSnapshot::Codex(snapshot));
        state.providers.codex.health = ProviderHealth::Fresh;
        state.providers.codex.error_category = None;
        bump_revision(&mut state);
        self.save_state(&state)?;
        Ok(self.outcome(state, status, backup_path))
    }

    pub fn update_settings(&self, patch: SettingsPatch) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let status = status_after_mutation(&loaded.status);
        let backup_path = loaded.backup_path;
        let mut state = loaded.state;
        let before = state.settings.clone();
        apply_settings_patch(&mut state.settings, &state.providers, patch);
        if state.settings != before {
            bump_revision(&mut state);
        }
        self.save_state(&state)?;
        Ok(self.outcome(state, status, backup_path))
    }

    pub fn set_provider_configured(
        &self,
        provider_id: ProviderId,
        configured: bool,
    ) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let status = status_after_mutation(&loaded.status);
        let backup_path = loaded.backup_path;
        let mut state = loaded.state;
        let before = state.clone();
        let provider = state.providers.get_mut(provider_id);
        provider.configured = configured || provider_id == ProviderId::Codex;
        provider.error_category = None;
        provider.health = if provider.configured {
            if provider.latest_snapshot.is_some() {
                ProviderHealth::Stale
            } else {
                ProviderHealth::Idle
            }
        } else {
            ProviderHealth::NotConfigured
        };
        state
            .settings
            .normalize_floating_provider_ids(&state.providers);
        if state != before {
            bump_revision(&mut state);
        }
        self.save_state(&state)?;
        Ok(self.outcome(state, status, backup_path))
    }

    pub fn sync_provider_configurations(
        &self,
        configurations: &[(ProviderId, bool)],
    ) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let status = status_after_mutation(&loaded.status);
        let backup_path = loaded.backup_path;
        let mut state = loaded.state;
        let before = state.clone();
        for (provider_id, configured) in configurations {
            if *provider_id == ProviderId::Codex {
                continue;
            }
            let provider = state.providers.get_mut(*provider_id);
            if provider.configured == *configured {
                continue;
            }
            provider.configured = *configured;
            provider.error_category = None;
            provider.health = if *configured {
                if provider.latest_snapshot.is_some() {
                    ProviderHealth::Stale
                } else {
                    ProviderHealth::Idle
                }
            } else {
                ProviderHealth::NotConfigured
            };
        }
        state
            .settings
            .normalize_floating_provider_ids(&state.providers);
        if state != before {
            bump_revision(&mut state);
            self.save_state(&state)?;
        }
        Ok(self.outcome(state, status, backup_path))
    }

    pub fn apply_provider_refreshes(
        &self,
        mutations: Vec<ProviderRefreshMutation>,
    ) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let status = status_after_mutation(&loaded.status);
        let backup_path = loaded.backup_path;
        let mut state = loaded.state;
        let before = state.clone();

        for mutation in mutations {
            let provider = state.providers.get_mut(mutation.provider_id);
            match mutation.kind {
                ProviderRefreshMutationKind::Updated(mut snapshot) => {
                    if snapshot.provider_id() != mutation.provider_id {
                        continue;
                    }
                    if let ProviderSnapshot::Codex(snapshot) = &mut snapshot {
                        snapshot.raw_text.clear();
                        append_history(&mut state.history, snapshot);
                    }
                    provider.configured = true;
                    provider.latest_snapshot = Some(snapshot);
                    provider.last_attempt_at = Some(mutation.attempted_at);
                    provider.health = ProviderHealth::Fresh;
                    provider.error_category = None;
                }
                ProviderRefreshMutationKind::Failed {
                    category,
                    configured,
                } => {
                    if let Some(configured) = configured {
                        provider.configured = configured;
                    }
                    provider.last_attempt_at = Some(mutation.attempted_at);
                    provider.health = if provider.latest_snapshot.is_some() {
                        ProviderHealth::Stale
                    } else {
                        ProviderHealth::Error
                    };
                    provider.error_category = Some(category);
                }
                ProviderRefreshMutationKind::NotConfigured => {
                    if mutation.provider_id != ProviderId::Codex {
                        provider.configured = false;
                    }
                    provider.health = if provider.configured {
                        ProviderHealth::Idle
                    } else {
                        ProviderHealth::NotConfigured
                    };
                    provider.error_category = None;
                }
            }
        }
        state
            .settings
            .normalize_floating_provider_ids(&state.providers);
        if state != before {
            bump_revision(&mut state);
        }
        self.save_state(&state)?;
        Ok(self.outcome(state, status, backup_path))
    }

    pub fn acknowledge_recovery(&self) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let mut state = loaded.state;
        let changed = state.recovery_notice.is_some();
        state.recovery_notice = None;
        if changed {
            bump_revision(&mut state);
        }
        self.save_state(&state)?;
        Ok(self.outcome(state, StorageStatus::Ready, None))
    }

    #[cfg(test)]
    pub fn clear_snapshot(&self) -> Result<LoadOutcome, StoreError> {
        let loaded = self.load()?;
        let status = status_after_mutation(&loaded.status);
        let backup_path = loaded.backup_path;
        let mut state = loaded.state;
        state.providers.codex.latest_snapshot = None;
        state.providers.codex.last_attempt_at = None;
        state.providers.codex.health = ProviderHealth::Idle;
        state.providers.codex.error_category = None;
        state.history.clear();
        bump_revision(&mut state);
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

fn migrate_previous_state(mut legacy: LegacyStoredState) -> StoredState {
    if matches!(legacy.version, 2 | 3) {
        legacy
            .history
            .retain(|point| point.weekly_remaining_percent.is_some());
        if legacy
            .latest_snapshot
            .as_ref()
            .is_some_and(|snapshot| !snapshot.has_usage())
        {
            legacy.latest_snapshot = None;
        } else if let Some(snapshot) = legacy.latest_snapshot.as_mut() {
            snapshot.raw_text.clear();
            snapshot.warnings.clear();
            snapshot.status_message = "已更新 1 周额度。".to_string();
        }
    }

    let mut providers = ProviderStates::default();
    if let Some(snapshot) = legacy.latest_snapshot.map(QuotaSnapshot::from) {
        providers.codex.last_attempt_at = Some(snapshot.captured_at.clone());
        providers.codex.latest_snapshot = Some(ProviderSnapshot::Codex(snapshot));
        providers.codex.health = ProviderHealth::Fresh;
    }
    let mut settings = legacy.settings;
    settings.floating_provider_ids = vec![ProviderId::Codex];

    StoredState {
        version: STATE_VERSION,
        revision: 0,
        providers,
        history: legacy.history,
        settings,
        recovery_notice: legacy.recovery_notice,
    }
}

fn normalize_state(state: &mut StoredState) {
    state.version = STATE_VERSION;
    state.providers.normalize();
    if let Some(ProviderSnapshot::Codex(snapshot)) = state.providers.codex.latest_snapshot.as_mut()
    {
        snapshot.raw_text.clear();
    }
    state
        .settings
        .normalize_floating_provider_ids(&state.providers);
}

fn bump_revision(state: &mut StoredState) {
    state.revision = state.revision.saturating_add(1);
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

fn apply_settings_patch(
    settings: &mut AppSettings,
    providers: &ProviderStates,
    patch: SettingsPatch,
) {
    if let Some(enabled) = patch.automatic_update_checks {
        settings.automatic_update_checks = enabled;
    }
    if let Some(enabled) = patch.low_quota_notifications {
        settings.low_quota_notifications = enabled;
    }
    if let Some(provider_ids) = patch.floating_provider_ids {
        settings.floating_provider_ids = provider_ids;
    }
    settings.normalize_floating_provider_ids(providers);
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
        ProviderHealth, ProviderId, QuotaReading, QuotaSnapshot, SettingsPatch, SnapshotSource,
        StorageStatus, StoredState, STATE_VERSION,
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

    fn codex_snapshot(state: &StoredState) -> Option<&QuotaSnapshot> {
        state.providers.codex_snapshot()
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
            codex_snapshot(&outcome.state)
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
            let migrated_snapshot = codex_snapshot(&outcome.state).unwrap();
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
        assert!(codex_snapshot(&outcome.state).is_none());
        assert!(outcome.state.history.is_empty());
    }

    #[test]
    fn v4_migration_preserves_complete_codex_snapshot_sparse_history_and_settings() {
        let dir = TestDir::new("migrate-v4-lossless");
        let path = dir.path().join("state.json");
        std::fs::write(
            &path,
            r#"{
              "version": 4,
              "latestSnapshot": {
                "id": "v4-snapshot",
                "source": "codex-app-server",
                "capturedAt": "unix:4242",
                "weekly": {"remainingPercent": 37, "resetAt": "unix:9000", "resetCountdownSeconds": 4758},
                "planType": "plus",
                "creditsBalance": "12.3400",
                "resetCreditsAvailable": 7,
                "rawText": "v4 raw payload",
                "statusMessage": "v4 exact status",
                "warnings": [{"code": "v4-warning", "message": "keep me"}]
              },
              "history": [
                {"capturedAt": "unix:1000", "weeklyRemainingPercent": null},
                {"capturedAt": "unix:2000", "weeklyRemainingPercent": 38}
              ],
              "settings": {
                "automaticUpdateChecks": false,
                "lowQuotaNotifications": true
              }
            }"#,
        )
        .unwrap();
        let store = UsageStore::new(path.clone());

        let outcome = store.load().unwrap();
        let migrated = codex_snapshot(&outcome.state).unwrap();

        assert_eq!(outcome.status, StorageStatus::Ready);
        assert_eq!(outcome.state.version, STATE_VERSION);
        assert_eq!(migrated.id, "v4-snapshot");
        assert_eq!(migrated.weekly.remaining_percent, Some(37));
        assert_eq!(migrated.plan_type.as_deref(), Some("plus"));
        assert_eq!(migrated.credits_balance.as_deref(), Some("12.3400"));
        assert_eq!(migrated.reset_credits_available, Some(7));
        assert!(migrated.raw_text.is_empty());
        assert_eq!(migrated.status_message, "v4 exact status");
        assert_eq!(migrated.warnings[0].code, "v4-warning");
        assert_eq!(outcome.state.history.len(), 2);
        assert_eq!(outcome.state.history[0].weekly_remaining_percent, None);
        assert!(!outcome.state.settings.automatic_update_checks);
        assert!(outcome.state.settings.low_quota_notifications);
        assert_eq!(
            outcome.state.settings.floating_provider_ids,
            [ProviderId::Codex]
        );
        assert!(!outcome.state.providers.deepseek.configured);
        assert!(!outcome.state.providers.kimi.configured);

        let persisted = std::fs::read_to_string(path).unwrap();
        let persisted_value: serde_json::Value = serde_json::from_str(&persisted).unwrap();
        assert!(persisted_value.get("latestSnapshot").is_none());
        assert!(persisted.contains("\"providers\""));
        assert!(persisted.contains("\"provider\": \"codex\""));
        assert!(!persisted.contains("v4 raw payload"));
    }

    #[test]
    fn v4_empty_state_migrates_to_three_provider_states() {
        let dir = TestDir::new("migrate-v4-empty");
        let path = dir.path().join("state.json");
        std::fs::write(&path, r#"{"version":4,"latestSnapshot":null}"#).unwrap();
        let store = UsageStore::new(path);

        let outcome = store.load().unwrap();

        assert!(outcome.state.providers.codex.configured);
        assert_eq!(outcome.state.providers.codex.health, ProviderHealth::Idle);
        assert!(!outcome.state.providers.deepseek.configured);
        assert_eq!(
            outcome.state.providers.deepseek.health,
            ProviderHealth::NotConfigured
        );
        assert!(!outcome.state.providers.kimi.configured);
        assert_eq!(
            outcome.state.settings.floating_provider_ids,
            [ProviderId::Codex]
        );
    }

    #[test]
    fn failed_v4_migration_backs_up_before_recovering() {
        let dir = TestDir::new("migrate-v4-invalid");
        let path = dir.path().join("state.json");
        let invalid = r#"{"version":4,"latestSnapshot":{"id":17}}"#;
        std::fs::write(&path, invalid).unwrap();
        let store = UsageStore::new(path);

        let outcome = store.load().unwrap();

        assert_eq!(outcome.status, StorageStatus::Recovered);
        let backup_path = outcome.backup_path.unwrap();
        assert!(backup_path.exists());
        assert_eq!(std::fs::read_to_string(backup_path).unwrap(), invalid);
        assert!(outcome.state.recovery_notice.is_some());
    }

    #[test]
    fn malformed_v5_provider_tag_is_cleared_with_stale_metadata() {
        let dir = TestDir::new("normalize-v5-provider-tag");
        let path = dir.path().join("state.json");
        std::fs::write(
            &path,
            r#"{
              "version": 5,
              "providers": {
                "codex": {
                  "configured": false,
                  "latestSnapshot": {
                    "provider": "deepseek",
                    "data": {
                      "id": "wrong-provider",
                      "capturedAt": "unix:1000",
                      "isAvailable": true,
                      "balances": []
                    }
                  },
                  "lastAttemptAt": "unix:1001",
                  "health": "fresh",
                  "errorCategory": "unauthorized"
                }
              }
            }"#,
        )
        .unwrap();
        let store = UsageStore::new(path.clone());

        let outcome = store.load().unwrap();

        assert!(outcome.state.providers.codex.configured);
        assert!(outcome.state.providers.codex.latest_snapshot.is_none());
        assert!(outcome.state.providers.codex.last_attempt_at.is_none());
        assert_eq!(outcome.state.providers.codex.health, ProviderHealth::Idle);
        assert!(outcome.state.providers.codex.error_category.is_none());

        let persisted: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(persisted["providers"]["codex"]["latestSnapshot"].is_null());
        assert!(persisted["providers"]["codex"]["lastAttemptAt"].is_null());
        assert_eq!(persisted["providers"]["codex"]["health"], "idle");
        assert!(persisted["providers"]["codex"]["errorCategory"].is_null());
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
        assert!(codex_snapshot(&outcome.state).is_none());
        assert_eq!(
            outcome.state.recovery_notice.as_ref().unwrap().status,
            StorageStatus::Recovered
        );

        let still_visible = store.load().unwrap();
        assert_eq!(still_visible.status, StorageStatus::Recovered);
        assert!(still_visible.state.recovery_notice.is_some());
    }

    #[test]
    fn unknown_v5_fields_are_rejected_without_copying_them_into_active_state() {
        let dir = TestDir::new("unknown-v5-field");
        let path = dir.path().join("state.json");
        let externally_injected_secret = "externally-injected-key";
        let mut value = serde_json::to_value(StoredState::default()).unwrap();
        value["apiKey"] = serde_json::Value::String(externally_injected_secret.to_string());
        let polluted = serde_json::to_string_pretty(&value).unwrap();
        std::fs::write(&path, &polluted).unwrap();
        let store = UsageStore::new(path.clone());

        let outcome = store.load().unwrap();

        assert_eq!(outcome.status, StorageStatus::Recovered);
        let active = std::fs::read_to_string(path).unwrap();
        assert!(!active.contains(externally_injected_secret));
        let backup = std::fs::read_to_string(outcome.backup_path.unwrap()).unwrap();
        assert_eq!(backup, polluted);
        assert!(backup.contains(externally_injected_secret));
    }

    #[test]
    fn nested_provider_fields_are_rejected_and_only_preserved_in_the_raw_recovery_backup() {
        let dir = TestDir::new("nested-provider-unknown-field");
        let path = dir.path().join("state.json");
        let externally_injected_secret = "nested-externally-injected-key";
        let mut value = serde_json::to_value(StoredState::default()).unwrap();
        value["providers"]["deepseek"]["apiKey"] =
            serde_json::Value::String(externally_injected_secret.to_string());
        let polluted = serde_json::to_string_pretty(&value).unwrap();
        std::fs::write(&path, &polluted).unwrap();
        let store = UsageStore::new(path.clone());

        let outcome = store.load().unwrap();

        assert_eq!(outcome.status, StorageStatus::Recovered);
        assert!(!std::fs::read_to_string(path)
            .unwrap()
            .contains(externally_injected_secret));
        let backup = std::fs::read_to_string(outcome.backup_path.unwrap()).unwrap();
        assert_eq!(backup, polluted);
        assert!(backup.contains(externally_injected_secret));
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
        assert!(codex_snapshot(&outcome.state).is_none());
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
        assert!(codex_snapshot(&saved.state).is_some());
        assert!(codex_snapshot(&saved.state).unwrap().raw_text.is_empty());

        let cleared = store.clear_snapshot().unwrap();
        assert!(codex_snapshot(&cleared.state).is_none());
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
                floating_provider_ids: None,
            })
            .unwrap();
        assert!(!updated.state.settings.automatic_update_checks);
        assert!(updated.state.settings.low_quota_notifications);
        assert!(updated.state.recovery_notice.is_some());

        let normalized = store
            .update_settings(SettingsPatch {
                automatic_update_checks: None,
                low_quota_notifications: None,
                floating_provider_ids: Some(vec![ProviderId::Kimi, ProviderId::Kimi]),
            })
            .unwrap();
        assert_eq!(
            normalized.state.settings.floating_provider_ids,
            [ProviderId::Codex]
        );

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
