use crate::app_server;
use crate::credentials::{
    self, CredentialAvailability, CredentialStatus, CredentialStore, CredentialStoreErrorKind,
    ProviderCredentialStatus, WindowsCredentialStore,
};
use crate::http_client::HttpClient;
use crate::models::{
    AppDiagnostics, AppState, KimiRegion, ProviderErrorCategory, ProviderId,
    ProviderRefreshOutcome, ProviderRefreshResult, ProviderSnapshot, QuotaSnapshot,
    RefreshProvidersResult, RefreshUsageResult, SettingsPatch, SnapshotSource, PROVIDER_ORDER,
};
use crate::providers::{self, ProviderError};
use crate::status_parser::{parse_status_text_with_source, ParseClock};
use crate::usage_store::{
    ProviderRefreshMutation, ProviderRefreshMutationKind, StoreError, UsageStore,
};
use crate::{startup, version};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(feature = "desktop")]
use tauri::plugin::PermissionState;
use tauri::{App, AppHandle, Emitter, Manager};
#[cfg(feature = "desktop")]
use tauri_plugin_notification::NotificationExt;

pub const USAGE_STATE_CHANGED_EVENT: &str = "usage-state-changed";
pub const PROVIDER_STATE_CHANGED_EVENT: &str = "provider-state-changed";
const AUTO_FIRST_REFRESH_DELAY: Duration = Duration::from_secs(10);
const AUTO_BASE_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const AUTO_LOW_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const AUTO_POST_RESET_REFRESH_DELAY: Duration = Duration::from_secs(30);
const AUTO_RESET_WATCH_WINDOW: Duration = Duration::from_secs(10 * 60);
const AUTO_MAX_FAILURE_BACKOFF: Duration = Duration::from_secs(30 * 60);
const AUTO_BUSY_RETRY_INTERVAL: Duration = Duration::from_secs(30);
const AUTO_REFRESH_STOP_POLL_INTERVAL: Duration = Duration::from_secs(1);
const LOW_USAGE_THRESHOLD_PERCENT: u8 = 20;
const STATUS_OUTPUT_SETTLE_DELAY: Duration = Duration::from_millis(900);

#[derive(Clone)]
pub struct RefreshCoordinator {
    running: Arc<[AtomicBool; 3]>,
    commit_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Default)]
pub struct AutoRefreshControl {
    stopping: Arc<AtomicBool>,
}

type AutoRefreshOutcome = Result<RefreshProvidersResult, String>;

struct AutoRefreshCompletion {
    sender: Option<Sender<(ProviderId, AutoRefreshOutcome)>>,
    provider_id: ProviderId,
}

impl AutoRefreshCompletion {
    fn new(sender: Sender<(ProviderId, AutoRefreshOutcome)>, provider_id: ProviderId) -> Self {
        Self {
            sender: Some(sender),
            provider_id,
        }
    }

    fn complete(mut self, outcome: AutoRefreshOutcome) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send((self.provider_id, outcome));
        }
    }
}

impl Drop for AutoRefreshCompletion {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send((
                self.provider_id,
                Err("自动刷新任务异常结束，稍后将重试。".to_string()),
            ));
        }
    }
}

struct RefreshPermit {
    running: Arc<[AtomicBool; 3]>,
    index: usize,
}

impl Default for RefreshCoordinator {
    fn default() -> Self {
        Self {
            running: Arc::new([
                AtomicBool::new(false),
                AtomicBool::new(false),
                AtomicBool::new(false),
            ]),
            commit_lock: Arc::new(Mutex::new(())),
        }
    }
}

impl RefreshCoordinator {
    fn try_begin(&self, provider_id: ProviderId) -> Option<RefreshPermit> {
        let index = provider_index(provider_id);
        let slot = &self.running[index];
        slot.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| RefreshPermit {
                running: Arc::clone(&self.running),
                index,
            })
    }
}

impl Drop for RefreshPermit {
    fn drop(&mut self) {
        self.running[self.index].store(false, Ordering::Release);
    }
}

const fn provider_index(provider_id: ProviderId) -> usize {
    match provider_id {
        ProviderId::Codex => 0,
        ProviderId::DeepSeek => 1,
        ProviderId::Kimi => 2,
    }
}

#[derive(Clone, Copy)]
enum RefreshOrigin {
    Command,
    Tray,
    Automatic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderFailure {
    category: ProviderErrorCategory,
    message: String,
}

impl ProviderFailure {
    fn new(category: ProviderErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }

