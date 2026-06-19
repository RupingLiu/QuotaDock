use crate::version::{APP_NAME, APP_VERSION, GITHUB_REPOSITORY, UPDATE_MANIFEST_URL};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONQUESTION, MB_YESNO, SW_SHOWNORMAL,
};

const WINDOWS_X64: &str = "windows-x86_64";
const AUTO_FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
static DOWNLOAD_COUNTER: AtomicU64 = AtomicU64::new(0);
static UPDATE_CHECK_RUNNING: AtomicBool = AtomicBool::new(false);
static AUTO_PROMPTED_VERSION: Mutex<Option<String>> = Mutex::new(None);

#[derive(Debug, Deserialize)]
struct UpdateManifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
    platforms: HashMap<String, UpdatePackage>,
}

#[derive(Debug, Deserialize)]
struct UpdatePackage {
    url: String,
    sha256: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    filename: Option<String>,
}

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

pub fn start_auto_check(app: AppHandle) {
    let _ = thread::Builder::new()
        .name("quotadock-auto-update-check".to_string())
        .spawn(move || {
            thread::sleep(AUTO_FIRST_CHECK_DELAY);
            loop {
                let check_app = app.clone();
                tauri::async_runtime::block_on(async move {
                    if let Err(error) =
                        check_download_and_prompt(check_app, CheckOrigin::Automatic).await
                    {
                        eprintln!("automatic update check failed: {error}");
                    }
                });
                thread::sleep(AUTO_CHECK_INTERVAL);
            }
        });
}

pub fn check_now(app: AppHandle) {
    #[cfg(feature = "desktop")]
    crate::tray::set_menu_status(&app, "更新检查中...");

    tauri::async_runtime::spawn(async move {
        let message = match check_download_and_prompt(app.clone(), CheckOrigin::Manual).await {
            Ok(message) => message,
            Err(error) => format!("更新检查失败：{error}"),
        };

        #[cfg(feature = "desktop")]
        crate::tray::set_menu_status_temporarily(&app, message);
    });
}

