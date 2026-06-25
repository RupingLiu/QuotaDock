use crate::models::{AppState, QuotaSnapshot, RefreshUsageResult, SnapshotSource};
use crate::status_parser::{parse_status_text_with_source, ParseClock};
use crate::usage_store::{StoreError, UsageStore};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{App, AppHandle, Emitter, Manager};

pub const USAGE_STATE_CHANGED_EVENT: &str = "usage-state-changed";
const AUTO_FIRST_REFRESH_DELAY: Duration = Duration::from_secs(10);
const AUTO_BASE_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const AUTO_LOW_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const AUTO_POST_RESET_REFRESH_DELAY: Duration = Duration::from_secs(30);
const AUTO_RESET_WATCH_WINDOW: Duration = Duration::from_secs(10 * 60);
const AUTO_MAX_FAILURE_BACKOFF: Duration = Duration::from_secs(30 * 60);
const LOW_USAGE_THRESHOLD_PERCENT: u8 = 20;

#[derive(Clone, Default)]
pub struct RefreshCoordinator {
    running: Arc<AtomicBool>,
}

struct RefreshPermit {
    running: Arc<AtomicBool>,
}

impl RefreshCoordinator {
    fn try_begin(&self) -> Option<RefreshPermit> {
        self.running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| RefreshPermit {
                running: Arc::clone(&self.running),
            })
    }
}

impl Drop for RefreshPermit {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy)]
enum RefreshOrigin {
    Command,
    Tray,
    Automatic,
}

impl RefreshOrigin {
    fn duplicate_message(self) -> String {
        match self {
            Self::Automatic => "已有额度查询正在进行，跳过本次自动查询。".to_string(),
            Self::Command | Self::Tray => "已有额度查询正在进行。".to_string(),
        }
    }
}

pub fn install_refresh_coordinator(app: &App) {
    app.manage(RefreshCoordinator::default());
}

pub fn start_auto_refresh(app: AppHandle) {
    let _ = thread::Builder::new()
        .name("quotadock-auto-refresh".to_string())
        .spawn(move || {
            let mut consecutive_failures = 0;
            thread::sleep(AUTO_FIRST_REFRESH_DELAY);
            loop {
                let refresh_app = app.clone();
                let outcome = tauri::async_runtime::block_on(async move {
                    refresh_usage_internal(refresh_app, RefreshOrigin::Automatic).await
                });
                let schedule = next_auto_refresh_schedule(&outcome, consecutive_failures);
                consecutive_failures = schedule.consecutive_failures;
                thread::sleep(schedule.delay);
            }
        });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AutoRefreshSchedule {
    delay: Duration,
    consecutive_failures: u32,
}

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
    [&snapshot.five_hour, &snapshot.weekly]
        .iter()
        .any(|reading| {
            reading
                .remaining_percent
                .is_some_and(|percent| percent <= LOW_USAGE_THRESHOLD_PERCENT)
        })
}

fn imminent_reset_refresh_interval(snapshot: &QuotaSnapshot) -> Option<Duration> {
    [&snapshot.five_hour, &snapshot.weekly]
        .iter()
        .filter_map(|reading| reading.reset_countdown_seconds)
        .filter(|seconds| *seconds <= AUTO_RESET_WATCH_WINDOW.as_secs() as i64)
        .map(|seconds| Duration::from_secs(seconds.max(0) as u64) + AUTO_POST_RESET_REFRESH_DELAY)
        .min()
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
    refresh_usage_internal(app, RefreshOrigin::Command).await
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

pub fn refresh_usage_from_tray(app: AppHandle) {
    #[cfg(feature = "desktop")]
    crate::tray::set_menu_status(&app, "额度查询中...");

    tauri::async_runtime::spawn(async move {
        let message = match refresh_usage_internal(app.clone(), RefreshOrigin::Tray).await {
            Ok(result) if result.updated => "额度已更新".to_string(),
            Ok(result) => result.message,
            Err(error) => error,
        };

        #[cfg(feature = "desktop")]
        crate::tray::set_menu_status_temporarily(&app, message);
    });
}

async fn refresh_usage_internal(
    app: AppHandle,
    origin: RefreshOrigin,
) -> Result<RefreshUsageResult, String> {
    let _permit = begin_refresh(&app, origin)?;
    let worker_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || refresh_usage_blocking(worker_app))
        .await
        .map_err(|error| format!("后台查询任务失败：{error}"))??;
    sync_tray(&app, &result.app_state);
    emit_usage_state(&app, &result);
    Ok(result)
}

fn begin_refresh(app: &AppHandle, origin: RefreshOrigin) -> Result<RefreshPermit, String> {
    app.try_state::<RefreshCoordinator>()
        .and_then(|coordinator| coordinator.try_begin())
        .ok_or_else(|| origin.duplicate_message())
}

