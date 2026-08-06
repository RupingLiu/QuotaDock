use crate::version::{APP_NAME, APP_VERSION};
use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{App, AppHandle, Emitter, Manager, Url};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};

const AUTO_FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const UPDATE_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const UPDATE_READ_TIMEOUT: Duration = Duration::from_secs(60);
const UPDATE_STATUS_EVENT: &str = "quotadock:update-status";
const UPDATE_CACHE_DIR: &str = "pending-update";
const UPDATE_CACHE_FILE: &str = "verified-update.cache";
const LEGACY_UPDATE_CACHE_PACKAGE: &str = "package.bin";
const LEGACY_UPDATE_CACHE_METADATA: &str = "metadata.json";
const UPDATE_CACHE_MAGIC: &[u8] = b"QuotaDockUpdateCacheV1\0";
const UPDATE_TRUST_STATE_FILE: &str = "update-trust-state.json";
const MAX_NOTIFIED_VERSIONS: usize = 32;
const MAX_TRUST_STATE_BYTES: u64 = 8 * 1024;
const MAX_CACHE_METADATA_BYTES: u64 = 64 * 1024;
const MAX_CACHE_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CACHE_FILE_BYTES: u64 = MAX_CACHE_PACKAGE_BYTES + MAX_CACHE_METADATA_BYTES + 128;
static UPDATE_CHECK_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdatePhase {
    Idle,
    Checking,
    UpToDate,
    Downloading,
    Ready,
    Installing,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub phase: UpdatePhase,
    pub message: String,
    pub technical_detail: Option<String>,
    pub available_version: Option<String>,
    pub progress_percent: Option<u8>,
    pub checked_at: Option<String>,
}

impl UpdateStatus {
    fn idle() -> Self {
        Self {
            current_version: APP_VERSION.to_string(),
            phase: UpdatePhase::Idle,
            message: "尚未检查软件更新。".to_string(),
            technical_detail: None,
            available_version: None,
            progress_percent: None,
            checked_at: None,
        }
    }

    fn checking() -> Self {
        Self {
            phase: UpdatePhase::Checking,
            message: "正在连接更新服务…".to_string(),
            checked_at: Some(now_marker()),
            ..Self::idle()
        }
    }

    fn up_to_date() -> Self {
        Self {
            phase: UpdatePhase::UpToDate,
            message: format!("已是最新版本 v{APP_VERSION}。"),
            checked_at: Some(now_marker()),
            ..Self::idle()
        }
    }

    fn downloading(version: &str, progress_percent: Option<u8>) -> Self {
        let message = progress_percent.map_or_else(
            || format!("正在下载签名更新 v{version}…"),
            |progress| format!("正在下载签名更新 v{version}（{progress}%）…"),
        );
        Self {
            phase: UpdatePhase::Downloading,
            message,
            available_version: Some(version.to_string()),
            progress_percent,
            checked_at: Some(now_marker()),
            ..Self::idle()
        }
    }

    fn verifying(version: &str) -> Self {
        Self {
            phase: UpdatePhase::Downloading,
            message: format!("下载完成，正在验证 v{version} 的发布签名…"),
            available_version: Some(version.to_string()),
            progress_percent: Some(100),
            checked_at: Some(now_marker()),
            ..Self::idle()
        }
    }

    fn ready(version: &str) -> Self {
        Self {
            phase: UpdatePhase::Ready,
            message: format!("更新 v{version} 已下载并通过发布签名验证，可随时安装。"),
            available_version: Some(version.to_string()),
            progress_percent: Some(100),
            checked_at: Some(now_marker()),
            ..Self::idle()
        }
    }

    fn ready_after_check_failure(version: &str, technical_detail: impl Into<String>) -> Self {
        Self {
            message: format!("更新 v{version} 仍已准备；本次后台复查失败，不影响安装。"),
            technical_detail: Some(technical_detail.into()),
            ..Self::ready(version)
        }
    }

    fn installing(version: &str) -> Self {
        Self {
            phase: UpdatePhase::Installing,
            message: format!("正在安装 v{version}，应用即将重新启动…"),
            available_version: Some(version.to_string()),
            progress_percent: Some(100),
            checked_at: Some(now_marker()),
            ..Self::idle()
        }
    }

    fn error(message: impl Into<String>, technical_detail: impl Into<String>) -> Self {
        Self {
            phase: UpdatePhase::Error,
            message: message.into(),
            technical_detail: Some(technical_detail.into()),
            checked_at: Some(now_marker()),
            ..Self::idle()
        }
    }
}