    fn from_provider(error: ProviderError) -> Self {
        Self::new(error.category(), error.message())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProviderFetchOutcome {
    Updated(ProviderSnapshot),
    NotConfigured,
    Failed {
        failure: ProviderFailure,
        configured: Option<bool>,
    },
}

trait ProviderFetcher: Sync {
    fn fetch(&self, provider_id: ProviderId) -> ProviderFetchOutcome;
}

#[derive(Clone)]
struct SystemProviderFetcher {
    http_client: Result<HttpClient, ProviderFailure>,
}

impl SystemProviderFetcher {
    fn new() -> Self {
        Self {
            http_client: HttpClient::new()
                .map_err(ProviderError::from)
                .map_err(ProviderFailure::from_provider),
        }
    }

    fn network_client(&self) -> Result<&HttpClient, ProviderFetchOutcome> {
        self.http_client
            .as_ref()
            .map_err(|failure| ProviderFetchOutcome::Failed {
                failure: failure.clone(),
                configured: Some(true),
            })
    }

    fn credential(
        &self,
        provider_id: ProviderId,
        region: Option<KimiRegion>,
    ) -> Result<String, ProviderFetchOutcome> {
        credentials::load_provider_credential(&WindowsCredentialStore, provider_id, region).map_err(
            |error| match error.kind() {
                CredentialStoreErrorKind::NotFound => ProviderFetchOutcome::NotConfigured,
                CredentialStoreErrorKind::Unavailable
                | CredentialStoreErrorKind::OperationFailed => ProviderFetchOutcome::Failed {
                    failure: ProviderFailure::new(
                        ProviderErrorCategory::CredentialStore,
                        error.to_string(),
                    ),
                    configured: None,
                },
            },
        )
    }
}

impl ProviderFetcher for SystemProviderFetcher {
    fn fetch(&self, provider_id: ProviderId) -> ProviderFetchOutcome {
        match provider_id {
            ProviderId::Codex => fetch_usage_from_codex_cli()
                .map(ProviderSnapshot::Codex)
                .map(ProviderFetchOutcome::Updated)
                .unwrap_or_else(|message| ProviderFetchOutcome::Failed {
                    failure: ProviderFailure::new(ProviderErrorCategory::InvalidResponse, message),
                    configured: Some(true),
                }),
            ProviderId::DeepSeek => {
                let credential = match self.credential(ProviderId::DeepSeek, None) {
                    Ok(credential) => credential,
                    Err(outcome) => return outcome,
                };
                let client = match self.network_client() {
                    Ok(client) => client,
                    Err(outcome) => return outcome,
                };
                providers::deepseek::fetch(client, &credential)
                    .map(ProviderFetchOutcome::Updated)
                    .unwrap_or_else(|error| ProviderFetchOutcome::Failed {
                        failure: ProviderFailure::from_provider(error),
                        configured: Some(true),
                    })
            }
            ProviderId::Kimi => {
                let credential = match self.credential(ProviderId::Kimi, Some(KimiRegion::China)) {
                    Ok(credential) => credential,
                    Err(outcome) => return outcome,
                };
                let client = match self.network_client() {
                    Ok(client) => client,
                    Err(outcome) => return outcome,
                };
                providers::kimi::fetch(client, &credential)
                    .map(ProviderFetchOutcome::Updated)
                    .unwrap_or_else(|error| ProviderFetchOutcome::Failed {
                        failure: ProviderFailure::from_provider(error),
                        configured: Some(true),
                    })
            }
        }
    }
}

pub fn install_refresh_coordinator(app: &App) {
    app.manage(RefreshCoordinator::default());
    app.manage(AutoRefreshControl::default());
}

pub fn start_auto_refresh(app: AppHandle) {
    let Some(control) = app
        .try_state::<AutoRefreshControl>()
        .map(|control| control.inner().clone())
    else {
        eprintln!("auto refresh control is not initialized");
        return;
    };
    control.stopping.store(false, Ordering::Release);
    if let Err(error) = thread::Builder::new()
        .name("quotadock-auto-refresh".to_string())
        .spawn(move || {
            let mut schedules = PROVIDER_ORDER.map(|_| ProviderAutoSchedule {
                due_at: Instant::now() + AUTO_FIRST_REFRESH_DELAY,
                consecutive_failures: 0,
            });
            let mut in_flight = [false; 3];
            let (completed_tx, completed_rx) = std::sync::mpsc::channel();
            while !control.stopping.load(Ordering::Acquire) {
                let now = Instant::now();
                for provider_id in PROVIDER_ORDER {
                    let index = provider_index(provider_id);
                    if in_flight[index] || schedules[index].due_at > now {
                        continue;
                    }
                    in_flight[index] = true;
                    let refresh_app = app.clone();
                    let completion = AutoRefreshCompletion::new(completed_tx.clone(), provider_id);
                    tauri::async_runtime::spawn(async move {
                        let outcome = refresh_providers_internal(
                            refresh_app,
                            RefreshOrigin::Automatic,
                            vec![provider_id],
                        )
                        .await;
                        completion.complete(outcome);
                    });
                }

                let timeout = auto_refresh_wait_timeout(
                    PROVIDER_ORDER
                        .into_iter()
                        .filter(|provider_id| !in_flight[provider_index(*provider_id)])
                        .map(|provider_id| {
                            schedules[provider_index(provider_id)]
                                .due_at
                                .saturating_duration_since(Instant::now())
                        })
                        .min()
                        .unwrap_or(AUTO_BASE_REFRESH_INTERVAL),
                );
                match completed_rx.recv_timeout(timeout) {
                    Ok((provider_id, outcome)) => {
                        let index = provider_index(provider_id);
                        in_flight[index] = false;
                        let previous_failures = schedules[index].consecutive_failures;
                        let (delay, consecutive_failures) = match &outcome {
                            Ok(result) => result
                                .provider_results
                                .iter()
                                .find(|result| result.provider_id == provider_id)
                                .map(|provider_result| {
                                    next_provider_auto_schedule(
                                        provider_id,
                                        provider_result,
                                        &result.app_state,
                                        previous_failures,
                                    )
                                })
                                .unwrap_or((AUTO_BASE_REFRESH_INTERVAL, previous_failures)),
                            Err(_) => {
                                let failures = previous_failures.saturating_add(1);
                                (failure_backoff_interval(failures), failures)
                            }
                        };
                        schedules[index] = ProviderAutoSchedule {
                            due_at: Instant::now() + delay,
                            consecutive_failures,
                        };
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        eprintln!("auto refresh completion channel disconnected");
                        break;
                    }
                }
            }
        })
    {
        eprintln!("start auto refresh thread failed: {error}");
    }
}

pub fn stop_auto_refresh(app: &AppHandle) {
    if let Some(control) = app.try_state::<AutoRefreshControl>() {
        control.stopping.store(true, Ordering::Release);
    }
}

fn auto_refresh_wait_timeout(next_due: Duration) -> Duration {
    next_due.min(AUTO_REFRESH_STOP_POLL_INTERVAL)
}

#[derive(Debug, Clone, Copy)]
struct ProviderAutoSchedule {
    due_at: Instant,
    consecutive_failures: u32,
}

fn next_provider_auto_schedule(
    provider_id: ProviderId,
    result: &ProviderRefreshResult,
    state: &AppState,
    previous_failures: u32,
) -> (Duration, u32) {
    match result.outcome {
        ProviderRefreshOutcome::Updated | ProviderRefreshOutcome::Unchanged => {
            let delay = if provider_id == ProviderId::Codex {
                adaptive_refresh_interval(state)
            } else {
                AUTO_BASE_REFRESH_INTERVAL
            };
            (delay, 0)
        }
        ProviderRefreshOutcome::Failed => {
            let failures = previous_failures.saturating_add(1);
            (failure_backoff_interval(failures), failures)
        }
        ProviderRefreshOutcome::Skipped
            if result.error_category == Some(ProviderErrorCategory::Busy) =>
        {
            (AUTO_BUSY_RETRY_INTERVAL, previous_failures)
        }
        ProviderRefreshOutcome::Skipped => (AUTO_BASE_REFRESH_INTERVAL, 0),
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AutoRefreshSchedule {
    delay: Duration,
    consecutive_failures: u32,
}

#[cfg(test)]
fn next_auto_refresh_schedule(
    outcome: &Result<RefreshUsageResult, String>,
    previous_failures: u32,
) -> AutoRefreshSchedule {
    match outcome {
        Ok(result) if result.updated => AutoRefreshSchedule {
            delay: adaptive_refresh_interval(&result.app_state),
            consecutive_failures: 0,
        },
        Ok(_) | Err(_) => {
            let consecutive_failures = previous_failures.saturating_add(1);
            AutoRefreshSchedule {
                delay: failure_backoff_interval(consecutive_failures),
                consecutive_failures,
            }
        }
    }
}

fn adaptive_refresh_interval(state: &AppState) -> Duration {
    let Some(snapshot) = &state.latest_snapshot else {
        return AUTO_BASE_REFRESH_INTERVAL;
    };

    let mut delay = AUTO_BASE_REFRESH_INTERVAL;
    if is_low_usage_snapshot(snapshot) {
        delay = delay.min(AUTO_LOW_USAGE_REFRESH_INTERVAL);
    }
    if let Some(reset_delay) = imminent_reset_refresh_interval(snapshot) {
        delay = delay.min(reset_delay);
    }
    delay
}

fn is_low_usage_snapshot(snapshot: &QuotaSnapshot) -> bool {
    snapshot
        .weekly
        .remaining_percent
        .is_some_and(|percent| percent <= LOW_USAGE_THRESHOLD_PERCENT)
}

fn imminent_reset_refresh_interval(snapshot: &QuotaSnapshot) -> Option<Duration> {
    let now_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    imminent_reset_refresh_interval_at(snapshot, now_unix_seconds)
}

fn imminent_reset_refresh_interval_at(
    snapshot: &QuotaSnapshot,
    now_unix_seconds: i64,
) -> Option<Duration> {
    let countdown = snapshot.weekly.reset_countdown_seconds;
    let absolute = snapshot
        .weekly
        .reset_at
        .as_deref()
        .and_then(unix_reset_seconds)
        .map(|reset| reset - now_unix_seconds);
    [countdown, absolute]
        .into_iter()
        .flatten()
        .filter(|seconds| {
            let watch = AUTO_RESET_WATCH_WINDOW.as_secs() as i64;
            *seconds >= -watch && *seconds <= watch
        })
        .map(|seconds| Duration::from_secs(seconds.max(0) as u64) + AUTO_POST_RESET_REFRESH_DELAY)
        .min()
}

fn unix_reset_seconds(value: &str) -> Option<i64> {
    value.strip_prefix("unix:")?.trim().parse().ok()
}

fn failure_backoff_interval(consecutive_failures: u32) -> Duration {
    let multiplier = 1_u64 << consecutive_failures.saturating_sub(1).min(3);
    let seconds = AUTO_BASE_REFRESH_INTERVAL
        .as_secs()
        .saturating_mul(multiplier)
        .min(AUTO_MAX_FAILURE_BACKOFF.as_secs());
    Duration::from_secs(seconds)
}

#[tauri::command]
pub fn get_app_state(app: AppHandle) -> Result<AppState, String> {
    let state = load_app_state(&app)?;
    sync_tray(&app, &state);
    Ok(state)
}

#[tauri::command]
pub async fn refresh_usage(app: AppHandle) -> Result<RefreshUsageResult, String> {
    let result =
        refresh_providers_internal(app.clone(), RefreshOrigin::Command, vec![ProviderId::Codex])
            .await?;
    Ok(legacy_codex_result(result))
}

fn legacy_codex_result(result: RefreshProvidersResult) -> RefreshUsageResult {
    let codex = result
        .provider_results
        .iter()
        .find(|item| item.provider_id == ProviderId::Codex);
    RefreshUsageResult {
        app_state: result.app_state,
        updated: codex.is_some_and(|item| item.outcome == ProviderRefreshOutcome::Updated),
        message: codex
            .map(|item| item.message.clone())
            .unwrap_or_else(|| "Codex 查询未执行。".to_string()),
    }
}

#[tauri::command]
pub async fn refresh_providers(app: AppHandle) -> Result<RefreshProvidersResult, String> {
    refresh_providers_internal(app, RefreshOrigin::Command, PROVIDER_ORDER.to_vec()).await
}

#[tauri::command]
pub async fn refresh_provider(
    app: AppHandle,
    provider: ProviderId,
) -> Result<RefreshProvidersResult, String> {
    refresh_providers_internal(app, RefreshOrigin::Command, vec![provider]).await
}

#[tauri::command]
pub fn show_dashboard_context_menu(app: AppHandle, x: f64, y: f64) -> Result<(), String> {
    #[cfg(feature = "desktop")]
    {
        crate::tray::show_dashboard_context_menu(&app, x, y)
    }

    #[cfg(not(feature = "desktop"))]
    {
        let _ = (app, x, y);
        Err("当前构建不支持桌面菜单。".to_string())
    }
}

#[tauri::command]
pub fn set_provider_credential(
    app: AppHandle,
    provider: ProviderId,
    region: Option<KimiRegion>,
    secret: String,
) -> Result<CredentialStatus, String> {
    let store = store_for_app(&app)?;
    let coordinator = app
        .try_state::<RefreshCoordinator>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "刷新协调器尚未初始化。".to_string())?;
    let _permit = coordinator
        .try_begin(provider)
        .ok_or_else(|| "该供应商正在刷新，请稍后再修改凭据。".to_string())?;
    let _commit = coordinator
        .commit_lock
        .lock()
        .map_err(|_| "凭据状态写入锁不可用。".to_string())?;
    let (status, app_state) = set_provider_credential_and_sync(
        &WindowsCredentialStore,
        &store,
        provider,
        region,
        &secret,
    )?;
    drop(_commit);
    emit_configuration_state(&app, app_state, provider, "连接凭据已保存。");
    Ok(status)
}

#[tauri::command]
pub fn delete_provider_credential(
    app: AppHandle,
    provider: ProviderId,
    region: Option<KimiRegion>,
) -> Result<CredentialStatus, String> {
    let store = store_for_app(&app)?;
    let coordinator = app
        .try_state::<RefreshCoordinator>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "刷新协调器尚未初始化。".to_string())?;
    let _permit = coordinator
        .try_begin(provider)
        .ok_or_else(|| "该供应商正在刷新，请稍后再删除凭据。".to_string())?;
    let _commit = coordinator
        .commit_lock
        .lock()
        .map_err(|_| "凭据状态写入锁不可用。".to_string())?;
    let (status, app_state) =
        delete_provider_credential_and_sync(&WindowsCredentialStore, &store, provider, region)?;
    drop(_commit);
    emit_configuration_state(&app, app_state, provider, "连接凭据已删除。");
    Ok(status)
}

fn set_provider_credential_and_sync<S: CredentialStore>(
    credential_store: &S,
    usage_store: &UsageStore,
    provider_id: ProviderId,
    region: Option<KimiRegion>,
    secret: &str,
) -> Result<(CredentialStatus, AppState), String> {
    let previous =
        match credentials::load_provider_credential(credential_store, provider_id, region) {
            Ok(secret) => Some(secret),
            Err(error) if error.kind() == CredentialStoreErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
    let status = credentials::set_provider_credential_with_store(
        credential_store,
        provider_id,
        region,
        secret,
    )?;
    let app_state = match usage_store.set_provider_configured(provider_id, true) {
        Ok(outcome) => outcome.into_app_state(),
        Err(_) => {
            let rolled_back = match previous {
                Some(previous) => credentials::set_provider_credential_with_store(
                    credential_store,
                    provider_id,
                    region,
                    &previous,
                )
                .map(|_| ()),
                None => credentials::delete_provider_credential_with_store(
                    credential_store,
                    provider_id,
                    region,
                )
                .map(|_| ()),
            };
            return Err(if rolled_back.is_ok() {
                "状态保存失败，凭据更改已回滚。".to_string()
            } else {
                "状态保存失败，凭据回滚也失败，请检查连接设置。".to_string()
            });
        }
    };
    Ok((status, app_state))
}

fn delete_provider_credential_and_sync<S: CredentialStore>(
    credential_store: &S,
    usage_store: &UsageStore,
    provider_id: ProviderId,
    region: Option<KimiRegion>,
) -> Result<(CredentialStatus, AppState), String> {
    let previous = credentials::load_provider_credential(credential_store, provider_id, region)
        .map_err(|error| error.to_string())?;
    let status =
        credentials::delete_provider_credential_with_store(credential_store, provider_id, region)?;
    let app_state = match usage_store.set_provider_configured(provider_id, false) {
        Ok(outcome) => outcome.into_app_state(),
        Err(_) => {
            let rolled_back = credentials::set_provider_credential_with_store(
                credential_store,
                provider_id,
                region,
                &previous,
            );
            return Err(if rolled_back.is_ok() {
                "状态保存失败，凭据删除已回滚。".to_string()
            } else {
                "状态保存失败，凭据回滚也失败，请检查连接设置。".to_string()
            });
        }
    };
    Ok((status, app_state))
}

#[tauri::command]
pub fn get_provider_credential_status(app: AppHandle) -> Vec<ProviderCredentialStatus> {
    read_and_sync_credential_facts(&app)
        .map(|(statuses, _)| statuses)
        .unwrap_or_else(|_| {
            credentials::provider_credential_status_with_store(&WindowsCredentialStore)
        })
}

#[tauri::command]
pub fn update_settings(app: AppHandle, patch: SettingsPatch) -> Result<AppState, String> {
    #[cfg(feature = "desktop")]
    if patch.low_quota_notifications == Some(true) {
        let notifications = app.notification();
        if matches!(
            notifications
                .permission_state()
                .map_err(|error| format!("读取通知权限失败：{error}"))?,
            PermissionState::Prompt | PermissionState::PromptWithRationale
        ) {
            notifications
                .request_permission()
                .map_err(|error| format!("请求通知权限失败：{error}"))?;
        }
        if notifications
            .permission_state()
            .map_err(|error| format!("读取通知权限失败：{error}"))?
            != PermissionState::Granted
        {
            return Err("系统未授予通知权限，低额度通知没有开启。".to_string());
        }
    }

    let coordinator = app
        .try_state::<RefreshCoordinator>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "刷新协调器尚未初始化。".to_string())?;
    let _commit = coordinator
        .commit_lock
        .lock()
        .map_err(|_| "设置状态写入锁不可用。".to_string())?;
    let app_state = store_for_app(&app)?
        .update_settings(patch)
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)?;
    drop(_commit);
    sync_tray(&app, &app_state);
    emit_usage_state(
        &app,
        &RefreshUsageResult {
            app_state: app_state.clone(),
            updated: true,
            message: "设置已保存。".to_string(),
        },
    );
    Ok(app_state)
}

#[tauri::command]
pub fn acknowledge_recovery(app: AppHandle) -> Result<AppState, String> {
    let coordinator = app
        .try_state::<RefreshCoordinator>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "刷新协调器尚未初始化。".to_string())?;
    let _commit = coordinator
        .commit_lock
        .lock()
        .map_err(|_| "恢复状态写入锁不可用。".to_string())?;
    let app_state = store_for_app(&app)?
        .acknowledge_recovery()
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)?;
    drop(_commit);
    emit_usage_state(
        &app,
        &RefreshUsageResult {
            app_state: app_state.clone(),
            updated: true,
            message: "存储恢复提示已确认。".to_string(),
        },
    );
    Ok(app_state)
}

#[tauri::command]
pub fn get_diagnostics(app: AppHandle) -> Result<AppDiagnostics, String> {
    let state = load_app_state(&app)?;
    let codex_path = find_codex_binary();
    let codex_version = codex_path.as_ref().and_then(|_| {
        run_codex(&["--version"], Duration::from_secs(3))
            .ok()
            .filter(|output| output.success)
            .map(|output| output.stdout.trim().to_string())
            .filter(|output| !output.is_empty())
    });
    Ok(AppDiagnostics {
        app_version: version::APP_VERSION.to_string(),
        codex_path: codex_path.map(|path| path.display().to_string()),
        codex_version,
        latest_source: state
            .latest_snapshot
            .as_ref()
            .map(|snapshot| snapshot.source.clone()),
        latest_success_at: state
            .latest_snapshot
            .as_ref()
            .map(|snapshot| snapshot.captured_at.clone()),
        storage_path: state.storage_path.clone(),
        storage_status: state.storage_status,
        startup_enabled: startup::is_enabled().unwrap_or(false),
        signed_updates_enabled: true,
    })
}

#[tauri::command]
pub fn set_startup_enabled(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let enabled = startup::set_enabled(enabled)?;
    #[cfg(feature = "desktop")]
    crate::tray::refresh_menu(&app);
    Ok(enabled)
}

#[tauri::command]
pub fn show_details(app: AppHandle) -> Result<(), String> {
    #[cfg(feature = "desktop")]
    {
        crate::details::show(&app)
    }
    #[cfg(not(feature = "desktop"))]
    {
        let _ = app;
        Err("当前构建不支持详情窗口。".to_string())
    }
}

#[tauri::command]
pub fn hide_details(app: AppHandle) -> Result<(), String> {
    #[cfg(feature = "desktop")]
    {
        crate::details::hide(&app)
    }
    #[cfg(not(feature = "desktop"))]
    {
        let _ = app;
        Err("当前构建不支持详情窗口。".to_string())
    }
}

#[tauri::command]
pub fn open_official_usage() -> Result<(), String> {
    #[cfg(feature = "desktop")]
    {
        crate::details::open_official_usage()
    }
    #[cfg(not(feature = "desktop"))]
    {
        Err("当前构建不支持打开外部页面。".to_string())
    }
}