fn refresh_usage_blocking(app: AppHandle) -> Result<RefreshUsageResult, String> {
    let store = store_for_app(&app)?;
    match fetch_usage_from_codex_cli() {
        Ok(snapshot) => {
            let app_state = store
                .save_snapshot(snapshot)
                .map(|outcome| outcome.into_app_state())
                .map_err(to_command_error)?;
            Ok(RefreshUsageResult {
                app_state,
                updated: true,
                message: "已通过 Codex CLI 更新额度。".to_string(),
            })
        }
        Err(message) => {
            let app_state = load_app_state(&app)?;
            Ok(RefreshUsageResult {
                app_state,
                updated: false,
                message,
            })
        }
    }
}

pub fn load_app_state(app: &AppHandle) -> Result<AppState, String> {
    let store = store_for_app(app)?;
    store
        .load()
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)
}

pub fn state_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("quotadock-state.json"))
        .map_err(|error| error.to_string())
}

fn fetch_usage_from_codex_cli() -> Result<QuotaSnapshot, String> {
    let output = run_codex_status_pty(Duration::from_secs(45)).or_else(|pty_error| {
        fetch_usage_from_structured_cli().map_err(|structured_error| {
            if structured_error.contains("未找到 Codex CLI") {
                structured_error
            } else {
                pty_error
            }
        })
    })?;

    let mut result =
        parse_status_text_with_source(&output, ParseClock::now(), SnapshotSource::CodexCli);
    result.snapshot.status_message = "已通过 Codex CLI 更新额度。".to_string();
    result.snapshot.raw_text.clear();
    if result.snapshot.has_any_usage() {
        Ok(result.snapshot)
    } else {
        Err("Codex CLI 没有返回可识别的额度，请稍后重试。".to_string())
    }
}

