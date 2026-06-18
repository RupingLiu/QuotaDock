use serde::{Deserialize, Serialize};

pub const STATE_VERSION: u32 = 1;
pub const DEFAULT_HISTORY_LIMIT: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfidenceState {
    Fresh,
    Stale,
    Partial,
    Manual,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotSource {
    PastedStatus,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub stale_after_minutes: u32,
    pub notify_below_percent: Vec<u8>,
    pub clipboard_monitoring: bool,
    #[serde(default = "default_notifications_enabled")]
    pub notifications_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManualField {
    RemainingPercent,
    ResetAt,
    CreditsBalance,
    Notes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub id: String,
    pub source: SnapshotSource,
    pub parsed_at: String,
    pub remaining_percent: Option<u8>,
    pub reset_at: Option<String>,
    pub reset_countdown_seconds: Option<i64>,
    pub credits_balance: Option<f64>,
    pub model: Option<String>,
    pub context_window: Option<String>,
    pub confidence: ConfidenceState,
    pub raw_text: String,
    pub manual_fields: Vec<ManualField>,
    pub warnings: Vec<ParseWarning>,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualUpdateInput {
    pub remaining_percent: Option<u8>,
    pub reset_at: Option<String>,
    pub credits_balance: Option<f64>,
    pub notes: Option<String>,
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
#[serde(rename_all = "kebab-case")]
pub enum CodexProbeStatus {
    Unknown,
    Healthy,
    Unavailable,
    NotAuthenticated,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexHealth {
    pub status: CodexProbeStatus,
    pub available: bool,
    pub authenticated: Option<bool>,
    pub version: Option<String>,
    pub doctor_status: Option<String>,
    pub checked_at: Option<String>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredState {
    pub version: u32,
    pub settings: Settings,
    pub latest_snapshot: Option<UsageSnapshot>,
    pub history: Vec<UsageSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub version: u32,
    pub settings: Settings,
    pub latest_snapshot: Option<UsageSnapshot>,
    pub history: Vec<UsageSnapshot>,
    pub storage_status: StorageStatus,
    pub storage_path: Option<String>,
    pub backup_path: Option<String>,
    pub codex_health: CodexHealth,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            stale_after_minutes: 60,
            notify_below_percent: vec![20, 10],
            clipboard_monitoring: false,
            notifications_enabled: true,
        }
    }
}

fn default_notifications_enabled() -> bool {
    true
}

impl Default for CodexHealth {
    fn default() -> Self {
        Self {
            status: CodexProbeStatus::Unknown,
            available: false,
            authenticated: None,
            version: None,
            doctor_status: None,
            checked_at: None,
            diagnostics: Vec::new(),
        }
    }
}

impl Default for StoredState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            settings: Settings::default(),
            latest_snapshot: None,
            history: Vec::new(),
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
        Self {
            version: stored.version,
            settings: stored.settings,
            latest_snapshot: stored.latest_snapshot,
            history: stored.history,
            storage_status,
            storage_path,
            backup_path,
            codex_health: CodexHealth::default(),
        }
    }
}