pub fn refresh_usage_from_tray(app: AppHandle) {
    #[cfg(feature = "desktop")]
    crate::tray::set_menu_status(&app, "正在刷新全部...");

    tauri::async_runtime::spawn(async move {
        let message = match refresh_providers_internal(
            app.clone(),
            RefreshOrigin::Tray,
            PROVIDER_ORDER.to_vec(),
        )
        .await
        {
            Ok(result) if result.any_updated => "额度已更新".to_string(),
            Ok(result) => result.message,
            Err(error) => error,
        };

        #[cfg(feature = "desktop")]
        crate::tray::set_menu_status_temporarily(&app, message);
    });
}

async fn refresh_providers_internal(
    app: AppHandle,
    _origin: RefreshOrigin,
    provider_ids: Vec<ProviderId>,
) -> Result<RefreshProvidersResult, String> {
    let coordinator = app
        .try_state::<RefreshCoordinator>()
        .map(|coordinator| coordinator.inner().clone())
        .ok_or_else(|| "刷新协调器尚未初始化。".to_string())?;
    let worker_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let store = store_for_app(&worker_app)?;
        let previous = store.load().map_err(to_command_error)?;
        let previous_codex = previous.state.providers.codex_snapshot().cloned();
        let notifications_enabled = previous.state.settings.low_quota_notifications;
        let fetcher = SystemProviderFetcher::new();
        let result = refresh_providers_blocking_with_callback(
            &store,
            &coordinator,
            &fetcher,
            &provider_ids,
            providers::captured_at_now(),
            |event| {
                sync_tray(&worker_app, &event.app_state);
                emit_provider_state(&worker_app, event);
                if let Some(codex) = event
                    .provider_results
                    .iter()
                    .find(|item| item.provider_id == ProviderId::Codex)
                {
                    emit_usage_state(
                        &worker_app,
                        &RefreshUsageResult {
                            app_state: event.app_state.clone(),
                            updated: codex.outcome == ProviderRefreshOutcome::Updated,
                            message: codex.message.clone(),
                        },
                    );
                }
            },
        )?;
        if notifications_enabled {
            if let Some(message) = result
                .app_state
                .providers
                .codex_snapshot()
                .and_then(|current| low_quota_notification(previous_codex.as_ref(), current))
            {
                notify_low_quota(&worker_app, &message);
            }
        }
        Ok::<RefreshProvidersResult, String>(result)
    })
    .await
    .map_err(|error| format!("后台查询任务失败：{error}"))??;
    sync_tray(&app, &result.app_state);
    Ok(result)
}

#[cfg(test)]
fn refresh_providers_blocking<F: ProviderFetcher>(
    store: &UsageStore,
    coordinator: &RefreshCoordinator,
    fetcher: &F,
    provider_ids: &[ProviderId],
    attempted_at: String,
) -> Result<RefreshProvidersResult, String> {
    refresh_providers_blocking_with_callback(
        store,
        coordinator,
        fetcher,
        provider_ids,
        attempted_at,
        |_| {},
    )
}

fn refresh_providers_blocking_with_callback<F, C>(
    store: &UsageStore,
    coordinator: &RefreshCoordinator,
    fetcher: &F,
    provider_ids: &[ProviderId],
    attempted_at: String,
    mut on_commit: C,
) -> Result<RefreshProvidersResult, String>
where
    F: ProviderFetcher,
    C: FnMut(&RefreshProvidersResult),
{
    let selected = PROVIDER_ORDER
        .into_iter()
        .filter(|provider_id| provider_ids.contains(provider_id))
        .collect::<Vec<_>>();
    let mut busy_results = Vec::new();
    let mut permits = Vec::new();
    for provider_id in selected {
        match coordinator.try_begin(provider_id) {
            Some(permit) => permits.push((provider_id, permit)),
            None => busy_results.push(ProviderRefreshResult {
                provider_id,
                outcome: ProviderRefreshOutcome::Skipped,
                message: "该供应商已有查询正在进行。".to_string(),
                error_category: Some(ProviderErrorCategory::Busy),
            }),
        }
    }

    let mut provider_results = busy_results;
    let mut app_state = store
        .load()
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)?;
    for busy in provider_results.clone() {
        let event = single_provider_event(app_state.clone(), busy.clone());
        on_commit(&event);
    }

    let completion_count = permits.len();
    thread::scope(|scope| -> Result<(), String> {
        let (completed_tx, completed_rx) = std::sync::mpsc::channel();
        for (provider_id, permit) in permits {
            let completed_tx = completed_tx.clone();
            scope.spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    fetcher.fetch(provider_id)
                }))
                .unwrap_or_else(|_| ProviderFetchOutcome::Failed {
                    failure: ProviderFailure::new(
                        ProviderErrorCategory::InvalidResponse,
                        "供应商查询任务异常结束。",
                    ),
                    configured: None,
                });
                let _ = completed_tx.send((provider_id, outcome, permit));
            });
        }
        drop(completed_tx);

        for _ in 0..completion_count {
            let (provider_id, outcome, permit) = completed_rx
                .recv()
                .map_err(|_| "供应商查询结果通道意外关闭。".to_string())?;
            let (mutation, provider_result) =
                provider_completion(provider_id, outcome, attempted_at.clone());
            let _commit = coordinator
                .commit_lock
                .lock()
                .map_err(|_| "刷新状态写入锁不可用。".to_string())?;
            app_state = store
                .apply_provider_refreshes(vec![mutation])
                .map(|outcome| outcome.into_app_state())
                .map_err(to_command_error)?;
            drop(_commit);
            drop(permit);
            let event = single_provider_event(app_state.clone(), provider_result.clone());
            on_commit(&event);
            provider_results.push(provider_result);
        }
        Ok(())
    })?;

    provider_results.sort_by_key(|result| provider_index(result.provider_id));
    let any_updated = provider_results
        .iter()
        .any(|result| result.outcome == ProviderRefreshOutcome::Updated);
    let failed = provider_results
        .iter()
        .filter(|result| result.outcome == ProviderRefreshOutcome::Failed)
        .count();
    let not_configured = provider_results
        .iter()
        .all(|result| result.error_category == Some(ProviderErrorCategory::NotConfigured));
    let busy = provider_results
        .iter()
        .any(|result| result.error_category == Some(ProviderErrorCategory::Busy));
    let message = if provider_results.len() == 1 {
        provider_results[0].message.clone()
    } else if any_updated && failed > 0 && busy {
        "部分供应商已更新，失败项保留上次成功数据，另有查询正在进行中。".to_string()
    } else if any_updated && failed > 0 {
        "部分供应商已更新，失败项保留上次成功数据。".to_string()
    } else if any_updated && busy {
        "部分供应商已更新，另有查询正在进行中。".to_string()
    } else if any_updated {
        "所有已配置供应商均已更新。".to_string()
    } else if failed > 0 {
        "供应商查询失败，当前保留上次成功数据。".to_string()
    } else if not_configured {
        "供应商尚未配置。".to_string()
    } else if busy {
        "部分供应商已有查询正在进行。".to_string()
    } else {
        "没有需要更新的供应商。".to_string()
    };

    Ok(RefreshProvidersResult {
        app_state,
        provider_results,
        any_updated,
        message,
    })
}

fn provider_completion(
    provider_id: ProviderId,
    outcome: ProviderFetchOutcome,
    attempted_at: String,
) -> (ProviderRefreshMutation, ProviderRefreshResult) {
    match outcome {
        ProviderFetchOutcome::Updated(snapshot) => (
            ProviderRefreshMutation {
                provider_id,
                attempted_at,
                kind: ProviderRefreshMutationKind::Updated(snapshot),
            },
            ProviderRefreshResult {
                provider_id,
                outcome: ProviderRefreshOutcome::Updated,
                message: format!("{} 已更新。", provider_label(provider_id)),
                error_category: None,
            },
        ),
        ProviderFetchOutcome::NotConfigured => (
            ProviderRefreshMutation {
                provider_id,
                attempted_at,
                kind: ProviderRefreshMutationKind::NotConfigured,
            },
            ProviderRefreshResult {
                provider_id,
                outcome: ProviderRefreshOutcome::Skipped,
                message: format!("{} 尚未配置。", provider_label(provider_id)),
                error_category: Some(ProviderErrorCategory::NotConfigured),
            },
        ),
        ProviderFetchOutcome::Failed {
            failure,
            configured,
        } => (
            ProviderRefreshMutation {
                provider_id,
                attempted_at,
                kind: ProviderRefreshMutationKind::Failed {
                    category: failure.category,
                    configured,
                },
            },
            ProviderRefreshResult {
                provider_id,
                outcome: ProviderRefreshOutcome::Failed,
                message: failure.message,
                error_category: Some(failure.category),
            },
        ),
    }
}

fn single_provider_event(
    app_state: AppState,
    provider_result: ProviderRefreshResult,
) -> RefreshProvidersResult {
    let any_updated = provider_result.outcome == ProviderRefreshOutcome::Updated;
    let message = provider_result.message.clone();
    RefreshProvidersResult {
        app_state,
        provider_results: vec![provider_result],
        any_updated,
        message,
    }
}

fn provider_label(provider_id: ProviderId) -> &'static str {
    match provider_id {
        ProviderId::Codex => "Codex",
        ProviderId::DeepSeek => "DeepSeek",
        ProviderId::Kimi => "Kimi",
    }
}

pub fn load_app_state(app: &AppHandle) -> Result<AppState, String> {
    let store = store_for_app(app)?;
    if let (_, Some(state)) = read_and_sync_credential_facts(app)? {
        return Ok(state);
    }
    store
        .load()
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)
}

fn read_and_sync_credential_facts(
    app: &AppHandle,
) -> Result<(Vec<ProviderCredentialStatus>, Option<AppState>), String> {
    let Some(coordinator) = app
        .try_state::<RefreshCoordinator>()
        .map(|state| state.inner().clone())
    else {
        return Ok((
            credentials::provider_credential_status_with_store(&WindowsCredentialStore),
            None,
        ));
    };
    let Some(deepseek_permit) = coordinator.try_begin(ProviderId::DeepSeek) else {
        return Ok((
            credentials::provider_credential_status_with_store(&WindowsCredentialStore),
            None,
        ));
    };
    let Some(kimi_permit) = coordinator.try_begin(ProviderId::Kimi) else {
        drop(deepseek_permit);
        return Ok((
            credentials::provider_credential_status_with_store(&WindowsCredentialStore),
            None,
        ));
    };
    let statuses = credentials::provider_credential_status_with_store(&WindowsCredentialStore);
    let configurations = statuses
        .iter()
        .filter_map(|status| match status.availability {
            CredentialAvailability::Configured => Some((status.provider_id, true)),
            CredentialAvailability::NotConfigured => Some((status.provider_id, false)),
            CredentialAvailability::Unavailable => None,
        })
        .collect::<Vec<_>>();
    if configurations.is_empty() {
        return Ok((statuses, None));
    }
    let _commit = coordinator
        .commit_lock
        .lock()
        .map_err(|_| "凭据状态同步锁不可用。".to_string())?;
    let state = store_for_app(app)?
        .sync_provider_configurations(&configurations)
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)?;
    drop(_commit);
    drop(kimi_permit);
    drop(deepseek_permit);
    Ok((statuses, Some(state)))
}

pub fn state_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("quotadock-state.json"))
        .map_err(|error| error.to_string())
}

fn fetch_usage_from_codex_cli() -> Result<QuotaSnapshot, String> {
    let app_server_error = match codex_command(&["app-server"]) {
        Ok(mut command) => {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x0800_0000);
            }
            match app_server::fetch_rate_limits(
                command,
                Duration::from_secs(12),
                version::APP_VERSION,
            ) {
                Ok(snapshot) => return Ok(snapshot),
                Err(error) => error,
            }
        }
        Err(error) => error,
    };
    let output = run_codex_status_pty(Duration::from_secs(45)).map_err(|pty_error| {
        format!("结构化查询失败：{app_server_error}；兼容查询也失败：{pty_error}")
    })?;

    let mut result =
        parse_status_text_with_source(&output, ParseClock::now(), SnapshotSource::CodexCli);
    result.snapshot.status_message = "已通过 Codex CLI /status 兼容模式更新额度。".to_string();
    result.snapshot.raw_text.clear();
    if result.snapshot.has_usage() {
        Ok(result.snapshot)
    } else {
        Err("Codex CLI 没有返回可识别的额度，请稍后重试。".to_string())
    }
}

fn run_codex_status_pty(timeout: Duration) -> Result<String, String> {
    let Some(target) = find_codex_binary() else {
        return Err("未找到 Codex CLI，请确认 codex 命令可用。".to_string());
    };

    #[cfg(windows)]
    {
        return windows_conpty::capture_status(&target, timeout);
    }

    #[cfg(not(windows))]
    {
        let _ = target;
        let _ = timeout;
        Err("自动查询当前仅支持 Windows。".to_string())
    }
}

