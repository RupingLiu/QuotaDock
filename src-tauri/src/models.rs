use serde::{Deserialize, Serialize};

pub const STATE_VERSION: u32 = 2;
pub const DEFAULT_STATUS_MESSAGE: &str = "尚未获取额度。可通过托盘刷新，后台也会自动查询。";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotSource {
    PastedStatus,
    CodexCli,
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
    pub fn has_value(&self) -> bool {
        self.remaining_percent.is_some()
            || self.reset_at.is_some()
            || self.reset_countdown_seconds.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSnapshot {
    pub id: String,
    pub source: SnapshotSource,
    pub captured_at: String,
    pub five_hour: QuotaReading,
    pub weekly: QuotaReading,
    pub raw_text: String,
    pub status_message: String,
    pub warnings: Vec<ParseWarning>,
}

impl QuotaSnapshot {
    pub fn has_any_usage(&self) -> bool {
        self.five_hour.has_value() || self.weekly.has_value()
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredState {
    pub version: u32,
    pub latest_snapshot: Option<QuotaSnapshot>,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshUsageResult {
    pub app_state: AppState,
    pub updated: bool,
    pub message: String,
}

impl Default for StoredState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            latest_snapshot: None,
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
            .latest_snapshot
            .as_ref()
            .map(|snapshot| snapshot.status_message.clone())
            .unwrap_or_else(|| DEFAULT_STATUS_MESSAGE.to_string());

        Self {
            version: stored.version,
            latest_snapshot: stored.latest_snapshot,
            storage_status,
            storage_path,
            backup_path,
            status_message,
        }
    }
}