pub struct UpdateStatusState(Mutex<UpdateStatus>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedUpdateMetadata {
    version: String,
    signature: String,
    download_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateTrustState {
    #[serde(default)]
    notified_versions: Vec<String>,
    #[serde(default)]
    highest_trusted_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct NumericReleaseVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

struct PreparedUpdate {
    update: Update,
    bytes: Vec<u8>,
    metadata: CachedUpdateMetadata,
}

pub struct PreparedUpdateState(Mutex<Option<PreparedUpdate>>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckOrigin {
    Manual,
    Automatic,
}

#[derive(Debug)]
struct UpdateCheckPermit;

impl Drop for UpdateCheckPermit {
    fn drop(&mut self) {
        UPDATE_CHECK_RUNNING.store(false, Ordering::Release);
    }
}

#[derive(Debug)]
struct UpdateFailure {
    message: String,
    technical_detail: String,
}

impl UpdateFailure {
    fn new(message: impl Into<String>, technical_detail: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            technical_detail: technical_detail.into(),
        }
    }

    fn into_status(self) -> UpdateStatus {
        UpdateStatus::error(self.message, self.technical_detail)
    }
}

pub fn install(app: &App) {
    app.manage(UpdateStatusState(Mutex::new(UpdateStatus::idle())));
    app.manage(PreparedUpdateState(Mutex::new(None)));
}

pub fn start_auto_check(app: AppHandle) {
    let _ = thread::Builder::new()
        .name("quotadock-auto-update-check".to_string())
        .spawn(move || {
            thread::sleep(AUTO_FIRST_CHECK_DELAY);
            loop {
                if automatic_checks_enabled(&app) {
                    let check_app = app.clone();
                    tauri::async_runtime::block_on(async move {
                        let status = execute_check(check_app, CheckOrigin::Automatic).await;
                        if status.phase == UpdatePhase::Error {
                            eprintln!("automatic update check failed: {}", status.message);
                        }
                    });
                }
                thread::sleep(AUTO_CHECK_INTERVAL);
            }
        });
}

pub fn check_now(app: AppHandle) {
    #[cfg(feature = "desktop")]
    if let Err(error) = crate::details::show(&app) {
        crate::tray::set_menu_status_temporarily(&app, error);
    }
    #[cfg(feature = "desktop")]
    crate::tray::set_menu_status(&app, "正在检查更新…");

    tauri::async_runtime::spawn(async move {
        let status = execute_check(app.clone(), CheckOrigin::Manual).await;

        #[cfg(feature = "desktop")]
        crate::tray::set_menu_status_temporarily(&app, menu_status_for(&status));
    });
}

#[tauri::command]
pub fn get_update_status(app: AppHandle) -> UpdateStatus {
    current_update_status(&app)
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> UpdateStatus {
    execute_check(app, CheckOrigin::Manual).await
}

#[tauri::command]
pub async fn install_downloaded_update(app: AppHandle) -> UpdateStatus {
    let _permit = match begin_update_check(CheckOrigin::Manual) {
        Ok(permit) => permit,
        Err(_) => return current_update_status(&app),
    };
    let Some(prepared) = take_prepared_update(&app) else {
        let status = UpdateStatus::error(
            "没有已准备好的更新，请先检查更新。",
            "安装请求被拒绝：内存中不存在已验签的更新包。",
        );
        set_update_status(&app, status.clone());
        return status;
    };

    let version = prepared.metadata.version.clone();
    let verification = (|| {
        if !prepared.metadata.matches(&prepared.update) {
            return Err("内存更新包与清单身份不一致。".to_string());
        }
        reject_trusted_version_rollback(&app, &version)?;
        let public_key = updater_public_key(&app)?;
        verify_package_identity(
            &prepared.bytes,
            &prepared.update.signature,
            &public_key,
            &prepared.metadata.version,
        )
    })();
    if let Err(error) = verification {
        clear_update_cache(&app);
        let status = UpdateStatus::error("更新包安装前安全校验失败，请重新检查更新。", error);
        set_update_status(&app, status.clone());
        #[cfg(feature = "desktop")]
        crate::tray::refresh_menu(&app);
        return status;
    }

    set_update_status(&app, UpdateStatus::installing(&version));
    #[cfg(feature = "desktop")]
    crate::tray::refresh_menu(&app);

    if let Err(error) = prepared.update.install(&prepared.bytes) {
        restore_prepared_update(&app, prepared);
        let status = UpdateStatus::error(
            "更新安装启动失败，已保留通过验证的安装包。",
            format!("启动更新安装程序失败：{error}"),
        );
        set_update_status(&app, status.clone());
        #[cfg(feature = "desktop")]
        crate::tray::refresh_menu(&app);
        return status;
    }

    clear_update_cache(&app);
    app.restart();
}

pub fn is_check_running() -> bool {
    UPDATE_CHECK_RUNNING.load(Ordering::Acquire)
}

async fn execute_check(app: AppHandle, origin: CheckOrigin) -> UpdateStatus {
    let status = match check_and_prepare_update(app.clone(), origin).await {
        Ok(status) => status,
        Err(failure) => {
            let status = if let Some(version) = prepared_version(&app) {
                eprintln!(
                    "update recheck failed while a verified package is ready: {}",
                    failure.message
                );
                UpdateStatus::ready_after_check_failure(&version, failure.technical_detail)
            } else {
                failure.into_status()
            };
            set_update_status(&app, status.clone());
            status
        }
    };
    #[cfg(feature = "desktop")]
    crate::tray::refresh_menu(&app);
    status
}

async fn check_and_prepare_update(
    app: AppHandle,
    origin: CheckOrigin,
) -> Result<UpdateStatus, UpdateFailure> {
    let _permit = match begin_update_check(origin) {
        Ok(permit) => permit,
        Err(_) => {
            let current = current_update_status(&app);
            return Ok(current);
        }
    };
    set_update_status(&app, UpdateStatus::checking());
    #[cfg(feature = "desktop")]
    crate::tray::refresh_menu(&app);

    let updater = build_updater(&app).map_err(|error| {
        UpdateFailure::new(
            "更新服务初始化失败，请稍后重试。",
            format!("初始化签名更新器失败：{error}"),
        )
    })?;
    let Some(update) = updater.check().await.map_err(check_request_failure)? else {
        clear_prepared_update(&app);
        clear_update_cache(&app);
        let status = UpdateStatus::up_to_date();
        set_update_status(&app, status.clone());
        return Ok(status);
    };

    let version = update.version.to_string();
    reject_trusted_version_rollback(&app, &version).map_err(|error| {
        UpdateFailure::new(
            "更新服务返回了低于已验证版本的安装包，已停止自动处理。",
            error,
        )
    })?;
    if matching_prepared_update(&app, &update) {
        let status = UpdateStatus::ready(&version);
        set_update_status(&app, status.clone());
        finish_ready_notification(&app, origin, &version);
        return Ok(status);
    }
    match load_verified_cache(&app, &update) {
        Ok(Some((metadata, bytes))) => {
            clear_prepared_update(&app);
            record_highest_trusted_version(&app, &version).map_err(|error| {
                UpdateFailure::new(
                    "已验证更新包，但无法保存版本安全记录，请检查磁盘后重试。",
                    error,
                )
            })?;
            restore_prepared_update(
                &app,
                PreparedUpdate {
                    update,
                    bytes,
                    metadata,
                },
            );
            let status = UpdateStatus::ready(&version);
            set_update_status(&app, status.clone());
            finish_ready_notification(&app, origin, &version);
            return Ok(status);
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("discarding unusable update cache: {error}");
            clear_update_cache(&app);
        }
    }

    let downloading = UpdateStatus::downloading(&version, None);
    set_update_status(&app, downloading);
    #[cfg(feature = "desktop")]
    crate::tray::set_menu_status(&app, format!("正在下载更新 v{version}…"));

    let progress_app = app.clone();
    let progress_version = version.clone();
    let finish_app = app.clone();
    let finish_version = version.clone();
    let mut downloaded_bytes = 0_u64;
    let mut last_reported_percent = 0_u8;
    let bytes = update
        .download(
            move |chunk_length, total_length| {
                downloaded_bytes = downloaded_bytes.saturating_add(chunk_length as u64);
                let Some(total_length) = total_length.filter(|total| *total > 0) else {
                    return;
                };
                let percent =
                    ((downloaded_bytes.saturating_mul(100) / total_length).min(100)) as u8;
                if percent == 100 || percent >= last_reported_percent.saturating_add(5) {
                    last_reported_percent = percent;
                    set_update_status(
                        &progress_app,
                        UpdateStatus::downloading(&progress_version, Some(percent)),
                    );
                }
            },
            move || {
                set_update_status(&finish_app, UpdateStatus::verifying(&finish_version));
            },
        )
        .await
        .map_err(|error| {
            UpdateFailure::new(
                "更新下载或签名验证失败，未缓存安装包。",
                format!("下载或签名验证失败：{error}"),
            )
        })?;

    let metadata = CachedUpdateMetadata::from_update(&update);
    let public_key = updater_public_key(&app)
        .map_err(|error| UpdateFailure::new("更新包安全校验失败，未缓存安装包。", error))?;
    verify_package_identity(&bytes, &metadata.signature, &public_key, &metadata.version)
        .map_err(|error| UpdateFailure::new("更新包安全校验失败，未缓存安装包。", error))?;
    clear_prepared_update(&app);
    record_highest_trusted_version(&app, &version).map_err(|error| {
        UpdateFailure::new(
            "更新已下载并验证，但无法保存版本安全记录，请检查磁盘后重试。",
            error,
        )
    })?;
    write_update_cache_at(
        &update_cache_directory(&app).map_err(|error| {
            UpdateFailure::new("更新已下载并验证，但无法确定缓存位置，请稍后重试。", error)
        })?,
        &metadata,
        &bytes,
    )
    .map_err(|error| {
        UpdateFailure::new(
            "更新已下载并验证，但无法保存安装包，请检查磁盘空间后重试。",
            format!("保存已验签更新缓存失败：{error}"),
        )
    })?;
    restore_prepared_update(
        &app,
        PreparedUpdate {
            update,
            bytes,
            metadata,
        },
    );
    let status = UpdateStatus::ready(&version);
    set_update_status(&app, status.clone());
    finish_ready_notification(&app, origin, &version);
    Ok(status)
}

impl CachedUpdateMetadata {
    fn from_update(update: &Update) -> Self {
        Self {
            version: update.version.clone(),
            signature: update.signature.clone(),
            download_url: update.download_url.to_string(),
        }
    }

    fn matches(&self, update: &Update) -> bool {
        self.matches_release(
            &update.version,
            &update.signature,
            update.download_url.as_str(),
        )
    }

    fn matches_release(&self, version: &str, signature: &str, download_url: &str) -> bool {
        self.version == version && self.signature == signature && self.download_url == download_url
    }
}

fn prepared_version(app: &AppHandle) -> Option<String> {
    app.try_state::<PreparedUpdateState>().and_then(|state| {
        state.0.lock().ok().and_then(|prepared| {
            prepared
                .as_ref()
                .map(|value| value.metadata.version.clone())
        })
    })
}

fn matching_prepared_update(app: &AppHandle, update: &Update) -> bool {
    app.try_state::<PreparedUpdateState>()
        .and_then(|state| {
            state.0.lock().ok().and_then(|prepared| {
                prepared
                    .as_ref()
                    .map(|value| value.metadata.matches(update))
            })
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadyNotificationAction {
    None,
    MarkOnly,
    Notify,
}

fn ready_notification_action(
    origin: CheckOrigin,
    notification_sent: bool,
) -> ReadyNotificationAction {
    if notification_sent {
        ReadyNotificationAction::None
    } else if origin == CheckOrigin::Manual {
        ReadyNotificationAction::MarkOnly
    } else {
        ReadyNotificationAction::Notify
    }
}

fn finish_ready_notification(app: &AppHandle, origin: CheckOrigin, version: &str) {
    let notification_sent = was_version_notified(app, version);
    match ready_notification_action(origin, notification_sent) {
        ReadyNotificationAction::None => {}
        ReadyNotificationAction::MarkOnly => {
            if let Err(error) = mark_notification_sent(app, version) {
                eprintln!("persist manual update notification marker failed: {error}");
            }
        }
        ReadyNotificationAction::Notify => notify_update_ready_once(app, version),
    }
}

fn take_prepared_update(app: &AppHandle) -> Option<PreparedUpdate> {
    app.try_state::<PreparedUpdateState>()
        .and_then(|state| state.0.lock().ok().and_then(|mut prepared| prepared.take()))
}

fn restore_prepared_update(app: &AppHandle, prepared: PreparedUpdate) {
    if let Some(state) = app.try_state::<PreparedUpdateState>() {
        if let Ok(mut current) = state.0.lock() {
            *current = Some(prepared);
        }
    }
}

fn clear_prepared_update(app: &AppHandle) {
    if let Some(state) = app.try_state::<PreparedUpdateState>() {
        if let Ok(mut current) = state.0.lock() {
            *current = None;
        }
    }
}

fn mark_notification_sent(app: &AppHandle, version: &str) -> Result<(), String> {
    write_notification_marker(app, version)
}

fn load_verified_cache(
    app: &AppHandle,
    update: &Update,
) -> Result<Option<(CachedUpdateMetadata, Vec<u8>)>, String> {
    let directory = update_cache_directory(app)?;
    let Some((metadata, bytes)) = read_update_cache_at(&directory)? else {
        return Ok(None);
    };
    if !metadata.matches(update) {
        return Ok(None);
    }
    let public_key = updater_public_key(app)?;
    verify_package_identity(&bytes, &update.signature, &public_key, &metadata.version)?;
    Ok(Some((metadata, bytes)))
}

fn write_update_cache_at(
    directory: &Path,
    metadata: &CachedUpdateMetadata,
    bytes: &[u8],
) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_CACHE_PACKAGE_BYTES {
        return Err("已验证的更新包大小超出安全缓存限制。".to_string());
    }
    std::fs::create_dir_all(directory).map_err(|error| format!("创建更新缓存目录失败：{error}"))?;
    let json = serde_json::to_vec_pretty(metadata)
        .map_err(|error| format!("编码更新缓存元数据失败：{error}"))?;
    if json.len() as u64 > MAX_CACHE_METADATA_BYTES {
        return Err("更新缓存元数据超过允许大小。".to_string());
    }

    let cache_path = directory.join(UPDATE_CACHE_FILE);
    let temporary =
        cache_path.with_extension(format!("tmp-{}-{}", std::process::id(), unix_nanos()));
    let result = (|| {
        let metadata_length =
            u32::try_from(json.len()).map_err(|_| "更新缓存元数据长度无效。".to_string())?;
        let mut file =
            File::create(&temporary).map_err(|error| format!("创建临时更新缓存失败：{error}"))?;
        file.write_all(UPDATE_CACHE_MAGIC)
            .and_then(|_| file.write_all(&metadata_length.to_le_bytes()))
            .and_then(|_| file.write_all(&json))
            .and_then(|_| file.write_all(bytes))
            .map_err(|error| format!("写入临时更新缓存失败：{error}"))?;
        file.sync_all()
            .map_err(|error| format!("同步临时更新缓存失败：{error}"))?;
        replace_file(&temporary, &cache_path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn read_update_cache_at(
    directory: &Path,
) -> Result<Option<(CachedUpdateMetadata, Vec<u8>)>, String> {
    let cache_path = directory.join(UPDATE_CACHE_FILE);
    if !cache_path.exists() {
        return Ok(None);
    }
    if !cache_path.is_file() {
        return Err("更新缓存路径不是文件。".to_string());
    }
    let mut cache = read_bounded_file(&cache_path, MAX_CACHE_FILE_BYTES)?;
    let header_length = UPDATE_CACHE_MAGIC.len() + std::mem::size_of::<u32>();
    if cache.len() < header_length || !cache.starts_with(UPDATE_CACHE_MAGIC) {
        return Err("更新缓存格式无效。".to_string());
    }
    let metadata_length_offset = UPDATE_CACHE_MAGIC.len();
    let metadata_length = u32::from_le_bytes(
        cache[metadata_length_offset..header_length]
            .try_into()
            .map_err(|_| "更新缓存元数据长度无效。".to_string())?,
    ) as usize;
    if metadata_length == 0 || metadata_length as u64 > MAX_CACHE_METADATA_BYTES {
        return Err("更新缓存元数据长度超出限制。".to_string());
    }
    let metadata_end = header_length
        .checked_add(metadata_length)
        .filter(|end| *end <= cache.len())
        .ok_or_else(|| "更新缓存元数据不完整。".to_string())?;
    let metadata = serde_json::from_slice(&cache[header_length..metadata_end])
        .map_err(|error| format!("解析更新缓存元数据失败：{error}"))?;
    let bytes = cache.split_off(metadata_end);
    if bytes.is_empty() {
        return Err("更新缓存包为空。".to_string());
    }
    if bytes.len() as u64 > MAX_CACHE_PACKAGE_BYTES {
        return Err("更新缓存包超过允许大小。".to_string());
    }
    Ok(Some((metadata, bytes)))
}

fn read_bounded_file(path: &Path, maximum_bytes: u64) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| format!("打开更新缓存失败：{error}"))?;
    let mut bytes = Vec::new();
    file.take(maximum_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("读取更新缓存失败：{error}"))?;
    if bytes.len() as u64 > maximum_bytes {
        return Err("更新缓存文件超过允许大小。".to_string());
    }
    Ok(bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "更新缓存路径无效。".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| format!("创建更新缓存目录失败：{error}"))?;
    let temporary = path.with_extension(format!("tmp-{}-{}", std::process::id(), unix_nanos()));
    let result = (|| {
        let mut file =
            File::create(&temporary).map_err(|error| format!("创建临时更新缓存失败：{error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("写入临时更新缓存失败：{error}"))?;
        file.sync_all()
            .map_err(|error| format!("同步临时更新缓存失败：{error}"))?;
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(format!(
            "提交更新缓存失败：{}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::rename(source, destination).map_err(|error| format!("提交更新缓存失败：{error}"))
}

fn update_cache_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_cache_dir()
        .map(|path| path.join(UPDATE_CACHE_DIR))
        .map_err(|error| format!("无法确定更新缓存目录：{error}"))
}

fn notification_marker_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join(UPDATE_TRUST_STATE_FILE))
        .map_err(|error| format!("无法确定更新安全记录路径：{error}"))
}

fn was_version_notified(app: &AppHandle, version: &str) -> bool {
    notification_marker_path(app)
        .ok()
        .and_then(|path| read_update_trust_state_at(&path).ok())
        .is_some_and(|state| state.notified_versions.iter().any(|item| item == version))
}

fn write_notification_marker(app: &AppHandle, version: &str) -> Result<(), String> {
    let path = notification_marker_path(app)?;
    let mut state = read_update_trust_state_at(&path)?;
    if !state.notified_versions.iter().any(|item| item == version) {
        state.notified_versions.push(version.to_string());
    }
    if state.notified_versions.len() > MAX_NOTIFIED_VERSIONS {
        state
            .notified_versions
            .drain(..state.notified_versions.len() - MAX_NOTIFIED_VERSIONS);
    }
    write_update_trust_state_at(&path, &state)
}

fn read_update_trust_state_at(path: &Path) -> Result<UpdateTrustState, String> {
    if !path.is_file() {
        return Ok(UpdateTrustState::default());
    }
    let bytes = read_bounded_file(path, MAX_TRUST_STATE_BYTES)?;
    serde_json::from_slice(&bytes).map_err(|error| format!("解析更新安全记录失败：{error}"))
}

fn write_update_trust_state_at(path: &Path, state: &UpdateTrustState) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("编码更新通知记录失败：{error}"))?;
    write_atomic(&path, &json)
}

fn reject_trusted_version_rollback(app: &AppHandle, version: &str) -> Result<(), String> {
    let candidate = parse_numeric_release_version(version)?;
    let path = notification_marker_path(app)?;
    let state = read_update_trust_state_at(&path)?;
    let Some(highest) = state.highest_trusted_version else {
        return Ok(());
    };
    let highest_version = parse_numeric_release_version(&highest)?;
    if candidate < highest_version {
        return Err(format!(
            "更新清单版本 v{version} 低于本机已验证的 v{highest}，已拒绝覆盖。"
        ));
    }
    Ok(())
}

fn record_highest_trusted_version(app: &AppHandle, version: &str) -> Result<(), String> {
    let candidate = parse_numeric_release_version(version)?;
    let path = notification_marker_path(app)?;
    let mut state = read_update_trust_state_at(&path)?;
    let should_update = match state.highest_trusted_version.as_deref() {
        Some(highest) => candidate > parse_numeric_release_version(highest)?,
        None => true,
    };
    if should_update {
        state.highest_trusted_version = Some(version.to_string());
        write_update_trust_state_at(&path, &state)?;
    }
    Ok(())
}

fn clear_update_cache(app: &AppHandle) {
    if let Ok(directory) = update_cache_directory(app) {
        clear_update_cache_at(&directory);
    }
}

fn clear_update_cache_at(directory: &Path) {
    for file in [
        UPDATE_CACHE_FILE,
        LEGACY_UPDATE_CACHE_PACKAGE,
        LEGACY_UPDATE_CACHE_METADATA,
    ] {
        let path = directory.join(file);
        if path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }
    let _ = std::fs::remove_dir(directory);
}

fn updater_public_key(app: &AppHandle) -> Result<String, String> {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|config| config.get("pubkey"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| "应用更新公钥配置缺失。".to_string())
}

fn verify_package_signature(data: &[u8], signature: &str, public_key: &str) -> Result<(), String> {
    let public_key = decode_base64_text(public_key, "更新公钥")?;
    let public_key =
        PublicKey::decode(&public_key).map_err(|error| format!("解析更新公钥失败：{error}"))?;
    let signature = decode_base64_text(signature, "更新签名")?;
    let signature =
        Signature::decode(&signature).map_err(|error| format!("解析更新签名失败：{error}"))?;
    public_key
        .verify(data, &signature, true)
        .map_err(|error| format!("更新缓存签名验证失败：{error}"))
}

fn verify_package_identity(
    data: &[u8],
    signature: &str,
    public_key: &str,
    expected_version: &str,
) -> Result<(), String> {
    verify_package_signature(data, signature, public_key)?;
    verify_installer_product_version(data, expected_version)
}

fn parse_numeric_release_version(value: &str) -> Result<NumericReleaseVersion, String> {
    let parse_component = |component: Option<&str>| -> Result<u16, String> {
        let component = component.ok_or_else(|| "版本号必须使用 x.y.z 格式。".to_string())?;
        if component.is_empty()
            || !component.bytes().all(|byte| byte.is_ascii_digit())
            || (component.len() > 1 && component.starts_with('0'))
        {
            return Err(format!("版本号 {value} 不是规范的数字 x.y.z 格式。"));
        }
        component
            .parse::<u16>()
            .map_err(|_| format!("版本号 {value} 超出 Windows 产品版本范围。"))
    };

    let mut components = value.split('.');
    let version = NumericReleaseVersion {
        major: parse_component(components.next())?,
        minor: parse_component(components.next())?,
        patch: parse_component(components.next())?,
    };
    if components.next().is_some() {
        return Err(format!("版本号 {value} 不是规范的数字 x.y.z 格式。"));
    }
    Ok(version)
}

#[cfg(windows)]
fn verify_installer_product_version(data: &[u8], expected_version: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };

    const FIXED_FILE_INFO_SIGNATURE: u32 = 0xFEEF_04BD;
    let expected = parse_numeric_release_version(expected_version)?;
    let temporary = create_installer_version_probe(data)?;
    let result = (|| {
        let path: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut ignored = 0_u32;
        let info_size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut ignored) };
        if info_size == 0 {
            return Err(format!(
                "读取安装包产品版本大小失败：{}",
                std::io::Error::last_os_error()
            ));
        }
        let mut info = vec![0_u8; info_size as usize];
        if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, info_size, info.as_mut_ptr().cast()) }
            == 0
        {
            return Err(format!(
                "读取安装包产品版本失败：{}",
                std::io::Error::last_os_error()
            ));
        }

        let root = ['\\' as u16, 0];
        let mut fixed_info_pointer = std::ptr::null_mut();
        let mut fixed_info_length = 0_u32;
        if unsafe {
            VerQueryValueW(
                info.as_ptr().cast(),
                root.as_ptr(),
                &mut fixed_info_pointer,
                &mut fixed_info_length,
            )
        } == 0
            || fixed_info_pointer.is_null()
            || fixed_info_length < std::mem::size_of::<VS_FIXEDFILEINFO>() as u32
        {
            return Err("安装包缺少有效的 Windows 产品版本信息。".to_string());
        }
        let fixed =
            unsafe { std::ptr::read_unaligned(fixed_info_pointer.cast::<VS_FIXEDFILEINFO>()) };
        if fixed.dwSignature != FIXED_FILE_INFO_SIGNATURE {
            return Err("安装包 Windows 产品版本签名无效。".to_string());
        }
        let actual = NumericReleaseVersion {
            major: (fixed.dwProductVersionMS >> 16) as u16,
            minor: fixed.dwProductVersionMS as u16,
            patch: (fixed.dwProductVersionLS >> 16) as u16,
        };
        let revision = fixed.dwProductVersionLS as u16;
        if actual != expected || revision != 0 {
            return Err(format!(
                "安装包内嵌产品版本 {}.{}.{}.{} 与更新清单 v{expected_version} 不一致。",
                actual.major, actual.minor, actual.patch, revision
            ));
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&temporary);
    result
}

#[cfg(windows)]
fn create_installer_version_probe(data: &[u8]) -> Result<PathBuf, String> {
    for attempt in 0..16_u8 {
        let path = std::env::temp_dir().join(format!(
            "quotadock-version-check-{}-{}-{attempt}.exe",
            std::process::id(),
            unix_nanos()
        ));
        match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
        {
            Ok(mut file) => {
                let result = file
                    .write_all(data)
                    .and_then(|_| file.sync_all())
                    .map_err(|error| format!("准备安装包版本校验文件失败：{error}"));
                drop(file);
                if let Err(error) = result {
                    let _ = std::fs::remove_file(&path);
                    return Err(error);
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("创建安装包版本校验文件失败：{error}")),
        }
    }
    Err("无法创建唯一的安装包版本校验文件。".to_string())
}

#[cfg(not(windows))]
fn verify_installer_product_version(_data: &[u8], expected_version: &str) -> Result<(), String> {
    parse_numeric_release_version(expected_version).map(|_| ())
}

fn decode_base64_text(value: &str, label: &str) -> Result<String, String> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|error| format!("解码{label}失败：{error}"))?;
    String::from_utf8(decoded).map_err(|error| format!("{label}不是有效文本：{error}"))
}

#[cfg(feature = "desktop")]
fn notify_update_ready_once(app: &AppHandle, version: &str) {
    use tauri::plugin::PermissionState;
    use tauri_plugin_notification::NotificationExt;

    if !matches!(
        app.notification().permission_state(),
        Ok(PermissionState::Granted)
    ) {
        return;
    }
    if let Err(error) = mark_notification_sent(app, version) {
        eprintln!("persist update notification marker before display failed: {error}");
        return;
    }
    if let Err(error) = app
        .notification()
        .builder()
        .title(format!("{APP_NAME} 更新已准备"))
        .body(format!(
            "v{version} 已下载并通过签名验证，可在详情页选择安装。"
        ))
        .show()
    {
        eprintln!("display update-ready notification failed: {error}");
    }
}

#[cfg(not(feature = "desktop"))]
fn notify_update_ready_once(_app: &AppHandle, _version: &str) {}

fn build_updater(app: &AppHandle) -> Result<Updater, tauri_plugin_updater::Error> {
    let mut builder = app.updater_builder().configure_client(|client| {
        client
            .connect_timeout(UPDATE_CONNECT_TIMEOUT)
            .read_timeout(UPDATE_READ_TIMEOUT)
    });

    if !environment_proxy_configured() {
        if let Some(proxy) = windows_internet_proxy() {
            builder = builder.proxy(proxy);
        }
    }

    #[cfg(feature = "desktop")]
    {
        let exit_app = app.clone();
        builder = builder.on_before_exit(move || {
            crate::window_state::save_main_window_position_for_app(&exit_app);
        });
    }

    builder.build()
}

fn environment_proxy_configured() -> bool {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .any(|name| {
            std::env::var_os(name).is_some_and(|value| !value.to_string_lossy().trim().is_empty())
        })
}

#[cfg(windows)]
fn windows_internet_proxy() -> Option<Url> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let internet_settings = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;
    let enabled = internet_settings
        .get_value::<u32, _>("ProxyEnable")
        .unwrap_or(0);
    if enabled == 0 {
        return None;
    }
    let proxy_server = internet_settings
        .get_value::<String, _>("ProxyServer")
        .ok()?;
    parse_windows_proxy_server(&proxy_server)
}

#[cfg(not(windows))]
fn windows_internet_proxy() -> Option<Url> {
    None
}

fn parse_windows_proxy_server(raw: &str) -> Option<Url> {
    let mut generic = None;
    let mut http = None;
    let mut https = None;

    for entry in raw
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        if let Some((protocol, address)) = entry.split_once('=') {
            let address = address.trim();
            if address.is_empty() {
                continue;
            }
            match protocol.trim().to_ascii_lowercase().as_str() {
                "https" => https = Some(address),
                "http" => http = Some(address),
                _ => {}
            }
        } else if generic.is_none() {
            generic = Some(entry);
        }
    }

    let selected = https.or(http).or(generic)?;
    let normalized = if selected.contains("://") {
        selected.to_string()
    } else {
        format!("http://{selected}")
    };
    let parsed = Url::parse(&normalized).ok()?;
    matches!(parsed.scheme(), "http" | "https").then_some(parsed)
}

fn check_request_failure(error: tauri_plugin_updater::Error) -> UpdateFailure {
    let detail = format!("获取签名更新清单失败：{error}");
    let lower = detail.to_ascii_lowercase();
    let message = if [
        "error sending request",
        "timed out",
        "timeout",
        "connect",
        "dns",
        "network",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        "暂时无法连接更新服务，请检查网络或代理后重试。"
    } else {
        "无法读取更新信息，请稍后重试。"
    };
    UpdateFailure::new(message, detail)
}

fn automatic_checks_enabled(app: &AppHandle) -> bool {
    crate::commands::load_app_state(app)
        .map(|state| state.settings.automatic_update_checks)
        .unwrap_or(true)
}

fn begin_update_check(origin: CheckOrigin) -> Result<UpdateCheckPermit, String> {
    UPDATE_CHECK_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| UpdateCheckPermit)
        .map_err(|_| match origin {
            CheckOrigin::Manual => "已有更新检查正在进行。".to_string(),
            CheckOrigin::Automatic => "已有更新检查正在进行，跳过本次自动检查。".to_string(),
        })
}

fn current_update_status(app: &AppHandle) -> UpdateStatus {
    app.try_state::<UpdateStatusState>()
        .and_then(|state| state.0.lock().ok().map(|status| status.clone()))
        .unwrap_or_else(UpdateStatus::idle)
}

fn set_update_status(app: &AppHandle, status: UpdateStatus) {
    if let Some(state) = app.try_state::<UpdateStatusState>() {
        if let Ok(mut current) = state.0.lock() {
            *current = status.clone();
        }
    }
    let _ = app.emit_to("details", UPDATE_STATUS_EVENT, status);
}

fn menu_status_for(status: &UpdateStatus) -> String {
    match status.phase {
        UpdatePhase::Idle => "更新：尚未检查".to_string(),
        UpdatePhase::Checking => "正在检查更新…".to_string(),
        UpdatePhase::UpToDate => format!("已是最新版 v{}", status.current_version),
        UpdatePhase::Downloading => "正在下载并验证更新…".to_string(),
        UpdatePhase::Ready => status
            .available_version
            .as_deref()
            .map(|version| format!("更新 v{version} 已准备"))
            .unwrap_or_else(|| "更新已准备".to_string()),
        UpdatePhase::Installing => "正在启动更新安装…".to_string(),
        UpdatePhase::Error => "更新检查失败，请查看详情".to_string(),
    }
}

fn now_marker() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{seconds}")
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::{
        begin_update_check, check_request_failure, menu_status_for, parse_numeric_release_version,
        parse_windows_proxy_server, read_update_cache_at, read_update_trust_state_at,
        ready_notification_action, verify_installer_product_version, verify_package_signature,
        write_atomic, write_update_cache_at, CachedUpdateMetadata, CheckOrigin,
        NumericReleaseVersion, ReadyNotificationAction, UpdatePhase, UpdateStatus,
        UpdateTrustState, AUTO_CHECK_INTERVAL, MAX_CACHE_METADATA_BYTES, MAX_TRUST_STATE_BYTES,
        UPDATE_CACHE_FILE, UPDATE_CACHE_MAGIC,
    };
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let unique = format!(
                "quotadock-update-{name}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn metadata() -> CachedUpdateMetadata {
        CachedUpdateMetadata {
            version: "9.9.9".to_string(),
            signature: "signed-manifest-value".to_string(),
            download_url: "https://example.invalid/update.exe".to_string(),
        }
    }

    #[test]
    fn parses_common_windows_proxy_formats() {
        assert_eq!(
            parse_windows_proxy_server("127.0.0.1:7897")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:7897/"
        );
        assert_eq!(
            parse_windows_proxy_server("http=proxy.local:8080;https=secure.local:8443")
                .unwrap()
                .as_str(),
            "http://secure.local:8443/"
        );
        assert_eq!(
            parse_windows_proxy_server("https://proxy.local:9443")
                .unwrap()
                .as_str(),
            "https://proxy.local:9443/"
        );
        assert!(parse_windows_proxy_server("socks=127.0.0.1:1080").is_none());
        assert!(parse_windows_proxy_server("not a proxy").is_none());
    }

    #[test]
    fn network_failure_has_recovery_copy_without_exposing_url_in_menu() {
        let failure = check_request_failure(tauri_plugin_updater::Error::Network(
            "error sending request for url (https://example.invalid/latest.json)".to_string(),
        ));
        let status = failure.into_status();
        assert_eq!(status.phase, UpdatePhase::Error);
        assert!(status.message.contains("代理"));
        assert!(status
            .technical_detail
            .as_deref()
            .is_some_and(|detail| detail.contains("latest.json")));
        assert!(!menu_status_for(&status).contains("http"));
    }

    #[test]
    fn menu_update_statuses_stay_compact() {
        let status = UpdateStatus::error(
            "暂时无法连接更新服务，请检查网络或代理后重试。",
            "https://example.invalid/a/very/long/path/latest.json",
        );
        let menu = menu_status_for(&status);
        assert!(menu.chars().count() <= 16);
        assert!(!menu.contains("http"));
    }

    #[test]
    fn lifecycle_has_six_hour_interval_and_explicit_ready_state() {
        assert_eq!(AUTO_CHECK_INTERVAL, Duration::from_secs(6 * 60 * 60));
        let ready = UpdateStatus::ready("9.9.9");
        assert_eq!(ready.phase, UpdatePhase::Ready);
        assert_eq!(ready.progress_percent, Some(100));
        assert!(ready.message.contains("签名"));
        let retained = UpdateStatus::ready_after_check_failure("9.9.9", "network unavailable");
        assert_eq!(retained.phase, UpdatePhase::Ready);
        assert!(retained
            .technical_detail
            .as_deref()
            .is_some_and(|detail| detail.contains("network")));
        assert_eq!(
            ready_notification_action(CheckOrigin::Automatic, false),
            ReadyNotificationAction::Notify
        );
        assert_eq!(
            ready_notification_action(CheckOrigin::Automatic, true),
            ReadyNotificationAction::None
        );
        assert_eq!(
            ready_notification_action(CheckOrigin::Manual, false),
            ReadyNotificationAction::MarkOnly
        );
        assert_eq!(
            menu_status_for(&UpdateStatus::installing("9.9.9")),
            "正在启动更新安装…"
        );
    }

    #[test]
    fn cache_round_trip_preserves_release_identity_in_one_atomic_file() {
        let directory = TestDir::new("round-trip");
        let expected = metadata();
        write_update_cache_at(directory.path(), &expected, b"verified-package").unwrap();

        let (actual, bytes) = read_update_cache_at(directory.path()).unwrap().unwrap();
        assert_eq!(actual, expected);
        assert_eq!(bytes, b"verified-package");
        assert!(directory.path().join(UPDATE_CACHE_FILE).is_file());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        assert!(actual.matches_release(
            "9.9.9",
            "signed-manifest-value",
            "https://example.invalid/update.exe"
        ));
        assert!(!actual.matches_release(
            "9.9.8",
            "signed-manifest-value",
            "https://example.invalid/update.exe"
        ));
    }

    #[test]
    fn cache_reader_rejects_incomplete_and_oversized_metadata() {
        let incomplete = TestDir::new("incomplete");
        std::fs::write(
            incomplete.path().join(UPDATE_CACHE_FILE),
            UPDATE_CACHE_MAGIC,
        )
        .unwrap();
        assert!(read_update_cache_at(incomplete.path()).is_err());

        let oversized = TestDir::new("oversized");
        let mut invalid = UPDATE_CACHE_MAGIC.to_vec();
        invalid.extend_from_slice(&((MAX_CACHE_METADATA_BYTES + 1) as u32).to_le_bytes());
        invalid.extend_from_slice(b"package");
        std::fs::write(oversized.path().join(UPDATE_CACHE_FILE), invalid).unwrap();
        assert!(read_update_cache_at(oversized.path()).is_err());
    }

    #[test]
    fn cache_writer_reports_persistence_failure() {
        let directory = TestDir::new("write-failure");
        let invalid_directory = directory.path().join("ordinary-file");
        std::fs::write(&invalid_directory, b"not a directory").unwrap();

        assert!(write_update_cache_at(&invalid_directory, &metadata(), b"package").is_err());
    }

    #[test]
    fn update_trust_state_is_persistent_and_bounded() {
        let directory = TestDir::new("notification-marker");
        let path = directory.path().join("notified-version");
        let expected = UpdateTrustState {
            notified_versions: vec!["9.9.8".to_string(), "9.9.9".to_string()],
            highest_trusted_version: Some("9.9.9".to_string()),
        };
        write_atomic(&path, &serde_json::to_vec(&expected).unwrap()).unwrap();
        assert_eq!(read_update_trust_state_at(&path).unwrap(), expected);
        std::fs::write(&path, vec![b'x'; MAX_TRUST_STATE_BYTES as usize + 1]).unwrap();
        assert!(read_update_trust_state_at(&path).is_err());
    }

    #[test]
    fn numeric_release_versions_are_strict_and_ordered() {
        assert_eq!(
            parse_numeric_release_version("0.5.4").unwrap(),
            NumericReleaseVersion {
                major: 0,
                minor: 5,
                patch: 4,
            }
        );
        assert!(
            parse_numeric_release_version("0.5.5").unwrap()
                > parse_numeric_release_version("0.5.4").unwrap()
        );
        for invalid in [
            "v0.5.4",
            "0.5",
            "0.5.4.0",
            "0.05.4",
            "0.5.4-beta",
            "65536.0.0",
        ] {
            assert!(parse_numeric_release_version(invalid).is_err(), "{invalid}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn installer_product_version_rejects_non_executable_bytes() {
        assert!(verify_installer_product_version(b"not-an-installer", "0.5.4").is_err());
    }

    #[test]
    fn cached_package_signature_must_be_decodable_and_valid() {
        assert!(verify_package_signature(b"package", "not-base64", "not-base64").is_err());
    }

    #[test]
    fn prevents_concurrent_update_checks() {
        let permit = begin_update_check(CheckOrigin::Manual).unwrap();
        let duplicate = begin_update_check(CheckOrigin::Automatic).unwrap_err();
        assert!(duplicate.contains("跳过"));
        drop(permit);
        assert!(begin_update_check(CheckOrigin::Manual).is_ok());
    }
}