#[allow(dead_code)]
pub(crate) fn probe_codex_status_output(timeout: Duration) -> Result<String, String> {
    run_codex_status_pty(timeout)
}

#[allow(dead_code)]
pub fn prewarm_codex_status_session() {
    let Some(target) = find_codex_binary() else {
        return;
    };

    #[cfg(windows)]
    windows_conpty::prewarm_status_session(target);
}

#[cfg(test)]
fn status_probe_command_override() -> Option<std::ffi::OsString> {
    std::env::var_os("QUOTADOCK_STATUS_PROBE_COMMAND")
}

#[cfg(not(test))]
fn status_probe_command_override() -> Option<std::ffi::OsString> {
    None
}

#[cfg(test)]
fn status_probe_log_path() -> Option<std::ffi::OsString> {
    std::env::var_os("QUOTADOCK_STATUS_PROBE_LOG")
}

#[cfg(not(test))]
fn status_probe_log_path() -> Option<std::ffi::OsString> {
    None
}

#[cfg(windows)]
mod windows_conpty {
    use super::{
        codex_status_output_ready, codex_update_prompt_visible, is_cmd_shim,
        should_send_status_command, status_command_waiting_for_enter,
        status_output_ready_after_settle, status_probe_command_override, status_probe_log_path,
    };
    use std::ffi::{c_void, OsStr};
    use std::io::{Read, Write};
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::ptr::{null, null_mut};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, HANDLE, HANDLE_FLAG_INHERIT, HMODULE, INVALID_HANDLE_VALUE,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    use windows_sys::Win32::System::Console::{COORD, HPCON};
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
        InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
        WaitForSingleObject, EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST,
        PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTF_USESTDHANDLES,
        STARTUPINFOEXW, STARTUPINFOW,
    };

    const PSEUDOCONSOLE_RESIZE_QUIRK: u32 = 0x2;
    const PSEUDOCONSOLE_WIN32_INPUT_MODE: u32 = 0x4;
    const PSEUDOCONSOLE_INHERIT_CURSOR: u32 = 0x1;
    pub fn capture_status(target: &Path, timeout: Duration) -> Result<String, String> {
        if status_probe_command_override().is_some() {
            return capture_status_with_portable_pty(target, timeout);
        }

        capture_status_with_portable_pty(target, timeout)
    }

    pub fn prewarm_status_session(_target: PathBuf) {}

    fn capture_status_with_portable_pty(
        target: &Path,
        timeout: Duration,
    ) -> Result<String, String> {
        use portable_pty::{native_pty_system, PtySize};

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 30,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("创建 Codex 伪终端失败：{error}"))?;

        let mut command = portable_command(target);
        if let Some(cwd) = user_profile_path() {
            command.cwd(cwd);
        }
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("启动 Codex CLI 失败：{error}"))?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("读取 Codex 伪终端失败：{error}"))?;
        let mut writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("打开 Codex 输入通道失败：{error}"))?;
        let (sender, receiver) = mpsc::channel();
        let _reader_thread = thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        if sender.send(buffer[..read].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        if status_probe_command_override().is_some() {
            let started = Instant::now();
            let mut output = Vec::new();
            let mut cursor_reported = false;
            loop {
                drain_receiver(&receiver, &mut output);
                let text = String::from_utf8_lossy(&output).to_string();
                respond_to_cursor_query(&mut writer, &text, &mut cursor_reported)?;
                if let Ok(Some(status)) = child.try_wait() {
                    drain_receiver_for(&receiver, &mut output, Duration::from_millis(800));
                    let text = String::from_utf8_lossy(&output).to_string();
                    write_probe_log(&format!(
                        "exit_code=Some({})\nbytes={}\n{text}",
                        status.exit_code(),
                        output.len()
                    ));
                    return Ok(text);
                }
                if started.elapsed() > timeout.min(Duration::from_secs(10)) {
                    let _ = child.kill();
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            drain_receiver_for(&receiver, &mut output, Duration::from_millis(800));
            let text = String::from_utf8_lossy(&output).to_string();
            write_probe_log(&format!(
                "exit_code={:?}\nbytes={}\n{text}",
                None::<u32>,
                output.len()
            ));
            return Ok(text);
        }

        let started = Instant::now();
        let mut output = Vec::new();
        let mut sent_count = 0_u8;
        let mut last_sent = started;
        let mut last_output_change = started;
        let mut cursor_reported = false;

        loop {
            let output_length_before_drain = output.len();
            drain_receiver(&receiver, &mut output);
            if output.len() != output_length_before_drain {
                last_output_change = Instant::now();
            }
            let text = String::from_utf8_lossy(&output).to_string();
            respond_to_cursor_query(&mut writer, &text, &mut cursor_reported)?;

            if codex_update_prompt_visible(&text) {
                let _ = child.kill();
                return Err(
                    "Codex CLI 正在显示更新提示，已停止自动查询以避免误触更新。请先在终端完成更新或选择跳过后重试。"
                        .to_string(),
                );
            }

            if should_send_status_command(&text, started, last_sent, sent_count, cursor_reported) {
                if sent_count == 0 {
                    output.clear();
                    write_status_command(&mut writer)?;
                } else if status_command_waiting_for_enter(&text) {
                    press_enter(&mut writer)?;
                } else {
                    write_status_command(&mut writer)?;
                }
                sent_count += 1;
                last_sent = Instant::now();
                last_output_change = last_sent;
                continue;
            }

            if sent_count > 0
                && status_output_ready_after_settle(&text, last_output_change.elapsed())
            {
                let _ = child.kill();
                write_probe_log(&format!(
                    "exit_code={:?}\nbytes={}\n{text}",
                    None::<u32>,
                    output.len()
                ));
                return Ok(text);
            }

            if let Ok(Some(status)) = child.try_wait() {
                drain_receiver_for(&receiver, &mut output, Duration::from_millis(800));
                let text = String::from_utf8_lossy(&output).to_string();
                write_probe_log(&format!(
                    "exit_code=Some({})\nbytes={}\n{text}",
                    status.exit_code(),
                    output.len()
                ));
                if codex_status_output_ready(&text) {
                    return Ok(text);
                }
                return Err("Codex CLI /status 自动查询失败，请稍后重试。".to_string());
            }

            if started.elapsed() > timeout {
                break;
            }

            thread::sleep(Duration::from_millis(50));
        }

        let _ = child.kill();
        drop(writer);
        drop(pair.master);
        drain_receiver_for(&receiver, &mut output, Duration::from_millis(800));
        let text = String::from_utf8_lossy(&output).to_string();
        write_probe_log(&format!(
            "exit_code={:?}\nbytes={}\n{text}",
            None::<u32>,
            output.len()
        ));
        if codex_status_output_ready(&text) {
            return Ok(text);
        }
        Err("Codex CLI /status 自动查询失败，请稍后重试。".to_string())
    }

    fn respond_to_cursor_query(
        writer: &mut Box<dyn std::io::Write + Send>,
        text: &str,
        already_sent: &mut bool,
    ) -> Result<(), String> {
        if *already_sent || !text.contains("\u{1b}[6n") {
            return Ok(());
        }
        writer
            .write_all(b"\x1b[1;1R")
            .map_err(|error| format!("回应 Codex 终端查询失败：{error}"))?;
        writer
            .flush()
            .map_err(|error| format!("回应 Codex 终端查询失败：{error}"))?;
        *already_sent = true;
        Ok(())
    }

    fn write_status_command(writer: &mut Box<dyn Write + Send>) -> Result<(), String> {
        writer
            .write_all(b"/status\r")
            .map_err(|error| format!("发送 /status 失败：{error}"))?;
        writer
            .flush()
            .map_err(|error| format!("发送 /status 失败：{error}"))
    }

    fn press_enter(writer: &mut Box<dyn Write + Send>) -> Result<(), String> {
        writer
            .write_all(b"\r")
            .map_err(|error| format!("发送 /status 失败：{error}"))?;
        writer
            .flush()
            .map_err(|error| format!("发送 /status 失败：{error}"))
    }

    fn portable_command(target: &Path) -> portable_pty::CommandBuilder {
        use portable_pty::CommandBuilder;

        if let Some(override_command) = status_probe_command_override() {
            let mut command = CommandBuilder::new("cmd.exe");
            command.arg("/D");
            command.arg("/C");
            command.arg(override_command);
            set_terminal_environment(&mut command);
            return command;
        }

        let mut command = if is_cmd_shim(target) {
            let mut command = CommandBuilder::new("cmd.exe");
            command.arg("/D");
            command.arg("/C");
            command.arg(target);
            command.arg("-c");
            command.arg("mcp_servers={}");
            command.arg("--no-alt-screen");
            command
        } else {
            let mut command = CommandBuilder::new(target);
            command.arg("-c");
            command.arg("mcp_servers={}");
            command.arg("--no-alt-screen");
            command
        };
        set_terminal_environment(&mut command);
        command
    }

    fn set_terminal_environment(command: &mut portable_pty::CommandBuilder) {
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
    }

    fn user_profile_path() -> Option<std::path::PathBuf> {
        std::env::var_os("USERPROFILE")
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from)
    }

    fn drain_receiver(receiver: &mpsc::Receiver<Vec<u8>>, output: &mut Vec<u8>) {
        while let Ok(chunk) = receiver.try_recv() {
            output.extend_from_slice(&chunk);
        }
    }

    fn drain_receiver_for(
        receiver: &mpsc::Receiver<Vec<u8>>,
        output: &mut Vec<u8>,
        duration: Duration,
    ) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            drain_receiver(receiver, output);
            thread::sleep(Duration::from_millis(25));
        }
        drain_receiver(receiver, output);
    }

    #[allow(dead_code)]
    fn capture_status_with_raw_conpty(target: &Path, timeout: Duration) -> Result<String, String> {
        unsafe {
            let mut input_read = Handle::default();
            let mut input_write = Handle::default();
            let mut output_read = Handle::default();
            let mut output_write = Handle::default();

            create_pipe(&mut input_read, &mut input_write, "创建 Codex 输入管道失败")?;
            create_pipe(
                &mut output_read,
                &mut output_write,
                "创建 Codex 输出管道失败",
            )?;

            let conpty = ConptyApi::load()?;
            let mut hpc: HPCON = 0;
            let hr = (conpty.create)(
                COORD { X: 100, Y: 30 },
                input_read.raw(),
                output_write.raw(),
                PSEUDOCONSOLE_INHERIT_CURSOR
                    | PSEUDOCONSOLE_RESIZE_QUIRK
                    | PSEUDOCONSOLE_WIN32_INPUT_MODE,
                &mut hpc,
            );
            if hr < 0 {
                return Err(format!("创建 Codex 伪终端失败：HRESULT 0x{hr:08X}"));
            }
            let pseudo_console = PseudoConsole {
                hpc,
                close: conpty.close,
            };

            let mut attributes = AttributeList::new(pseudo_console.raw())?;
            let mut startup: STARTUPINFOEXW = zeroed();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
            startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
            startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
            startup.lpAttributeList = attributes.raw();

            let mut process_info: PROCESS_INFORMATION = zeroed();
            let mut command_line = command_line(target);
            let cwd = user_profile_wide();
            let cwd_ptr = cwd.as_ref().map(|value| value.as_ptr()).unwrap_or(null());
            let created = CreateProcessW(
                null(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT,
                null(),
                cwd_ptr,
                &startup as *const STARTUPINFOEXW as *const STARTUPINFOW,
                &mut process_info,
            );
            if created == 0 {
                return Err(last_error("启动 Codex CLI 失败"));
            }
            let process = ChildProcess::new(process_info);
            drop(attributes);
            let reader = OutputReader::start(output_read);

            let started = Instant::now();
            let mut output = Vec::new();
            let mut sent_count = 0_u8;
            let mut last_sent = started;
            let mut last_output_change = started;

            loop {
                let output_length_before_drain = output.len();
                reader.drain(&mut output);
                if output.len() != output_length_before_drain {
                    last_output_change = Instant::now();
                }
                let text = String::from_utf8_lossy(&output).to_string();

                if should_send_status_command(&text, started, last_sent, sent_count, false) {
                    write_all(input_write.raw(), b"/status\r")?;
                    sent_count += 1;
                    last_sent = Instant::now();
                    last_output_change = last_sent;
                    continue;
                }

                if sent_count > 0
                    && status_output_ready_after_settle(&text, last_output_change.elapsed())
                {
                    process.terminate();
                    write_probe_log(&format!(
                        "exit_code={:?}\nbytes={}\n{text}",
                        process.exit_code(),
                        output.len()
                    ));
                    return Ok(text);
                }

                match WaitForSingleObject(process.handle(), 0) {
                    WAIT_OBJECT_0 => break,
                    WAIT_TIMEOUT => {}
                    _ => break,
                }
                if started.elapsed() > timeout {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }

            process.terminate();
            drop(input_write);
            drop(input_read);
            drop(output_write);
            drop(pseudo_console);
            reader.drain_for(&mut output, Duration::from_millis(800));
            let text = String::from_utf8_lossy(&output).to_string();
            write_probe_log(&format!(
                "exit_code={:?}\nbytes={}\n{text}",
                process.exit_code(),
                output.len()
            ));
            if codex_status_output_ready(&text) {
                return Ok(text);
            }
            Err("Codex CLI /status 自动查询失败，请稍后重试。".to_string())
        }
    }

    unsafe fn create_pipe(
        read: &mut Handle,
        write: &mut Handle,
        context: &str,
    ) -> Result<(), String> {
        let mut read_raw: HANDLE = null_mut();
        let mut write_raw: HANDLE = null_mut();
        if CreatePipe(&mut read_raw, &mut write_raw, null(), 0) == 0 {
            return Err(last_error(context));
        }
        windows_sys::Win32::Foundation::SetHandleInformation(read_raw, HANDLE_FLAG_INHERIT, 0);
        windows_sys::Win32::Foundation::SetHandleInformation(write_raw, HANDLE_FLAG_INHERIT, 0);
        *read = Handle(read_raw);
        *write = Handle(write_raw);
        Ok(())
    }

    unsafe fn write_all(handle: HANDLE, bytes: &[u8]) -> Result<(), String> {
        let mut offset = 0_usize;
        while offset < bytes.len() {
            let mut written = 0_u32;
            if WriteFile(
                handle,
                bytes[offset..].as_ptr(),
                (bytes.len() - offset) as u32,
                &mut written,
                null_mut(),
            ) == 0
            {
                return Err(last_error("发送 /status 失败"));
            }
            if written == 0 {
                return Err("发送 /status 失败：写入 0 字节。".to_string());
            }
            offset += written as usize;
        }
        Ok(())
    }

    fn command_line(target: &Path) -> Vec<u16> {
        if let Some(override_command) = status_probe_command_override() {
            return wide_null(override_command.as_os_str());
        }

        let target = target.to_string_lossy();
        let command = if is_cmd_shim(Path::new(target.as_ref())) {
            format!(
                "cmd.exe /D /C {} -c mcp_servers={{}} --no-alt-screen",
                quote_arg(&target)
            )
        } else {
            format!("{} -c mcp_servers={{}} --no-alt-screen", quote_arg(&target))
        };
        wide_null(&command)
    }

    fn quote_arg(value: &str) -> String {
        if !value.contains([' ', '\t', '"']) {
            return value.to_string();
        }

        let mut quoted = String::from("\"");
        for character in value.chars() {
            if character == '"' {
                quoted.push('\\');
            }
            quoted.push(character);
        }
        quoted.push('"');
        quoted
    }

    fn user_profile_wide() -> Option<Vec<u16>> {
        std::env::var_os("USERPROFILE")
            .filter(|value| !value.is_empty())
            .map(|value| wide_null(value.as_os_str()))
    }

    fn wide_null(value: impl AsRef<OsStr>) -> Vec<u16> {
        value
            .as_ref()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn last_error(context: &str) -> String {
        unsafe { format!("{context}：Win32 错误 {}", GetLastError()) }
    }

    fn write_probe_log(output: &str) {
        let Some(path) = status_probe_log_path() else {
            return;
        };
        let _ = std::fs::write(path, probe_log_summary(output));
    }

    fn probe_log_summary(output: &str) -> String {
        format!(
            "format_version=1\ncaptured_bytes={}\nraw_output=redacted\n",
            output.len()
        )
    }

    #[derive(Default)]
    struct Handle(HANDLE);

    impl Handle {
        fn raw(&self) -> HANDLE {
            self.0
        }

        fn take(&mut self) -> HANDLE {
            let raw = self.0;
            self.0 = null_mut();
            raw
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CloseHandle(self.0);
                }
                self.0 = null_mut();
            }
        }
    }

    struct ReaderHandle(HANDLE);

    unsafe impl Send for ReaderHandle {}

    impl ReaderHandle {
        fn into_raw(self) -> HANDLE {
            self.0
        }
    }

    struct OutputReader {
        receiver: mpsc::Receiver<Vec<u8>>,
        _thread: thread::JoinHandle<()>,
    }

    impl OutputReader {
        unsafe fn start(mut output_read: Handle) -> Self {
            let reader_handle = ReaderHandle(output_read.take());
            let (sender, receiver) = mpsc::channel();
            let thread = thread::spawn(move || {
                let handle = Handle(reader_handle.into_raw());
                loop {
                    let mut buffer = vec![0_u8; 4096];
                    let mut read = 0_u32;
                    let ok = unsafe {
                        ReadFile(
                            handle.raw(),
                            buffer.as_mut_ptr(),
                            buffer.len() as u32,
                            &mut read,
                            null_mut(),
                        )
                    };
                    if ok == 0 {
                        let error = unsafe { GetLastError() };
                        let _ = sender.send(format!("\n[quotadock-reader-error:{error}]\n").into());
                        break;
                    }
                    if read == 0 {
                        let _ = sender.send(b"\n[quotadock-reader-eof]\n".to_vec());
                        break;
                    }
                    buffer.truncate(read as usize);
                    if sender.send(buffer).is_err() {
                        break;
                    }
                }
            });

            Self {
                receiver,
                _thread: thread,
            }
        }

        fn drain(&self, output: &mut Vec<u8>) {
            while let Ok(chunk) = self.receiver.try_recv() {
                output.extend_from_slice(&chunk);
            }
        }

        fn drain_for(&self, output: &mut Vec<u8>, duration: Duration) {
            let deadline = Instant::now() + duration;
            while Instant::now() < deadline {
                self.drain(output);
                thread::sleep(Duration::from_millis(25));
            }
            self.drain(output);
        }
    }

    type CreatePseudoConsoleFn =
        unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut HPCON) -> i32;
    type ClosePseudoConsoleFn = unsafe extern "system" fn(HPCON);

    struct ConptyApi {
        _module: HMODULE,
        create: CreatePseudoConsoleFn,
        close: ClosePseudoConsoleFn,
    }

    impl ConptyApi {
        unsafe fn load() -> Result<Self, String> {
            let module = LoadLibraryW(wide_null("kernel32.dll").as_ptr());
            if module.is_null() {
                return Err(last_error("加载 kernel32.dll 失败"));
            }

            let Some(create_proc) = GetProcAddress(module, c"CreatePseudoConsole".as_ptr().cast())
            else {
                return Err("当前 Windows 不支持 ConPTY：缺少 CreatePseudoConsole。".to_string());
            };
            let Some(close_proc) = GetProcAddress(module, c"ClosePseudoConsole".as_ptr().cast())
            else {
                return Err("当前 Windows 不支持 ConPTY：缺少 ClosePseudoConsole。".to_string());
            };

            Ok(Self {
                _module: module,
                create: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    CreatePseudoConsoleFn,
                >(create_proc),
                close: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    ClosePseudoConsoleFn,
                >(close_proc),
            })
        }
    }

    struct PseudoConsole {
        hpc: HPCON,
        close: ClosePseudoConsoleFn,
    }

    impl PseudoConsole {
        fn raw(&self) -> HPCON {
            self.hpc
        }
    }

    impl Drop for PseudoConsole {
        fn drop(&mut self) {
            if self.hpc != 0 {
                unsafe {
                    (self.close)(self.hpc);
                }
                self.hpc = 0;
            }
        }
    }

    struct AttributeList {
        data: Vec<u8>,
        ptr: LPPROC_THREAD_ATTRIBUTE_LIST,
    }

    impl AttributeList {
        unsafe fn new(hpc: HPCON) -> Result<Self, String> {
            let mut size = 0_usize;
            let _ = InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
            if size == 0 {
                return Err(last_error("初始化 Codex 伪终端属性失败"));
            }

            let mut data = vec![0_u8; size];
            let ptr = data.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
            if InitializeProcThreadAttributeList(ptr, 1, 0, &mut size) == 0 {
                return Err(last_error("初始化 Codex 伪终端属性失败"));
            }

            let hpc_value = hpc;
            if UpdateProcThreadAttribute(
                ptr,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                &hpc_value as *const HPCON as *const c_void,
                size_of::<HPCON>(),
                null_mut(),
                null(),
            ) == 0
            {
                return Err(last_error("绑定 Codex 伪终端失败"));
            }

            Ok(Self { data, ptr })
        }

        fn raw(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
            self.ptr
        }
    }

    impl Drop for AttributeList {
        fn drop(&mut self) {
            if !self.ptr.is_null() {
                unsafe {
                    DeleteProcThreadAttributeList(self.ptr);
                }
                self.ptr = null_mut();
            }
            let _ = self.data.len();
        }
    }

    struct ChildProcess {
        process: Handle,
        thread: Handle,
    }

    impl ChildProcess {
        unsafe fn new(info: PROCESS_INFORMATION) -> Self {
            Self {
                process: Handle(info.hProcess),
                thread: Handle(info.hThread),
            }
        }

        fn handle(&self) -> HANDLE {
            self.process.raw()
        }

        unsafe fn terminate(&self) {
            if !self.process.raw().is_null() {
                TerminateProcess(self.process.raw(), 1);
                WaitForSingleObject(self.process.raw(), 1000);
            }
            let _ = self.thread.raw();
        }

        unsafe fn exit_code(&self) -> Option<u32> {
            if self.process.raw().is_null() {
                return None;
            }
            let mut code = 0_u32;
            (GetExitCodeProcess(self.process.raw(), &mut code) != 0).then_some(code)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::probe_log_summary;

        #[test]
        fn probe_log_summary_never_persists_terminal_contents() {
            let terminal_output =
                "Authorization: Bearer fake-secret\n1 周剩余 42%\n充值余额 100.00";
            let summary = probe_log_summary(terminal_output);

            assert!(summary.contains(&format!("captured_bytes={}", terminal_output.len())));
            assert!(summary.contains("raw_output=redacted"));
            assert!(!summary.contains("fake-secret"));
            assert!(!summary.contains("42%"));
            assert!(!summary.contains("100.00"));
        }
    }
}

