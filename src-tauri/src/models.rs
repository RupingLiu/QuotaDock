use serde::{Deserialize, Serialize};

pub const STATE_VERSION: u32 = 5;
pub const DEFAULT_STATUS_MESSAGE: &str = "尚未获取额度。可通过托盘刷新，后台也会自动查询。";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Codex,
    DeepSeek,
    Kimi,
}

pub const PROVIDER_ORDER: [ProviderId; 3] =
    [ProviderId::Codex, ProviderId::DeepSeek, ProviderId::Kimi];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotSource {
    PastedStatus,
    CodexCli,
    CodexAppServer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ParseWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodexSnapshot {
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

impl CodexSnapshot {
    pub fn has_usage(&self) -> bool {
        self.weekly.has_usage()
    }
}

/// Compatibility name used by the existing Codex parser and refresh path.
/// Provider-backed persisted state uses `ProviderSnapshot::Codex` as its fact source.
pub type QuotaSnapshot = CodexSnapshot;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeepSeekBalance {
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: String,
    pub topped_up_balance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeepSeekSnapshot {
    pub id: String,
    pub captured_at: String,
    pub is_available: bool,
    pub balances: Vec<DeepSeekBalance>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KimiRegion {
    China,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KimiSnapshot {
    pub id: String,
    pub captured_at: String,
    pub region: KimiRegion,
    pub currency: String,
    pub available_balance: String,
    pub cash_balance: String,
    pub voucher_balance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "provider",
    content = "data",
    rename_all = "lowercase",
    deny_unknown_fields
)]
pub enum ProviderSnapshot {
    Codex(CodexSnapshot),
    DeepSeek(DeepSeekSnapshot),
    Kimi(KimiSnapshot),
}

impl ProviderSnapshot {
    pub fn provider_id(&self) -> ProviderId {
        match self {
            Self::Codex(_) => ProviderId::Codex,
            Self::DeepSeek(_) => ProviderId::DeepSeek,
            Self::Kimi(_) => ProviderId::Kimi,
        }
    }

    pub fn captured_at(&self) -> &str {
        match self {
            Self::Codex(snapshot) => &snapshot.captured_at,
            Self::DeepSeek(snapshot) => &snapshot.captured_at,
            Self::Kimi(snapshot) => &snapshot.captured_at,
        }
    }

    pub fn as_codex(&self) -> Option<&CodexSnapshot> {
        match self {
            Self::Codex(snapshot) => Some(snapshot),
            Self::DeepSeek(_) | Self::Kimi(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderHealth {
    NotConfigured,
    Idle,
    Fresh,
    Refreshing,
    Stale,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderErrorCategory {
    NotConfigured,
    Busy,
    Unauthorized,
    InsufficientBalance,
    RateLimited,
    Timeout,
    Network,
    Server,
    InvalidResponse,
    CredentialStore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderState {
    pub configured: bool,
    pub latest_snapshot: Option<ProviderSnapshot>,
    pub last_attempt_at: Option<String>,
    pub health: ProviderHealth,
    pub error_category: Option<ProviderErrorCategory>,
}

impl ProviderState {
    fn configured() -> Self {
        Self {
            configured: true,
            latest_snapshot: None,
            last_attempt_at: None,
            health: ProviderHealth::Idle,
            error_category: None,
        }
    }

    fn not_configured() -> Self {
        Self {
            configured: false,
            latest_snapshot: None,
            last_attempt_at: None,
            health: ProviderHealth::NotConfigured,
            error_category: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderStates {
    #[serde(default = "default_codex_provider_state")]
    pub codex: ProviderState,
    #[serde(default = "ProviderState::not_configured")]
    pub deepseek: ProviderState,
    #[serde(default = "ProviderState::not_configured")]
    pub kimi: ProviderState,
}

impl Default for ProviderStates {
    fn default() -> Self {
        Self {
            codex: ProviderState::configured(),
            deepseek: ProviderState::not_configured(),
            kimi: ProviderState::not_configured(),
        }
    }
}

impl ProviderStates {
    pub fn get(&self, provider_id: ProviderId) -> &ProviderState {
        match provider_id {
            ProviderId::Codex => &self.codex,
            ProviderId::DeepSeek => &self.deepseek,
            ProviderId::Kimi => &self.kimi,
        }
    }

    #[allow(dead_code)]
    pub fn get_mut(&mut self, provider_id: ProviderId) -> &mut ProviderState {
        match provider_id {
            ProviderId::Codex => &mut self.codex,
            ProviderId::DeepSeek => &mut self.deepseek,
            ProviderId::Kimi => &mut self.kimi,
        }
    }

    pub fn codex_snapshot(&self) -> Option<&CodexSnapshot> {
        self.codex
            .latest_snapshot
            .as_ref()
            .and_then(ProviderSnapshot::as_codex)
    }

    pub fn normalize(&mut self) {
        self.codex.configured = true;
        for (provider_id, state) in [
            (ProviderId::Codex, &mut self.codex),
            (ProviderId::DeepSeek, &mut self.deepseek),
            (ProviderId::Kimi, &mut self.kimi),
        ] {
            let mismatched_snapshot = state
                .latest_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.provider_id() != provider_id);
            if mismatched_snapshot {
                state.latest_snapshot = None;
                state.last_attempt_at = None;
                state.error_category = None;
                state.health = if state.configured {
                    ProviderHealth::Idle
                } else {
                    ProviderHealth::NotConfigured
                };
            }
            if state.last_attempt_at.is_none() {
                state.last_attempt_at = state
                    .latest_snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.captured_at().to_string());
            }
            if !state.configured {
                if state.error_category == Some(ProviderErrorCategory::CredentialStore) {
                    state.health = if state.latest_snapshot.is_some() {
                        ProviderHealth::Stale
                    } else {
                        ProviderHealth::Error
                    };
                } else {
                    state.health = ProviderHealth::NotConfigured;
                    state.error_category = None;
                }
            } else if state.health == ProviderHealth::NotConfigured {
                state.health = if state.latest_snapshot.is_some() {
                    ProviderHealth::Fresh
                } else {
                    ProviderHealth::Idle
                };
            }
        }
    }
}

fn default_codex_provider_state() -> ProviderState {
    ProviderState::configured()
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageHistoryPoint {
    pub captured_at: String,
    pub weekly_remaining_percent: Option<u8>,
}

impl From<&CodexSnapshot> for UsageHistoryPoint {
    fn from(snapshot: &CodexSnapshot) -> Self {
        Self {
            captured_at: snapshot.captured_at.clone(),
            weekly_remaining_percent: snapshot.weekly.remaining_percent,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppSettings {
    pub automatic_update_checks: bool,
    pub low_quota_notifications: bool,
    #[serde(default = "default_floating_provider_ids")]
    pub floating_provider_ids: Vec<ProviderId>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            automatic_update_checks: true,
            low_quota_notifications: false,
            floating_provider_ids: default_floating_provider_ids(),
        }
    }
}

impl AppSettings {
    pub fn normalize_floating_provider_ids(&mut self, providers: &ProviderStates) {
        let selected = self.floating_provider_ids.clone();
        self.floating_provider_ids = PROVIDER_ORDER
            .into_iter()
            .filter(|provider_id| {
                selected.contains(provider_id) && providers.get(*provider_id).configured
            })
            .collect();
        if self.floating_provider_ids.is_empty() {
            self.floating_provider_ids.push(ProviderId::Codex);
        }
    }
}

fn default_floating_provider_ids() -> Vec<ProviderId> {
    vec![ProviderId::Codex]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryNotice {
    pub status: StorageStatus,
    pub message: String,
    pub backup_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredState {
    pub version: u32,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub providers: ProviderStates,
    #[serde(default)]
    pub history: Vec<UsageHistoryPoint>,
    #[serde(default)]
    pub settings: AppSettings,
    #[serde(default)]
    pub recovery_notice: Option<RecoveryNotice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub version: u32,
    pub revision: u64,
    pub providers: ProviderStates,
    /// Read-only projection kept for compatibility with the legacy Codex IPC result.
    /// Persisted state has no global `latestSnapshot`; `providers.codex` is authoritative.
    pub latest_snapshot: Option<CodexSnapshot>,
    pub storage_status: StorageStatus,
    pub storage_path: Option<String>,
    pub backup_path: Option<String>,
    pub status_message: String,
    pub history: Vec<UsageHistoryPoint>,
    pub settings: AppSettings,
    pub recovery_notice: Option<RecoveryNotice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshUsageResult {
    pub app_state: AppState,
    pub updated: bool,
    pub message: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderRefreshOutcome {
    Updated,
    Unchanged,
    Skipped,
    Failed,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRefreshResult {
    pub provider_id: ProviderId,
    pub outcome: ProviderRefreshOutcome,
    pub message: String,
    pub error_category: Option<ProviderErrorCategory>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshProvidersResult {
    pub app_state: AppState,
    pub provider_results: Vec<ProviderRefreshResult>,
    pub any_updated: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    pub automatic_update_checks: Option<bool>,
    pub low_quota_notifications: Option<bool>,
    pub floating_provider_ids: Option<Vec<ProviderId>>,
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
            revision: 0,
            providers: ProviderStates::default(),
            history: Vec::new(),
            settings: AppSettings::default(),
            recovery_notice: None,
        }
    }
}

impl AppState {
    pub fn from_stored(
        mut stored: StoredState,
        storage_status: StorageStatus,
        storage_path: Option<String>,
        backup_path: Option<String>,
    ) -> Self {
        stored.providers.normalize();
        stored
            .settings
            .normalize_floating_provider_ids(&stored.providers);
        let latest_snapshot = stored.providers.codex_snapshot().cloned();
        let status_message = stored
            .recovery_notice
            .as_ref()
            .map(|notice| notice.message.clone())
            .or_else(|| {
                latest_snapshot
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
            revision: stored.revision,
            providers: stored.providers,
            latest_snapshot,
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

#[cfg(test)]
mod tests {
    use super::{
        AppSettings, DeepSeekBalance, DeepSeekSnapshot, KimiRegion, KimiSnapshot,
        ProviderErrorCategory, ProviderHealth, ProviderId, ProviderSnapshot, ProviderStates,
        SettingsPatch, StoredState, STATE_VERSION,
    };

    #[test]
    fn default_state_contains_all_providers_and_only_codex_floats() {
        let state = StoredState::default();

        assert_eq!(state.version, STATE_VERSION);
        assert!(state.providers.codex.configured);
        assert_eq!(state.providers.codex.health, ProviderHealth::Idle);
        assert!(!state.providers.deepseek.configured);
        assert_eq!(
            state.providers.deepseek.health,
            ProviderHealth::NotConfigured
        );
        assert!(!state.providers.kimi.configured);
        assert_eq!(state.settings.floating_provider_ids, [ProviderId::Codex]);
    }

    #[test]
    fn provider_snapshot_uses_a_stable_tagged_shape_and_string_amounts() {
        let snapshot = ProviderSnapshot::DeepSeek(DeepSeekSnapshot {
            id: "deepseek-1".to_string(),
            captured_at: "unix:1".to_string(),
            is_available: true,
            balances: vec![DeepSeekBalance {
                currency: "CNY".to_string(),
                total_balance: "110.00".to_string(),
                granted_balance: "10.00".to_string(),
                topped_up_balance: "100.00".to_string(),
            }],
        });

        let value = serde_json::to_value(snapshot).unwrap();

        assert_eq!(value["provider"], "deepseek");
        assert_eq!(value["data"]["balances"][0]["toppedUpBalance"], "100.00");
        assert!(value.get("apiKey").is_none());
        assert!(value.get("token").is_none());
        assert!(value.get("authorization").is_none());
    }

    #[test]
    fn kimi_snapshot_preserves_negative_and_trailing_decimal_digits() {
        let snapshot = ProviderSnapshot::Kimi(KimiSnapshot {
            id: "kimi-1".to_string(),
            captured_at: "unix:2".to_string(),
            region: KimiRegion::China,
            currency: "CNY".to_string(),
            available_balance: "49.5900".to_string(),
            cash_balance: "-0.4100".to_string(),
            voucher_balance: "50.0000".to_string(),
        });

        let json = serde_json::to_string(&snapshot).unwrap();

        assert!(json.contains(r#""provider":"kimi""#));
        assert!(json.contains(r#""availableBalance":"49.5900""#));
        assert!(json.contains(r#""cashBalance":"-0.4100""#));
    }

    #[test]
    fn persisted_state_schema_has_no_sensitive_credential_fields() {
        let json = serde_json::to_string(&StoredState::default()).unwrap();

        assert!(!json.contains("apiKey"));
        assert!(!json.contains("token"));
        assert!(!json.contains("Authorization"));
        assert!(!json.contains("password"));
    }

    #[test]
    fn normalization_resets_metadata_when_a_snapshot_tag_is_mismatched() {
        let mut providers = ProviderStates::default();
        providers.codex.configured = false;
        providers.codex.latest_snapshot = Some(ProviderSnapshot::DeepSeek(DeepSeekSnapshot {
            id: "wrong-provider".to_string(),
            captured_at: "unix:10".to_string(),
            is_available: true,
            balances: Vec::new(),
        }));
        providers.codex.last_attempt_at = Some("unix:11".to_string());
        providers.codex.health = ProviderHealth::Fresh;
        providers.codex.error_category = Some(ProviderErrorCategory::Unauthorized);

        providers.normalize();

        assert!(providers.codex.configured);
        assert!(providers.codex.latest_snapshot.is_none());
        assert!(providers.codex.last_attempt_at.is_none());
        assert_eq!(providers.codex.health, ProviderHealth::Idle);
        assert!(providers.codex.error_category.is_none());
    }

    #[test]
    fn floating_provider_ids_are_deduplicated_canonically_and_require_configuration() {
        let mut providers = ProviderStates::default();
        providers.deepseek.configured = true;
        providers.deepseek.health = ProviderHealth::Idle;
        let mut settings = AppSettings {
            floating_provider_ids: vec![
                ProviderId::DeepSeek,
                ProviderId::Codex,
                ProviderId::DeepSeek,
                ProviderId::Kimi,
            ],
            ..AppSettings::default()
        };

        settings.normalize_floating_provider_ids(&providers);

        assert_eq!(
            settings.floating_provider_ids,
            [ProviderId::Codex, ProviderId::DeepSeek]
        );
    }

    #[test]
    fn floating_provider_ids_fall_back_to_codex() {
        let providers = ProviderStates::default();
        let mut settings = AppSettings {
            floating_provider_ids: vec![ProviderId::Kimi],
            ..AppSettings::default()
        };

        settings.normalize_floating_provider_ids(&providers);

        assert_eq!(settings.floating_provider_ids, [ProviderId::Codex]);
    }

    #[test]
    fn settings_patch_rejects_unknown_provider_ids() {
        let result = serde_json::from_str::<SettingsPatch>(
            r#"{"floatingProviderIds":["codex","unknown-provider"]}"#,
        );

        assert!(result.is_err());
    }
}
