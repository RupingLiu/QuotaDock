use serde::{Deserialize, Serialize};

pub const STATE_VERSION: u32 = 4;
pub const DEFAULT_STATUS_MESSAGE: &str = "尚未获取额度。可通过托盘刷新，后台也会自动查询。";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotSource {
    PastedStatus,
    CodexCli,
    CodexAppServer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaReading {
    pub remaining_percent: Option<u8>,
    pub reset_at: Option<String>,
    pub reset_countdown_seconds: Option<i64>,
}

impl QuotaReading {
    pub fn has_usage(&self) -> bool {
        self.remaining_percent.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSnapshot {
    pub id: String,
    pub source: SnapshotSource,
    pub captured_at: String,
    pub weekly: QuotaReading,
    #[serde(default)]
    pub plan_type: Option<String>,
    #[serde(default)]
    pub credits_balance: Option<String>,
    #[serde(default)]
    pub reset_credits_available: Option<u32>,
    pub raw_text: String,
    pub status_message: String,
    pub warnings: Vec<ParseWarning>,
}

impl QuotaSnapshot {
    pub fn has_usage(&self) -> bool {
        self.weekly.has_usage()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StorageStatus {
    Ready,
    Missing,
    Recovered,
    UnsupportedVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageHistoryPoint {
    pub captured_at: String,
    pub weekly_remaining_percent: Option<u8>,
}

impl From<&QuotaSnapshot> for UsageHistoryPoint {
    fn from(snapshot: &QuotaSnapshot) -> Self {
        Self {
            captured_at: snapshot.captured_at.clone(),
            weekly_remaining_percent: snapshot.weekly.remaining_percent,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub automatic_update_checks: bool,
    pub low_quota_notifications: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            automatic_update_checks: true,
            low_quota_notifications: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryNotice {
    pub status: StorageStatus,
    pub message: String,
    pub backup_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredState {
    pub version: u32,
    pub latest_snapshot: Option<QuotaSnapshot>,
    #[serde(default)]
    pub history: Vec<UsageHistoryPoint>,
    #[serde(default)]
    pub settings: AppSettings,
    #[serde(default)]
    pub recovery_notice: Option<RecoveryNotice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub version: u32,
    pub latest_snapshot: Option<QuotaSnapshot>,
    pub storage_status: StorageStatus,
    pub storage_path: Option<String>,
    pub backup_path: Option<String>,
    pub status_message: String,
    pub history: Vec<UsageHistoryPoint>,
    pub settings: AppSettings,
    pub recovery_notice: Option<RecoveryNotice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshUsageResult {
    pub app_state: AppState,
    pub updated: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub automatic_update_checks: Option<bool>,
    pub low_quota_notifications: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDiagnostics {
    pub app_version: String,
    pub codex_path: Option<String>,
    pub codex_version: Option<String>,
    pub latest_source: Option<SnapshotSource>,
    pub latest_success_at: Option<String>,
    pub storage_path: Option<String>,
    pub storage_status: StorageStatus,
    pub startup_enabled: bool,
    pub signed_updates_enabled: bool,
}

impl Default for StoredState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            latest_snapshot: None,
            history: Vec::new(),
            settings: AppSettings::default(),
            recovery_notice: None,
        }
    }
}

impl AppState {
    pub fn from_stored(
        stored: StoredState,
        storage_status: StorageStatus,
        storage_path: Option<String>,
        backup_path: Option<String>,
    ) -> Self {
        let status_message = stored
            .recovery_notice
            .as_ref()
            .map(|notice| notice.message.clone())
            .or_else(|| {
                stored
                    .latest_snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.status_message.clone())
            })
            .unwrap_or_else(|| DEFAULT_STATUS_MESSAGE.to_string());
        let backup_path = stored
            .recovery_notice
            .as_ref()
            .map(|notice| notice.backup_path.clone())
            .or(backup_path);

        Self {
            version: stored.version,
            latest_snapshot: stored.latest_snapshot,
            storage_status,
            storage_path,
            backup_path,
            status_message,
            history: stored.history,
            settings: stored.settings,
            recovery_notice: stored.recovery_notice,
        }
    }
}