fn should_send_status_command(
    output: &str,
    started: Instant,
    last_sent: Instant,
    sent_count: u8,
    _terminal_ready: bool,
) -> bool {
    if sent_count >= 6 {
        return false;
    }
    if sent_count == 0 {
        return started.elapsed() >= Duration::from_secs(15) && codex_prompt_ready(output);
    }
    last_sent.elapsed() >= Duration::from_secs(3)
}

fn codex_prompt_ready(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("use /skills to list available skills")
        || lower.contains("implement {feature}")
        || (lower.contains("model:") && lower.contains("directory:"))
}

fn codex_update_prompt_visible(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("update available!") && lower.contains("press enter to continue")
}

fn status_command_waiting_for_enter(output: &str) -> bool {
    output.contains("› /status") || output.contains("> /status")
}

fn codex_status_output_ready(output: &str) -> bool {
    let parsed = parse_status_text_with_source(output, ParseClock::now(), SnapshotSource::CodexCli);
    parsed.snapshot.has_usage()
}

fn status_output_ready_after_settle(output: &str, quiet_for: Duration) -> bool {
    quiet_for >= STATUS_OUTPUT_SETTLE_DELAY && codex_status_output_ready(output)
}

fn low_quota_notification(
    previous: Option<&QuotaSnapshot>,
    current: &QuotaSnapshot,
) -> Option<String> {
    let previous = previous?;
    let old = previous.weekly.remaining_percent;
    let new = current.weekly.remaining_percent;
    (old.is_some_and(|percent| percent > LOW_USAGE_THRESHOLD_PERCENT)
        && new.is_some_and(|percent| percent <= LOW_USAGE_THRESHOLD_PERCENT))
    .then(|| format!("1 周剩余 {}%", new.unwrap_or_default()))
}

fn notify_low_quota(app: &AppHandle, message: &str) {
    #[cfg(feature = "desktop")]
    {
        if matches!(
            app.notification().permission_state(),
            Ok(PermissionState::Granted)
        ) {
            if let Err(error) = app
                .notification()
                .builder()
                .title("QuotaDock 低额度提醒")
                .body(message)
                .show()
            {
                eprintln!("show low quota notification failed: {error}");
            }
        }
    }
    #[cfg(not(feature = "desktop"))]
    let _ = (app, message);
}

#[derive(Debug)]
struct CodexOutput {
    success: bool,
    stdout: String,
}

fn run_codex(args: &[&str], timeout: Duration) -> Result<CodexOutput, String> {
    let mut command = codex_command(args)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    let mut child = command
        .spawn()
        .map_err(|_| "未找到 Codex CLI，请确认 codex 命令可用。".to_string())?;
    let started = Instant::now();

    loop {
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Codex CLI 查询超时，请稍后重试。".to_string());
        }

        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("读取 Codex CLI 输出失败：{error}"))?;
                return Ok(CodexOutput {
                    success: output.status.success(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => return Err(format!("Codex CLI 查询失败：{error}")),
        }
    }
}

fn codex_command(args: &[&str]) -> Result<Command, String> {
    let Some(target) = find_codex_binary() else {
        return Err("未找到 Codex CLI，请确认 codex 命令可用。".to_string());
    };

    let mut command = if is_cmd_shim(&target) {
        let mut command = Command::new("cmd");
        command.arg("/D").arg("/C").arg(&target);
        command
    } else {
        Command::new(&target)
    };
    command.args(args);
    Ok(command)
}