fn fetch_usage_from_structured_cli() -> Result<String, String> {
    let help = run_codex(&["--help"], Duration::from_secs(3))?;
    if !help.success {
        return Err("Codex CLI 无法执行，请确认 codex 已安装并可登录。".to_string());
    }

    let args = if command_list_contains(&help.stdout, "usage") {
        ["usage", "--json"]
    } else if command_list_contains(&help.stdout, "status") {
        ["status", "--json"]
    } else {
        return Err("Codex CLI 未提供结构化额度查询。".to_string());
    };

    let output = run_codex(&args, Duration::from_secs(8))?;
    if output.success {
        Ok(output.stdout)
    } else {
        Err("Codex CLI 额度查询失败，请稍后重试。".to_string())
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

#[cfg(windows)]
mod windows_conpty {
    use super::{
        codex_status_output_ready, is_cmd_shim, should_send_status_command,
        status_command_waiting_for_enter,
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
        if std::env::var_os("QUOTADOCK_STATUS_PROBE_COMMAND").is_some() {
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

        if std::env::var_os("QUOTADOCK_STATUS_PROBE_COMMAND").is_some() {
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
        let mut cursor_reported = false;

        loop {
            drain_receiver(&receiver, &mut output);
            let text = String::from_utf8_lossy(&output).to_string();
            respond_to_cursor_query(&mut writer, &text, &mut cursor_reported)?;

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
            }

            if sent_count > 0 && codex_status_output_ready(&text) {
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

        if let Some(override_command) = std::env::var_os("QUOTADOCK_STATUS_PROBE_COMMAND") {
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

            loop {
                reader.drain(&mut output);
                let text = String::from_utf8_lossy(&output).to_string();

                if should_send_status_command(&text, started, last_sent, sent_count, false) {
                    write_all(input_write.raw(), b"/status\r")?;
                    sent_count += 1;
                    last_sent = Instant::now();
                }

                if sent_count > 0 && codex_status_output_ready(&text) {
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
        if let Some(override_command) = std::env::var_os("QUOTADOCK_STATUS_PROBE_COMMAND") {
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
        let Some(path) = std::env::var_os("QUOTADOCK_STATUS_PROBE_LOG") else {
            return;
        };
        let _ = std::fs::write(path, output);
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
}

fn should_send_status_command(
    _output: &str,
    started: Instant,
    last_sent: Instant,
    sent_count: u8,
    _terminal_ready: bool,
) -> bool {
    if sent_count >= 6 {
        return false;
    }
    if sent_count == 0 {
        return started.elapsed() >= Duration::from_secs(15);
    }
    last_sent.elapsed() >= Duration::from_secs(3)
}

fn status_command_waiting_for_enter(output: &str) -> bool {
    output.contains("› /status") || output.contains("> /status")
}

fn codex_status_output_ready(output: &str) -> bool {
    let parsed = parse_status_text_with_source(output, ParseClock::now(), SnapshotSource::CodexCli);
    parsed.snapshot.five_hour.remaining_percent.is_some()
        && parsed.snapshot.weekly.remaining_percent.is_some()
}

fn command_list_contains(help: &str, command: &str) -> bool {
    help.lines().any(|line| {
        let trimmed = line.trim_start().to_ascii_lowercase();
        let Some(rest) = trimmed.strip_prefix(command) else {
            return false;
        };
        rest.starts_with(char::is_whitespace)
    })
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

fn to_command_error(error: StoreError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use crate::commands::{
        adaptive_refresh_interval, command_list_contains, failure_backoff_interval,
        next_auto_refresh_schedule, AUTO_BASE_REFRESH_INTERVAL, AUTO_LOW_USAGE_REFRESH_INTERVAL,
        AUTO_POST_RESET_REFRESH_DELAY, AUTO_RESET_WATCH_WINDOW,
    };
    use crate::models::{
        AppState, QuotaReading, QuotaSnapshot, RefreshUsageResult, SnapshotSource, StorageStatus,
        STATE_VERSION,
    };
    use std::time::Duration;

    fn app_state(snapshot: QuotaSnapshot) -> AppState {
        AppState {
            version: STATE_VERSION,
            latest_snapshot: Some(snapshot),
            storage_status: StorageStatus::Ready,
            storage_path: None,
            backup_path: None,
            status_message: "已通过 Codex CLI 更新额度。".to_string(),
        }
    }

    fn refresh_result(snapshot: QuotaSnapshot) -> RefreshUsageResult {
        RefreshUsageResult {
            app_state: app_state(snapshot),
            updated: true,
            message: "已通过 Codex CLI 更新额度。".to_string(),
        }
    }

    fn snapshot(
        five_hour_percent: u8,
        weekly_percent: u8,
        reset_countdown_seconds: Option<i64>,
    ) -> QuotaSnapshot {
        QuotaSnapshot {
            id: "snap-1".to_string(),
            source: SnapshotSource::CodexCli,
            captured_at: "unix:1000".to_string(),
            five_hour: QuotaReading {
                remaining_percent: Some(five_hour_percent),
                reset_at: None,
                reset_countdown_seconds,
            },
            weekly: QuotaReading {
                remaining_percent: Some(weekly_percent),
                reset_at: None,
                reset_countdown_seconds: None,
            },
            raw_text: String::new(),
            status_message: "已通过 Codex CLI 更新额度。".to_string(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn detects_usage_command_as_top_level_command_only() {
        let help = "Commands:\n  usage   Show quota\n  login   Manage login";

        assert!(command_list_contains(help, "usage"));
        assert!(!command_list_contains(help, "status"));
    }

    #[test]
    fn does_not_treat_login_status_as_status_command() {
        let help = "Commands:\n  login   Manage login status\n  doctor  Diagnose";

        assert!(!command_list_contains(help, "status"));
    }

    #[test]
    fn detects_windows_cmd_shims() {
        assert!(super::is_cmd_shim(std::path::Path::new("codex.cmd")));
        assert!(super::is_cmd_shim(std::path::Path::new("codex.bat")));
        assert!(!super::is_cmd_shim(std::path::Path::new("codex.exe")));
    }

    #[test]
    fn recognizes_interactive_status_output() {
        let output = "5h limit: [======] 44% left (resets 22:04)\nWeekly limit: [======] 59% left (resets 07:00 on 25 Jun)";

        assert!(super::codex_status_output_ready(output));
    }

    #[test]
    fn auto_refresh_keeps_base_interval_for_healthy_usage() {
        let state = app_state(snapshot(75, 64, Some(3600)));

        assert_eq!(
            adaptive_refresh_interval(&state),
            AUTO_BASE_REFRESH_INTERVAL
        );
    }

    #[test]
    fn auto_refresh_accelerates_when_usage_is_low() {
        let outcome = Ok(refresh_result(snapshot(20, 64, Some(3600))));

        let schedule = next_auto_refresh_schedule(&outcome, 2);

        assert_eq!(schedule.delay, AUTO_LOW_USAGE_REFRESH_INTERVAL);
        assert_eq!(schedule.consecutive_failures, 0);
    }

    #[test]
    fn auto_refresh_schedules_after_imminent_reset() {
        let reset_in = Duration::from_secs(42);
        let state = app_state(snapshot(75, 64, Some(reset_in.as_secs() as i64)));

        assert_eq!(
            adaptive_refresh_interval(&state),
            reset_in + AUTO_POST_RESET_REFRESH_DELAY
        );
    }

    #[test]
    fn auto_refresh_ignores_distant_reset_countdown() {
        let reset_after_watch_window = AUTO_RESET_WATCH_WINDOW + Duration::from_secs(1);
        let state = app_state(snapshot(
            75,
            64,
            Some(reset_after_watch_window.as_secs() as i64),
        ));

        assert_eq!(
            adaptive_refresh_interval(&state),
            AUTO_BASE_REFRESH_INTERVAL
        );
    }

    #[test]
    fn auto_refresh_uses_failure_backoff_for_unsuccessful_results() {
        let state = AppState {
            version: STATE_VERSION,
            latest_snapshot: Some(snapshot(10, 64, Some(30))),
            storage_status: StorageStatus::Ready,
            storage_path: None,
            backup_path: None,
            status_message: "Codex CLI 额度查询失败，请稍后重试。".to_string(),
        };
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
    #[ignore]
    fn captures_real_codex_status_with_pty() {
        let output = super::run_codex_status_pty(std::time::Duration::from_secs(20)).unwrap();

        assert!(super::codex_status_output_ready(&output));
    }
}
