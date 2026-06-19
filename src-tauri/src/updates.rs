use crate::version::{APP_NAME, APP_VERSION, GITHUB_REPOSITORY, UPDATE_MANIFEST_URL};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONQUESTION, MB_YESNO, SW_SHOWNORMAL,
};

const WINDOWS_X64: &str = "windows-x86_64";

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

pub fn check_on_startup(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let _ = check_download_and_prompt(app).await;
    });
}

pub fn check_now(app: AppHandle) {
    #[cfg(feature = "desktop")]
    crate::tray::set_menu_status(&app, "更新检查中...");

    tauri::async_runtime::spawn(async move {
        let message = match check_download_and_prompt(app.clone()).await {
            Ok(message) => message,
            Err(error) => format!("更新检查失败：{error}"),
        };

        #[cfg(feature = "desktop")]
        crate::tray::set_menu_status_temporarily(&app, message);
    });
}

async fn check_download_and_prompt(app: AppHandle) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(format!("{APP_NAME}/{APP_VERSION} ({GITHUB_REPOSITORY})"))
        .build()
        .map_err(|error| format!("创建更新检查客户端失败：{error}"))?;

    let manifest = fetch_manifest(&client).await?;
    if !is_newer_version(&manifest.version, APP_VERSION)? {
        return Ok(format!("已是最新版本 v{APP_VERSION}"));
    }

    let package = manifest
        .platforms
        .get(WINDOWS_X64)
        .ok_or_else(|| "发布清单中没有 Windows x64 更新包。".to_string())?;
    let installer = download_package(&app, &client, &manifest.version, package).await?;
    let message = install_prompt_message(&manifest, package, &installer);
    let answer = show_message("QuotaDock 更新已下载", &message, MB_YESNO | MB_ICONQUESTION);
    if answer == IDYES {
        launch_installer_and_exit(&app, &installer)?;
    }

    Ok(format!("已下载更新包 v{}", manifest.version))
}

async fn fetch_manifest(client: &reqwest::Client) -> Result<UpdateManifest, String> {
    client
        .get(UPDATE_MANIFEST_URL)
        .send()
        .await
        .map_err(|error| format!("获取更新清单失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("获取更新清单失败：{error}"))?
        .json::<UpdateManifest>()
        .await
        .map_err(|error| format!("解析更新清单失败：{error}"))
}

async fn download_package(
    app: &AppHandle,
    client: &reqwest::Client,
    version: &str,
    package: &UpdatePackage,
) -> Result<PathBuf, String> {
    let bytes = client
        .get(&package.url)
        .send()
        .await
        .map_err(|error| format!("下载安装包失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("下载安装包失败：{error}"))?
        .bytes()
        .await
        .map_err(|error| format!("读取安装包失败：{error}"))?;

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
    use super::{format_size, is_newer_version, sha256_hex};

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
}