fn find_codex_binary() -> Option<PathBuf> {
    codex_candidate_paths()
        .into_iter()
        .find(|path| path.is_file())
}

fn codex_candidate_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    for variable in ["APPDATA", "USERPROFILE", "LOCALAPPDATA"] {
        if let Some(value) = std::env::var_os(variable) {
            let base = PathBuf::from(value);
            match variable {
                "APPDATA" => {
                    push_npm_managed_codex(&mut candidates, &base.join("npm"));
                    push_codex_names(&mut candidates, &base.join("npm"));
                }
                "USERPROFILE" => {
                    push_npm_managed_codex(
                        &mut candidates,
                        &base.join("AppData").join("Roaming").join("npm"),
                    );
                    push_codex_names(
                        &mut candidates,
                        &base.join("AppData").join("Roaming").join("npm"),
                    );
                    push_codex_names(
                        &mut candidates,
                        &base
                            .join("AppData")
                            .join("Local")
                            .join("Microsoft")
                            .join("WindowsApps"),
                    );
                }
                "LOCALAPPDATA" => {
                    push_codex_names(&mut candidates, &base.join("Microsoft").join("WindowsApps"))
                }
                _ => {}
            }
        }
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            push_codex_names(&mut candidates, &dir);
        }
    }

    candidates
}

fn push_npm_managed_codex(candidates: &mut Vec<PathBuf>, npm_dir: &Path) {
    candidates.push(
        npm_dir
            .join("node_modules")
            .join("@openai")
            .join("codex")
            .join("node_modules")
            .join("@openai")
            .join("codex-win32-x64")
            .join("vendor")
            .join("x86_64-pc-windows-msvc")
            .join("bin")
            .join("codex.exe"),
    );
}

fn push_codex_names(candidates: &mut Vec<PathBuf>, dir: &Path) {
    #[cfg(windows)]
    for name in ["codex.exe", "codex.cmd", "codex.bat", "codex"] {
        candidates.push(dir.join(name));
    }

    #[cfg(not(windows))]
    candidates.push(dir.join("codex"));
}

fn is_cmd_shim(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        })
}

fn store_for_app(app: &AppHandle) -> Result<UsageStore, String> {
    Ok(UsageStore::new(state_path(app)?))
}

fn sync_tray(_app: &AppHandle, _state: &AppState) {
    #[cfg(feature = "desktop")]
    crate::tray::sync_from_app_state(_app, _state);
}

fn emit_usage_state(app: &AppHandle, result: &RefreshUsageResult) {
    if let Err(error) = app.emit(USAGE_STATE_CHANGED_EVENT, result.clone()) {
        eprintln!("emit usage state failed: {error}");
    }
}

fn emit_provider_state(app: &AppHandle, result: &RefreshProvidersResult) {
    if let Err(error) = app.emit(PROVIDER_STATE_CHANGED_EVENT, result.clone()) {
        eprintln!("emit provider state failed: {error}");
    }
}

fn emit_configuration_state(
    app: &AppHandle,
    app_state: AppState,
    provider_id: ProviderId,
    message: &str,
) {
    sync_tray(app, &app_state);
    emit_provider_state(
        app,
        &RefreshProvidersResult {
            app_state,
            provider_results: vec![ProviderRefreshResult {
                provider_id,
                outcome: ProviderRefreshOutcome::Unchanged,
                message: message.to_string(),
                error_category: None,
            }],
            any_updated: false,
            message: message.to_string(),
        },
    );
}