async fn check_download_and_prompt(app: AppHandle, origin: CheckOrigin) -> Result<String, String> {
    let _permit = begin_update_check(origin)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(format!("{APP_NAME}/{APP_VERSION} ({GITHUB_REPOSITORY})"))
        .build()
        .map_err(|error| format!("创建更新检查客户端失败：{error}"))?;

    let manifest = fetch_manifest(&client).await?;
    if !is_newer_version(&manifest.version, APP_VERSION)? {
        return Ok(format!("已是最新版本 v{APP_VERSION}"));
    }
    if origin == CheckOrigin::Automatic && was_auto_prompted(&manifest.version) {
        return Ok(format!("已提醒过更新包 v{}", manifest.version));
    }

    let package = manifest
        .platforms
        .get(WINDOWS_X64)
        .ok_or_else(|| "发布清单中没有 Windows x64 更新包。".to_string())?;
    let installer = download_package(&app, &client, &manifest.version, package).await?;
    if origin == CheckOrigin::Automatic {
        remember_auto_prompt(&manifest.version);
    }
    let message = install_prompt_message(&manifest, package, &installer);
    let answer = show_message("QuotaDock 更新已下载", &message, MB_YESNO | MB_ICONQUESTION);
    if answer == IDYES {
        launch_installer_and_exit(&app, &installer)?;
    }

    Ok(format!("已下载更新包 v{}", manifest.version))
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

fn was_auto_prompted(version: &str) -> bool {
    AUTO_PROMPTED_VERSION
        .lock()
        .map(|prompted| prompted.as_deref() == Some(version))
        .unwrap_or(false)
}

fn remember_auto_prompt(version: &str) {
    if let Ok(mut prompted) = AUTO_PROMPTED_VERSION.lock() {
        *prompted = Some(version.to_string());
    }
}

async fn fetch_manifest(client: &reqwest::Client) -> Result<UpdateManifest, String> {
    let bytes = fetch_url_bytes(client, UPDATE_MANIFEST_URL, "获取更新清单").await?;
    serde_json::from_slice::<UpdateManifest>(&bytes)
        .map_err(|error| format!("解析更新清单失败：{error}"))
}

async fn download_package(
    app: &AppHandle,
    client: &reqwest::Client,
    version: &str,
    package: &UpdatePackage,
) -> Result<PathBuf, String> {
    let bytes = fetch_url_bytes(client, &package.url, "下载安装包").await?;

    let digest = sha256_hex(&bytes);
    if !digest.eq_ignore_ascii_case(&package.sha256) {
        return Err("安装包 SHA256 校验失败，已拒绝安装。".to_string());
    }

    let filename = package
        .filename
        .clone()
        .unwrap_or_else(|| format!("QuotaDock_{version}_x64-setup.exe"));
    let update_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取更新缓存目录失败：{error}"))?
        .join("updates");
    fs::create_dir_all(&update_dir).map_err(|error| format!("创建更新缓存目录失败：{error}"))?;
    let installer = update_dir.join(filename);
    fs::write(&installer, &bytes).map_err(|error| format!("保存安装包失败：{error}"))?;
    Ok(installer)
}

async fn fetch_url_bytes(
    client: &reqwest::Client,
    url: &str,
    action: &str,
) -> Result<Vec<u8>, String> {
    match fetch_url_bytes_with_reqwest(client, url).await {
        Ok(bytes) => Ok(bytes),
        Err(reqwest_error) => {
            let reqwest_error = describe_error_chain(&reqwest_error);
            let url = url.to_string();
            match tauri::async_runtime::spawn_blocking(move || download_with_system_tool(&url)).await
            {
                Ok(Ok(bytes)) => Ok(bytes),
                Ok(Err(system_error)) => Err(format!(
                    "{action}失败：内置网络请求失败：{reqwest_error}；系统下载工具也失败：{system_error}"
                )),
                Err(join_error) => Err(format!(
                    "{action}失败：内置网络请求失败：{reqwest_error}；系统下载任务启动失败：{join_error}"
                )),
            }
        }
    }
}

async fn fetch_url_bytes_with_reqwest(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<u8>, reqwest::Error> {
    let response = client.get(url).send().await?.error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}

fn download_with_system_tool(url: &str) -> Result<Vec<u8>, String> {
    let path = temp_download_path();
    let result = try_curl_download(url, &path).or_else(|curl_error| {
        try_powershell_download(url, &path).map_err(|powershell_error| {
            format!("curl.exe: {curl_error}；PowerShell: {powershell_error}")
        })
    });

    let bytes = match result {
        Ok(()) => fs::read(&path).map_err(|error| format!("读取系统下载临时文件失败：{error}")),
        Err(error) => Err(error),
    };
    let _ = fs::remove_file(&path);
    bytes
}

fn try_curl_download(url: &str, path: &Path) -> Result<(), String> {
    let mut command = Command::new("curl.exe");
    command
        .args([
            "-L",
            "--fail",
            "--silent",
            "--show-error",
            "--max-time",
            "90",
            "--output",
        ])
        .arg(path)
        .arg(url);
    run_download_command("curl.exe", &mut command)
}

fn try_powershell_download(url: &str, path: &Path) -> Result<(), String> {
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "$ProgressPreference = 'SilentlyContinue'; Invoke-WebRequest -Uri $args[0] -OutFile $args[1] -UseBasicParsing",
        ])
        .arg(url)
        .arg(path);
    run_download_command("powershell.exe", &mut command)
}

fn run_download_command(name: &str, command: &mut Command) -> Result<(), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_command_window(command);

    let output = command
        .output()
        .map_err(|error| format!("启动失败：{error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format_command_failure(name, &output))
    }
}

fn hide_command_window(command: &mut Command) {
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

fn temp_download_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = DOWNLOAD_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "quotadock-update-{}-{nanos}-{counter}.tmp",
        std::process::id()
    ))
}

fn is_newer_version(candidate: &str, current: &str) -> Result<bool, String> {
    let candidate = Version::parse(candidate.trim_start_matches('v'))
        .map_err(|error| format!("发布版本号无效：{error}"))?;
    let current = Version::parse(current.trim_start_matches('v'))
        .map_err(|error| format!("当前版本号无效：{error}"))?;
    Ok(candidate > current)
}