fn to_command_error(error: StoreError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use crate::commands::{
        adaptive_refresh_interval, auto_refresh_wait_timeout, delete_provider_credential_and_sync,
        failure_backoff_interval, legacy_codex_result, next_auto_refresh_schedule,
        next_provider_auto_schedule, refresh_providers_blocking,
        refresh_providers_blocking_with_callback, set_provider_credential_and_sync,
        AutoRefreshCompletion, ProviderFailure, ProviderFetchOutcome, ProviderFetcher,
        RefreshCoordinator, AUTO_BASE_REFRESH_INTERVAL, AUTO_BUSY_RETRY_INTERVAL,
        AUTO_LOW_USAGE_REFRESH_INTERVAL, AUTO_POST_RESET_REFRESH_DELAY,
        AUTO_REFRESH_STOP_POLL_INTERVAL, AUTO_RESET_WATCH_WINDOW, STATUS_OUTPUT_SETTLE_DELAY,
    };
    use crate::credentials::{CredentialStore, CredentialStoreError, CredentialStoreErrorKind};
    use crate::models::{
        AppSettings, AppState, DeepSeekBalance, DeepSeekSnapshot, KimiRegion, KimiSnapshot,
        ProviderErrorCategory, ProviderHealth, ProviderId, ProviderRefreshOutcome,
        ProviderRefreshResult, ProviderSnapshot, ProviderStates, QuotaReading, QuotaSnapshot,
        RefreshProvidersResult, RefreshUsageResult, SnapshotSource, StorageStatus, PROVIDER_ORDER,
        STATE_VERSION,
    };
    use crate::usage_store::UsageStore;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Condvar, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn auto_refresh_completion_reports_aborted_workers_and_never_double_sends() {
        let (aborted_tx, aborted_rx) = std::sync::mpsc::channel();
        drop(AutoRefreshCompletion::new(aborted_tx, ProviderId::DeepSeek));
        let (provider_id, outcome) = aborted_rx.recv().unwrap();
        assert_eq!(provider_id, ProviderId::DeepSeek);
        assert_eq!(outcome.unwrap_err(), "自动刷新任务异常结束，稍后将重试。");

        let (completed_tx, completed_rx) = std::sync::mpsc::channel();
        AutoRefreshCompletion::new(completed_tx, ProviderId::Kimi)
            .complete(Err("expected failure".to_string()));
        let (provider_id, outcome) = completed_rx.recv().unwrap();
        assert_eq!(provider_id, ProviderId::Kimi);
        assert_eq!(outcome.unwrap_err(), "expected failure");
        assert!(completed_rx.recv().is_err());
    }

    #[test]
    fn auto_refresh_scheduler_checks_for_shutdown_within_one_second() {
        assert_eq!(
            auto_refresh_wait_timeout(Duration::from_secs(5 * 60)),
            AUTO_REFRESH_STOP_POLL_INTERVAL
        );
        assert_eq!(
            auto_refresh_wait_timeout(Duration::from_millis(250)),
            Duration::from_millis(250)
        );
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let unique = format!(
                "quotadock-commands-{name}-{}-{}",
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

    struct FakeFetcher {
        outcomes: HashMap<ProviderId, ProviderFetchOutcome>,
    }

    impl ProviderFetcher for FakeFetcher {
        fn fetch(&self, provider_id: ProviderId) -> ProviderFetchOutcome {
            self.outcomes
                .get(&provider_id)
                .cloned()
                .unwrap_or(ProviderFetchOutcome::NotConfigured)
        }
    }

    struct ConcurrentFetcher {
        gate: (Mutex<(usize, usize, usize)>, Condvar),
    }

    impl ConcurrentFetcher {
        fn new() -> Self {
            Self {
                gate: (Mutex::new((0, 0, 0)), Condvar::new()),
            }
        }

        fn max_concurrency(&self) -> usize {
            self.gate.0.lock().unwrap().2
        }
    }

    impl ProviderFetcher for ConcurrentFetcher {
        fn fetch(&self, provider_id: ProviderId) -> ProviderFetchOutcome {
            let (lock, wake) = &self.gate;
            let mut counters = lock.lock().unwrap();
            counters.0 += 1;
            counters.1 += 1;
            counters.2 = counters.2.max(counters.1);
            if counters.0 == 3 {
                wake.notify_all();
            } else {
                let (next, _) = wake
                    .wait_timeout_while(counters, Duration::from_millis(250), |(arrived, _, _)| {
                        *arrived < 3
                    })
                    .unwrap();
                counters = next;
            }
            counters.1 -= 1;
            drop(counters);
            ProviderFetchOutcome::Updated(provider_snapshot(provider_id, "unix:parallel"))
        }
    }

    struct OrderedFetcher;

    impl ProviderFetcher for OrderedFetcher {
        fn fetch(&self, provider_id: ProviderId) -> ProviderFetchOutcome {
            std::thread::sleep(match provider_id {
                ProviderId::Codex => Duration::from_millis(100),
                ProviderId::DeepSeek => Duration::from_millis(1),
                ProviderId::Kimi => Duration::from_millis(40),
            });
            ProviderFetchOutcome::Updated(provider_snapshot(provider_id, "unix:ordered"))
        }
    }

    struct BlockingFetcher {
        entered: std::sync::mpsc::SyncSender<()>,
        release: (Mutex<bool>, Condvar),
    }

    impl ProviderFetcher for BlockingFetcher {
        fn fetch(&self, provider_id: ProviderId) -> ProviderFetchOutcome {
            self.entered.send(()).unwrap();
            let (lock, wake) = &self.release;
            let released = lock.lock().unwrap();
            drop(wake.wait_while(released, |released| !*released).unwrap());
            ProviderFetchOutcome::Updated(provider_snapshot(provider_id, "unix:released"))
        }
    }

    #[derive(Default)]
    struct MemoryCredentialStore {
        entries: Mutex<HashMap<&'static str, String>>,
    }

    impl CredentialStore for MemoryCredentialStore {
        fn set_password(
            &self,
            account: &'static str,
            secret: &str,
        ) -> Result<(), CredentialStoreError> {
            self.entries
                .lock()
                .unwrap()
                .insert(account, secret.to_string());
            Ok(())
        }

        fn get_password(&self, account: &'static str) -> Result<String, CredentialStoreError> {
            self.entries
                .lock()
                .unwrap()
                .get(account)
                .cloned()
                .ok_or_else(|| CredentialStoreError::new(CredentialStoreErrorKind::NotFound))
        }

        fn delete_credential(&self, account: &'static str) -> Result<(), CredentialStoreError> {
            self.entries
                .lock()
                .unwrap()
                .remove(account)
                .map(|_| ())
                .ok_or_else(|| CredentialStoreError::new(CredentialStoreErrorKind::NotFound))
        }
    }

    fn app_state(snapshot: QuotaSnapshot) -> AppState {
        let mut providers = ProviderStates::default();
        providers.codex.last_attempt_at = Some(snapshot.captured_at.clone());
        providers.codex.latest_snapshot = Some(ProviderSnapshot::Codex(snapshot.clone()));
        providers.codex.health = ProviderHealth::Fresh;
        AppState {
            version: STATE_VERSION,
            revision: 1,
            providers,
            latest_snapshot: Some(snapshot),
            storage_status: StorageStatus::Ready,
            storage_path: None,
            backup_path: None,
            status_message: "已通过 Codex CLI 更新额度。".to_string(),
            history: Vec::new(),
            settings: AppSettings::default(),
            recovery_notice: None,
        }
    }

    fn refresh_result(snapshot: QuotaSnapshot) -> RefreshUsageResult {
        RefreshUsageResult {
            app_state: app_state(snapshot),
            updated: true,
            message: "已通过 Codex CLI 更新额度。".to_string(),
        }
    }

    fn snapshot(weekly_percent: u8, reset_countdown_seconds: Option<i64>) -> QuotaSnapshot {
        QuotaSnapshot {
            id: "snap-1".to_string(),
            source: SnapshotSource::CodexCli,
            captured_at: "unix:1000".to_string(),
            weekly: QuotaReading {
                remaining_percent: Some(weekly_percent),
                reset_at: None,
                reset_countdown_seconds,
            },
            plan_type: None,
            credits_balance: None,
            reset_credits_available: None,
            raw_text: String::new(),
            status_message: "已通过 Codex CLI 更新额度。".to_string(),
            warnings: Vec::new(),
        }
    }

    fn provider_snapshot(provider_id: ProviderId, captured_at: &str) -> ProviderSnapshot {
        match provider_id {
            ProviderId::Codex => {
                let mut value = snapshot(61, None);
                value.id = format!("codex-{captured_at}");
                value.captured_at = captured_at.to_string();
                ProviderSnapshot::Codex(value)
            }
            ProviderId::DeepSeek => ProviderSnapshot::DeepSeek(DeepSeekSnapshot {
                id: format!("deepseek-{captured_at}"),
                captured_at: captured_at.to_string(),
                is_available: true,
                balances: vec![DeepSeekBalance {
                    currency: "CNY".to_string(),
                    total_balance: "110.00".to_string(),
                    granted_balance: "10.00".to_string(),
                    topped_up_balance: "100.00".to_string(),
                }],
            }),
            ProviderId::Kimi => ProviderSnapshot::Kimi(KimiSnapshot {
                id: format!("kimi-{captured_at}"),
                captured_at: captured_at.to_string(),
                region: KimiRegion::China,
                currency: "CNY".to_string(),
                available_balance: "49.59".to_string(),
                cash_balance: "3.00".to_string(),
                voucher_balance: "46.59".to_string(),
            }),
        }
    }

    fn successful_fetcher(captured_at: &str) -> FakeFetcher {
        FakeFetcher {
            outcomes: [
                (
                    ProviderId::Codex,
                    ProviderFetchOutcome::Updated(provider_snapshot(
                        ProviderId::Codex,
                        captured_at,
                    )),
                ),
                (
                    ProviderId::DeepSeek,
                    ProviderFetchOutcome::Updated(provider_snapshot(
                        ProviderId::DeepSeek,
                        captured_at,
                    )),
                ),
                (
                    ProviderId::Kimi,
                    ProviderFetchOutcome::Updated(provider_snapshot(ProviderId::Kimi, captured_at)),
                ),
            ]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn detects_windows_cmd_shims() {
        assert!(super::is_cmd_shim(std::path::Path::new("codex.cmd")));
        assert!(super::is_cmd_shim(std::path::Path::new("codex.bat")));
        assert!(!super::is_cmd_shim(std::path::Path::new("codex.exe")));
    }

    #[test]
    fn recognizes_interactive_status_output() {
        let output = "Weekly limit: [======] 59% left (resets 07:00 on 25 Jun)";

        assert!(super::codex_status_output_ready(output));
    }

    #[test]
    fn recognizes_weekly_only_interactive_status_output() {
        let output = "Weekly limit: [===================░] 93% left\n                              (resets 13:21 on 22 Jul)\nGPT-5.3-Codex-Spark Weekly limit: [====================] 100% left";

        assert!(super::codex_status_output_ready(output));
    }

    #[test]
    fn waits_for_status_output_to_settle_before_accepting_it() {
        let partial = "Weekly limit: [======] 44% left (resets 22:04)";

        assert!(!super::status_output_ready_after_settle(
            partial,
            STATUS_OUTPUT_SETTLE_DELAY - Duration::from_millis(1),
        ));
        assert!(super::status_output_ready_after_settle(
            partial,
            STATUS_OUTPUT_SETTLE_DELAY,
        ));
    }

    #[test]
    fn waits_for_the_codex_prompt_before_typing_status() {
        let started = std::time::Instant::now() - Duration::from_secs(15);

        assert!(!super::should_send_status_command(
            "Update available!\nPress enter to continue",
            started,
            started,
            0,
            false,
        ));
        assert!(super::should_send_status_command(
            "model: gpt-5\ndirectory: ~\n› Use /skills to list available skills",
            started,
            started,
            0,
            false,
        ));
        assert!(super::codex_update_prompt_visible(
            "Update available!\nPress enter to continue"
        ));
    }

    #[test]
    fn auto_refresh_keeps_base_interval_for_healthy_usage() {
        let state = app_state(snapshot(64, Some(3600)));

        assert_eq!(
            adaptive_refresh_interval(&state),
            AUTO_BASE_REFRESH_INTERVAL
        );
    }

    #[test]
    fn auto_refresh_accelerates_when_usage_is_low() {
        let outcome = Ok(refresh_result(snapshot(20, Some(3600))));

        let schedule = next_auto_refresh_schedule(&outcome, 2);

        assert_eq!(schedule.delay, AUTO_LOW_USAGE_REFRESH_INTERVAL);
        assert_eq!(schedule.consecutive_failures, 0);
    }

    #[test]
    fn auto_refresh_schedules_after_imminent_reset() {
        let reset_in = Duration::from_secs(42);
        let state = app_state(snapshot(64, Some(reset_in.as_secs() as i64)));

        assert_eq!(
            adaptive_refresh_interval(&state),
            reset_in + AUTO_POST_RESET_REFRESH_DELAY
        );
    }

    #[test]
    fn auto_refresh_ignores_distant_reset_countdown() {
        let reset_after_watch_window = AUTO_RESET_WATCH_WINDOW + Duration::from_secs(1);
        let state = app_state(snapshot(
            64,
            Some(reset_after_watch_window.as_secs() as i64),
        ));

        assert_eq!(
            adaptive_refresh_interval(&state),
            AUTO_BASE_REFRESH_INTERVAL
        );
    }

    #[test]
    fn auto_refresh_uses_structured_absolute_reset_time() {
        let mut value = snapshot(64, None);
        value.weekly.reset_at = Some("unix:1042".to_string());

        assert_eq!(
            super::imminent_reset_refresh_interval_at(&value, 1000),
            Some(Duration::from_secs(42) + AUTO_POST_RESET_REFRESH_DELAY)
        );
    }

    #[test]
    fn auto_refresh_ignores_long_expired_absolute_reset_time() {
        let mut value = snapshot(64, None);
        value.weekly.reset_at = Some("unix:1000".to_string());

        assert_eq!(
            super::imminent_reset_refresh_interval_at(&value, 2000),
            None
        );
    }

    #[test]
    fn low_quota_notification_reports_the_weekly_threshold_crossing() {
        let previous = snapshot(21, None);
        let current = snapshot(20, None);

        assert_eq!(
            super::low_quota_notification(Some(&previous), &current).as_deref(),
            Some("1 周剩余 20%")
        );
        assert!(super::low_quota_notification(Some(&current), &current).is_none());
    }

    #[test]
    fn auto_refresh_uses_failure_backoff_for_unsuccessful_results() {
        let mut state = app_state(snapshot(64, Some(30)));
        state.status_message = "Codex CLI 额度查询失败，请稍后重试。".to_string();
        let outcome = Ok(RefreshUsageResult {
            app_state: state,
            updated: false,
            message: "Codex CLI 额度查询失败，请稍后重试。".to_string(),
        });

        let schedule = next_auto_refresh_schedule(&outcome, 1);

        assert_eq!(schedule.delay, Duration::from_secs(10 * 60));
        assert_eq!(schedule.consecutive_failures, 2);
    }

    #[test]
    fn failure_backoff_caps_at_thirty_minutes() {
        assert_eq!(failure_backoff_interval(1), Duration::from_secs(5 * 60));
        assert_eq!(failure_backoff_interval(2), Duration::from_secs(10 * 60));
        assert_eq!(failure_backoff_interval(3), Duration::from_secs(20 * 60));
        assert_eq!(failure_backoff_interval(4), Duration::from_secs(30 * 60));
        assert_eq!(failure_backoff_interval(8), Duration::from_secs(30 * 60));
    }

    #[test]
    fn refresh_all_commits_three_successes_in_one_state() {
        let dir = TestDir::new("three-successes");
        let store = UsageStore::new(dir.path().join("state.json"));
        let coordinator = RefreshCoordinator::default();

        let result = refresh_providers_blocking(
            &store,
            &coordinator,
            &successful_fetcher("unix:2000"),
            &[ProviderId::Codex, ProviderId::DeepSeek, ProviderId::Kimi],
            "unix:2001".to_string(),
        )
        .unwrap();

        assert!(result.any_updated);
        assert_eq!(result.provider_results.len(), 3);
        assert!(result
            .provider_results
            .iter()
            .all(|item| item.outcome == ProviderRefreshOutcome::Updated));
        for provider_id in [ProviderId::Codex, ProviderId::DeepSeek, ProviderId::Kimi] {
            let provider = result.app_state.providers.get(provider_id);
            assert_eq!(provider.health, ProviderHealth::Fresh);
            assert_eq!(provider.last_attempt_at.as_deref(), Some("unix:2001"));
            assert!(provider.latest_snapshot.is_some());
        }
    }

    #[test]
    fn partial_success_keeps_the_failed_provider_last_snapshot() {
        let dir = TestDir::new("partial-success");
        let store = UsageStore::new(dir.path().join("state.json"));
        let coordinator = RefreshCoordinator::default();
        refresh_providers_blocking(
            &store,
            &coordinator,
            &successful_fetcher("unix:old"),
            &[ProviderId::Codex, ProviderId::DeepSeek, ProviderId::Kimi],
            "unix:old-attempt".to_string(),
        )
        .unwrap();
        let fetcher = FakeFetcher {
            outcomes: [
                (
                    ProviderId::Codex,
                    ProviderFetchOutcome::Updated(provider_snapshot(ProviderId::Codex, "unix:new")),
                ),
                (
                    ProviderId::DeepSeek,
                    ProviderFetchOutcome::Failed {
                        failure: ProviderFailure::new(
                            ProviderErrorCategory::Network,
                            "无法连接 DeepSeek。",
                        ),
                        configured: Some(true),
                    },
                ),
                (
                    ProviderId::Kimi,
                    ProviderFetchOutcome::Updated(provider_snapshot(ProviderId::Kimi, "unix:new")),
                ),
            ]
            .into_iter()
            .collect(),
        };

        let result = refresh_providers_blocking(
            &store,
            &coordinator,
            &fetcher,
            &[ProviderId::Codex, ProviderId::DeepSeek, ProviderId::Kimi],
            "unix:new-attempt".to_string(),
        )
        .unwrap();

        assert!(result.any_updated);
        assert!(result.message.contains("部分"));
        let deepseek = &result.app_state.providers.deepseek;
        assert_eq!(deepseek.health, ProviderHealth::Stale);
        assert_eq!(
            deepseek.error_category,
            Some(ProviderErrorCategory::Network)
        );
        assert_eq!(
            deepseek
                .latest_snapshot
                .as_ref()
                .map(ProviderSnapshot::captured_at),
            Some("unix:old")
        );
        assert_eq!(
            deepseek.last_attempt_at.as_deref(),
            Some("unix:new-attempt")
        );
    }

    #[test]
    fn unconfigured_providers_are_skipped_without_failure_state() {
        let dir = TestDir::new("unconfigured");
        let store = UsageStore::new(dir.path().join("state.json"));
        let coordinator = RefreshCoordinator::default();
        let fetcher = FakeFetcher {
            outcomes: [
                (
                    ProviderId::Codex,
                    ProviderFetchOutcome::Updated(provider_snapshot(
                        ProviderId::Codex,
                        "unix:3000",
                    )),
                ),
                (ProviderId::DeepSeek, ProviderFetchOutcome::NotConfigured),
                (ProviderId::Kimi, ProviderFetchOutcome::NotConfigured),
            ]
            .into_iter()
            .collect(),
        };

        let result = refresh_providers_blocking(
            &store,
            &coordinator,
            &fetcher,
            &[ProviderId::Codex, ProviderId::DeepSeek, ProviderId::Kimi],
            "unix:3001".to_string(),
        )
        .unwrap();

        assert_eq!(
            result.provider_results[1].outcome,
            ProviderRefreshOutcome::Skipped
        );
        assert_eq!(
            result.app_state.providers.deepseek.health,
            ProviderHealth::NotConfigured
        );
        assert!(result.app_state.providers.deepseek.error_category.is_none());
        assert!(result
            .app_state
            .providers
            .deepseek
            .last_attempt_at
            .is_none());
    }

    #[test]
    fn refresh_all_starts_slow_codex_and_network_providers_concurrently() {
        let dir = TestDir::new("parallel");
        let store = UsageStore::new(dir.path().join("state.json"));
        let coordinator = RefreshCoordinator::default();
        let fetcher = ConcurrentFetcher::new();

        refresh_providers_blocking(
            &store,
            &coordinator,
            &fetcher,
            &[ProviderId::Codex, ProviderId::DeepSeek, ProviderId::Kimi],
            "unix:parallel-attempt".to_string(),
        )
        .unwrap();

        assert_eq!(fetcher.max_concurrency(), 3);
    }

    #[test]
    fn provider_completions_are_persisted_and_reported_immediately_in_completion_order() {
        let dir = TestDir::new("completion-order");
        let store = UsageStore::new(dir.path().join("state.json"));
        let coordinator = RefreshCoordinator::default();
        let mut events = Vec::new();

        let result = refresh_providers_blocking_with_callback(
            &store,
            &coordinator,
            &OrderedFetcher,
            &[ProviderId::Codex, ProviderId::DeepSeek, ProviderId::Kimi],
            "unix:ordered-attempt".to_string(),
            |event| {
                let persisted = store.load().unwrap().into_app_state();
                assert_eq!(persisted.revision, event.app_state.revision);
                assert_eq!(persisted.providers, event.app_state.providers);
                events.push((
                    event.provider_results[0].provider_id,
                    event.app_state.revision,
                    event.app_state.providers.codex.latest_snapshot.is_some(),
                ));
            },
        )
        .unwrap();

        assert_eq!(
            events,
            vec![
                (ProviderId::DeepSeek, 1, false),
                (ProviderId::Kimi, 2, false),
                (ProviderId::Codex, 3, true),
            ]
        );
        assert_eq!(result.app_state.revision, 3);
        assert_eq!(
            result
                .provider_results
                .iter()
                .map(|item| item.provider_id)
                .collect::<Vec<_>>(),
            PROVIDER_ORDER
        );
    }

    #[test]
    fn provider_permit_blocks_credential_mutation_during_an_in_flight_fetch() {
        let dir = TestDir::new("credential-race");
        let store = UsageStore::new(dir.path().join("state.json"));
        let coordinator = RefreshCoordinator::default();
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let fetcher = BlockingFetcher {
            entered: entered_tx,
            release: (Mutex::new(false), Condvar::new()),
        };

        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                refresh_providers_blocking(
                    &store,
                    &coordinator,
                    &fetcher,
                    &[ProviderId::DeepSeek],
                    "unix:race".to_string(),
                )
            });
            entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert!(coordinator.try_begin(ProviderId::DeepSeek).is_none());
            {
                let mut released = fetcher.release.0.lock().unwrap();
                *released = true;
                fetcher.release.1.notify_all();
            }
            worker.join().unwrap().unwrap();
        });

        assert!(coordinator.try_begin(ProviderId::DeepSeek).is_some());
    }

    #[test]
    fn a_busy_provider_does_not_block_other_provider_refreshes() {
        let dir = TestDir::new("independent-locks");
        let store = UsageStore::new(dir.path().join("state.json"));
        let coordinator = RefreshCoordinator::default();
        let codex_permit = coordinator.try_begin(ProviderId::Codex).unwrap();

        let result = refresh_providers_blocking(
            &store,
            &coordinator,
            &successful_fetcher("unix:4000"),
            &[ProviderId::Codex, ProviderId::DeepSeek, ProviderId::Kimi],
            "unix:4001".to_string(),
        )
        .unwrap();

        assert_eq!(
            result.provider_results[0].outcome,
            ProviderRefreshOutcome::Skipped
        );
        assert_eq!(
            result.provider_results[1].outcome,
            ProviderRefreshOutcome::Updated
        );
        assert_eq!(
            result.provider_results[2].outcome,
            ProviderRefreshOutcome::Updated
        );
        assert!(result.message.contains("部分供应商已更新"));
        assert!(result.message.contains("查询正在进行中"));
        assert!(!result.message.contains("所有已配置供应商"));
        assert!(result.app_state.providers.codex.latest_snapshot.is_none());
        drop(codex_permit);
        assert!(coordinator.try_begin(ProviderId::Codex).is_some());
    }

    #[test]
    fn provider_backoff_is_independent_and_codex_adaptive_rules_do_not_touch_balances() {
        let state = app_state(snapshot(20, Some(3600)));
        let failed = ProviderRefreshResult {
            provider_id: ProviderId::DeepSeek,
            outcome: ProviderRefreshOutcome::Failed,
            message: "failed".to_string(),
            error_category: Some(ProviderErrorCategory::Network),
        };
        let codex_updated = ProviderRefreshResult {
            provider_id: ProviderId::Codex,
            outcome: ProviderRefreshOutcome::Updated,
            message: "updated".to_string(),
            error_category: None,
        };
        let deepseek_updated = ProviderRefreshResult {
            provider_id: ProviderId::DeepSeek,
            outcome: ProviderRefreshOutcome::Updated,
            message: "updated".to_string(),
            error_category: None,
        };
        let skipped = ProviderRefreshResult {
            provider_id: ProviderId::Kimi,
            outcome: ProviderRefreshOutcome::Skipped,
            message: "not configured".to_string(),
            error_category: Some(ProviderErrorCategory::NotConfigured),
        };
        let busy = ProviderRefreshResult {
            provider_id: ProviderId::Kimi,
            outcome: ProviderRefreshOutcome::Skipped,
            message: "busy".to_string(),
            error_category: Some(ProviderErrorCategory::Busy),
        };

        assert_eq!(
            next_provider_auto_schedule(ProviderId::DeepSeek, &failed, &state, 1),
            (Duration::from_secs(10 * 60), 2)
        );
        assert_eq!(
            next_provider_auto_schedule(ProviderId::Codex, &codex_updated, &state, 3),
            (AUTO_LOW_USAGE_REFRESH_INTERVAL, 0)
        );
        assert_eq!(
            next_provider_auto_schedule(ProviderId::DeepSeek, &deepseek_updated, &state, 3),
            (AUTO_BASE_REFRESH_INTERVAL, 0)
        );
        assert_eq!(
            next_provider_auto_schedule(ProviderId::Kimi, &skipped, &state, 3),
            (AUTO_BASE_REFRESH_INTERVAL, 0)
        );
        assert_eq!(
            next_provider_auto_schedule(ProviderId::Kimi, &busy, &state, 3),
            (AUTO_BUSY_RETRY_INTERVAL, 3)
        );
    }

    #[test]
    fn configured_network_failure_records_configuration_and_preserves_error_state() {
        let dir = TestDir::new("configured-failure");
        let store = UsageStore::new(dir.path().join("state.json"));
        let coordinator = RefreshCoordinator::default();
        let fetcher = FakeFetcher {
            outcomes: [(
                ProviderId::DeepSeek,
                ProviderFetchOutcome::Failed {
                    failure: ProviderFailure::new(
                        ProviderErrorCategory::Network,
                        "DeepSeek network failed",
                    ),
                    configured: Some(true),
                },
            )]
            .into_iter()
            .collect(),
        };

        let result = refresh_providers_blocking(
            &store,
            &coordinator,
            &fetcher,
            &[ProviderId::DeepSeek],
            "unix:failed".to_string(),
        )
        .unwrap();

        assert!(result.app_state.providers.deepseek.configured);
        assert_eq!(
            result.app_state.providers.deepseek.health,
            ProviderHealth::Error
        );
        assert_eq!(
            result.app_state.providers.deepseek.error_category,
            Some(ProviderErrorCategory::Network)
        );
    }

    #[test]
    fn credential_store_failure_does_not_invent_an_unconfigured_fact() {
        let dir = TestDir::new("credential-store-failure");
        let store = UsageStore::new(dir.path().join("state.json"));
        let coordinator = RefreshCoordinator::default();
        let fetcher = FakeFetcher {
            outcomes: [(
                ProviderId::DeepSeek,
                ProviderFetchOutcome::Failed {
                    failure: ProviderFailure::new(
                        ProviderErrorCategory::CredentialStore,
                        "credential store unavailable",
                    ),
                    configured: None,
                },
            )]
            .into_iter()
            .collect(),
        };

        let result = refresh_providers_blocking(
            &store,
            &coordinator,
            &fetcher,
            &[ProviderId::DeepSeek],
            "unix:credential-failed".to_string(),
        )
        .unwrap();

        assert!(!result.app_state.providers.deepseek.configured);
        assert_eq!(
            result.app_state.providers.deepseek.health,
            ProviderHealth::Error
        );
        assert_eq!(
            result.app_state.providers.deepseek.error_category,
            Some(ProviderErrorCategory::CredentialStore)
        );
    }

    #[test]
    fn legacy_refresh_projection_only_uses_the_codex_outcome() {
        let mut state = app_state(snapshot(50, None));
        state.revision = 8;
        let deepseek_only = legacy_codex_result(RefreshProvidersResult {
            app_state: state.clone(),
            provider_results: vec![ProviderRefreshResult {
                provider_id: ProviderId::DeepSeek,
                outcome: ProviderRefreshOutcome::Updated,
                message: "DeepSeek updated".to_string(),
                error_category: None,
            }],
            any_updated: true,
            message: "updated".to_string(),
        });
        assert!(!deepseek_only.updated);
        assert_eq!(deepseek_only.message, "Codex 查询未执行。");

        let codex_failure = legacy_codex_result(RefreshProvidersResult {
            app_state: state,
            provider_results: vec![
                ProviderRefreshResult {
                    provider_id: ProviderId::Codex,
                    outcome: ProviderRefreshOutcome::Failed,
                    message: "Codex failed".to_string(),
                    error_category: Some(ProviderErrorCategory::InvalidResponse),
                },
                ProviderRefreshResult {
                    provider_id: ProviderId::DeepSeek,
                    outcome: ProviderRefreshOutcome::Updated,
                    message: "DeepSeek updated".to_string(),
                    error_category: None,
                },
            ],
            any_updated: true,
            message: "partial".to_string(),
        });
        assert!(!codex_failure.updated);
        assert_eq!(codex_failure.message, "Codex failed");
    }

    #[test]
    fn setting_configured_with_an_old_snapshot_marks_it_stale() {
        let dir = TestDir::new("configured-stale");
        let usage_store = UsageStore::new(dir.path().join("state.json"));
        let credential_store = MemoryCredentialStore::default();
        let coordinator = RefreshCoordinator::default();
        refresh_providers_blocking(
            &usage_store,
            &coordinator,
            &successful_fetcher("unix:old"),
            &[ProviderId::DeepSeek],
            "unix:old".to_string(),
        )
        .unwrap();
        usage_store
            .set_provider_configured(ProviderId::DeepSeek, false)
            .unwrap();

        let (_, state) = set_provider_credential_and_sync(
            &credential_store,
            &usage_store,
            ProviderId::DeepSeek,
            None,
            "test-secret",
        )
        .unwrap();

        assert!(state.providers.deepseek.configured);
        assert!(state.providers.deepseek.latest_snapshot.is_some());
        assert_eq!(state.providers.deepseek.health, ProviderHealth::Stale);
    }

    #[test]
    fn credential_set_rolls_back_new_and_existing_secrets_when_state_save_fails() {
        let dir = TestDir::new("credential-set-rollback");
        let invalid_parent = dir.path().join("parent-file");
        std::fs::write(&invalid_parent, b"not a directory").unwrap();
        let usage_store = UsageStore::new(invalid_parent.join("state.json"));
        let credential_store = MemoryCredentialStore::default();

        let error = set_provider_credential_and_sync(
            &credential_store,
            &usage_store,
            ProviderId::DeepSeek,
            None,
            "new-secret",
        )
        .unwrap_err();
        assert_eq!(error, "状态保存失败，凭据更改已回滚。");
        assert!(credential_store.entries.lock().unwrap().is_empty());

        crate::credentials::set_provider_credential_with_store(
            &credential_store,
            ProviderId::DeepSeek,
            None,
            "old-secret",
        )
        .unwrap();
        let error = set_provider_credential_and_sync(
            &credential_store,
            &usage_store,
            ProviderId::DeepSeek,
            None,
            "replacement-secret",
        )
        .unwrap_err();
        assert_eq!(error, "状态保存失败，凭据更改已回滚。");
        assert_eq!(
            crate::credentials::load_provider_credential(
                &credential_store,
                ProviderId::DeepSeek,
                None,
            )
            .unwrap(),
            "old-secret"
        );
    }

    #[test]
    fn credential_delete_restores_the_secret_when_state_save_fails() {
        let dir = TestDir::new("credential-delete-rollback");
        let invalid_parent = dir.path().join("parent-file");
        std::fs::write(&invalid_parent, b"not a directory").unwrap();
        let usage_store = UsageStore::new(invalid_parent.join("state.json"));
        let credential_store = MemoryCredentialStore::default();
        crate::credentials::set_provider_credential_with_store(
            &credential_store,
            ProviderId::DeepSeek,
            None,
            "old-secret",
        )
        .unwrap();

        let error = delete_provider_credential_and_sync(
            &credential_store,
            &usage_store,
            ProviderId::DeepSeek,
            None,
        )
        .unwrap_err();

        assert_eq!(error, "状态保存失败，凭据删除已回滚。");
        assert_eq!(
            crate::credentials::load_provider_credential(
                &credential_store,
                ProviderId::DeepSeek,
                None,
            )
            .unwrap(),
            "old-secret"
        );
    }

    #[test]
    fn deleting_a_credential_updates_configuration_and_floating_selection() {
        let dir = TestDir::new("credential-delete");
        let usage_store = UsageStore::new(dir.path().join("state.json"));
        let credential_store = MemoryCredentialStore::default();
        crate::credentials::set_provider_credential_with_store(
            &credential_store,
            ProviderId::DeepSeek,
            None,
            "test-secret",
        )
        .unwrap();
        usage_store
            .set_provider_configured(ProviderId::DeepSeek, true)
            .unwrap();
        usage_store
            .update_settings(crate::models::SettingsPatch {
                automatic_update_checks: None,
                low_quota_notifications: None,
                floating_provider_ids: Some(vec![ProviderId::DeepSeek]),
            })
            .unwrap();

        let (_, state) = delete_provider_credential_and_sync(
            &credential_store,
            &usage_store,
            ProviderId::DeepSeek,
            None,
        )
        .unwrap();

        assert!(!state.providers.deepseek.configured);
        assert_eq!(
            state.providers.deepseek.health,
            ProviderHealth::NotConfigured
        );
        assert_eq!(state.settings.floating_provider_ids, [ProviderId::Codex]);
        assert!(credential_store.entries.lock().unwrap().is_empty());
    }

    #[test]
    #[ignore]
    fn captures_real_codex_status_with_pty() {
        let output = super::run_codex_status_pty(std::time::Duration::from_secs(20)).unwrap();

        assert!(super::codex_status_output_ready(&output));
    }

    #[test]
    #[ignore]
    fn queries_real_codex_app_server_rate_limits() {
        let command = super::codex_command(&["app-server"]).unwrap();
        let snapshot = crate::app_server::fetch_rate_limits(
            command,
            std::time::Duration::from_secs(15),
            crate::version::APP_VERSION,
        )
        .unwrap();

        assert_eq!(snapshot.source, SnapshotSource::CodexAppServer);
        assert!(snapshot.has_usage());
    }
}