fn install_prompt_message(
    manifest: &UpdateManifest,
    package: &UpdatePackage,
    installer: &Path,
) -> String {
    let mut message = format!(
        "已下载 QuotaDock v{}。\n\n安装包：{}\n",
        manifest.version,
        installer.display()
    );
    if let Some(size) = package.size {
        message.push_str(&format!("大小：{}\n", format_size(size)));
    }
    if let Some(notes) = manifest.notes.as_deref().filter(|notes| !notes.is_empty()) {
        message.push_str(&format!("\n更新内容：\n{notes}\n"));
    }
    message.push_str("\n是否现在退出 QuotaDock 并启动安装程序？");
    message
}

fn launch_installer_and_exit(app: &AppHandle, installer: &Path) -> Result<(), String> {
    match shell_execute_installer(installer, "open") {
        Ok(()) => {
            app.exit(0);
            Ok(())
        }
        Err(open_error) => match shell_execute_installer(installer, "runas") {
            Ok(()) => {
                app.exit(0);
                Ok(())
            }
            Err(runas_error) => Err(format!("{open_error}；提权启动也失败：{runas_error}")),
        },
    }
}

fn shell_execute_installer(installer: &Path, operation: &str) -> Result<(), String> {
    let operation = wide_null(operation);
    let file = wide_null(&installer.display().to_string());
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;

    if result > 32 {
        return Ok(());
    }

    Err(format!("启动安装程序失败：ShellExecuteW 返回 {result}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn describe_error_chain(error: &dyn Error) -> String {
    let mut messages = vec![error.to_string()];
    let mut source = error.source();
    while let Some(error) = source {
        messages.push(error.to_string());
        source = error.source();
    }
    messages.join(" / ")
}

fn format_command_failure(name: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "无输出".to_string()
    };
    format!("{name} 退出码 {}：{detail}", output.status)
}

fn format_size(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    format!("{:.1} MB", bytes as f64 / MIB)
}

fn show_message(title: &str, body: &str, flags: u32) -> i32 {
    let title = wide_null(title);
    let body = wide_null(body);
    unsafe { MessageBoxW(std::ptr::null_mut(), body.as_ptr(), title.as_ptr(), flags) }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        begin_update_check, format_size, is_newer_version, remember_auto_prompt, sha256_hex,
        temp_download_path, was_auto_prompted, CheckOrigin,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn compares_semver_versions() {
        assert!(is_newer_version("0.2.0", "0.1.1").unwrap());
        assert!(!is_newer_version("0.2.0", "0.2.0").unwrap());
        assert!(!is_newer_version("0.1.1", "0.2.0").unwrap());
    }

    #[test]
    fn hashes_package_bytes() {
        assert_eq!(
            sha256_hex(b"QuotaDock"),
            "7505639e5476bdc4688f582a716adfbdfff6b19640ae80b736291fe1b7e877a2"
        );
    }

    #[test]
    fn formats_package_size() {
        assert_eq!(format_size(4 * 1024 * 1024), "4.0 MB");
    }

    #[test]
    fn creates_unique_temp_download_paths() {
        let first = temp_download_path();
        let second = temp_download_path();

        assert_ne!(first, second);
        assert!(first
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .starts_with("quotadock-update-"));
    }

    #[test]
    fn prevents_concurrent_update_checks() {
        let permit = begin_update_check(CheckOrigin::Manual).unwrap();

        let duplicate = begin_update_check(CheckOrigin::Automatic).unwrap_err();

        assert!(duplicate.contains("跳过本次自动检查"));
        drop(permit);
        assert!(begin_update_check(CheckOrigin::Manual).is_ok());
    }

    #[test]
    fn remembers_auto_prompted_version() {
        let unique_version = format!(
            "99.{}.{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );

        assert!(!was_auto_prompted(&unique_version));
        remember_auto_prompt(&unique_version);

        assert!(was_auto_prompted(&unique_version));
        assert!(!was_auto_prompted("99.0.0-other"));
    }
}
