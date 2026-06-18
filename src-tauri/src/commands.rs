use crate::models::{AppState, QuotaSnapshot, RefreshUsageResult, SnapshotSource};
use crate::status_parser::{
    parse_status_text as parse_status_text_impl, parse_status_text_with_source, ParseClock,
    ParseResult,
};
use crate::usage_store::{StoreError, UsageStore};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub fn get_app_state(app: AppHandle) -> Result<AppState, String> {
    let state = load_app_state(&app)?;
    sync_tray(&app, &state);
    Ok(state)
}

#[tauri::command]
pub fn parse_status_text(raw_text: String) -> ParseResult {
    parse_status_text_impl(&raw_text, ParseClock::now())
}

#[tauri::command]
pub fn save_snapshot(app: AppHandle, snapshot: QuotaSnapshot) -> Result<AppState, String> {
    let store = store_for_app(&app)?;
    let app_state = store
        .save_snapshot(snapshot)
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)?;
    sync_tray(&app, &app_state);
    Ok(app_state)
}

#[tauri::command]
pub fn clear_snapshot(app: AppHandle) -> Result<AppState, String> {
    let store = store_for_app(&app)?;
    let app_state = store
        .clear_snapshot()
        .map(|outcome| outcome.into_app_state())
        .map_err(to_command_error)?;
    sync_tray(&app, &app_state);
    Ok(app_state)
}

#[tauri::command]
pub async fn refresh_usage(app: AppHandle) -> Result<RefreshUsageResult, String> {
    let worker_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || refresh_usage_blocking(worker_app))
        .await
        .map_err(|error| format!("后台查询任务失败：{error}"))??;
    sync_tray(&app, &result.app_state);
    Ok(result)
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
    let help = run_codex(&["--help"], Duration::from_secs(3))?;
    if !help.success {
        return Err("Codex CLI 无法执行，请确认 codex 已安装并可登录。".to_string());
    }

    let output = if command_list_contains(&help.stdout, "usage") {
        let output = run_codex(&["usage", "--json"], Duration::from_secs(8))?;
        if !output.success {
            return Err("Codex CLI 额度查询失败，请粘贴 /status。".to_string());
        }
        output.stdout
    } else if command_list_contains(&help.stdout, "status") {
        let output = run_codex(&["status", "--json"], Duration::from_secs(8))?;
        if !output.success {
            return Err("Codex CLI 额度查询失败，请粘贴 /status。".to_string());
        }
        output.stdout
    } else {
        run_codex_status_pty(Duration::from_secs(75))?
    };

    let mut result =
        parse_status_text_with_source(&output, ParseClock::now(), SnapshotSource::CodexCli);
    result.snapshot.status_message = "已通过 Codex CLI 更新额度。".to_string();
    if result.snapshot.has_any_usage() {
        Ok(result.snapshot)
    } else {
        Err("Codex CLI 没有返回可识别的额度，请粘贴 /status。".to_string())
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
        Err("自动查询当前仅支持 Windows，请粘贴 /status。".to_string())
    }
}

#[cfg(windows)]
mod windows_conpty {
    use super::{codex_status_output_ready, is_cmd_shim, should_send_status_command};
    use std::ffi::{c_void, OsStr};
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::{null, null_mut};
    use std::thread;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, HANDLE, HANDLE_FLAG_INHERIT, HMODULE, WAIT_OBJECT_0,
        WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    use windows_sys::Win32::System::Console::{COORD, HPCON};
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::System::Pipes::{CreatePipe, PeekNamedPipe};
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
        TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW,
        EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
        PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTUPINFOEXW, STARTUPINFOW,
    };

    pub fn capture_status(target: &Path, timeout: Duration) -> Result<String, String> {
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
                0,
                &mut hpc,
            );
            if hr < 0 {
                return Err(format!("创建 Codex 伪终端失败：HRESULT 0x{hr:08X}"));
            }
            let pseudo_console = PseudoConsole {
                hpc,
                close: conpty.close,
            };
            drop(input_read);
            drop(output_write);

            let mut attributes = AttributeList::new(pseudo_console.raw())?;
            let mut startup: STARTUPINFOEXW = zeroed();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
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
                EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
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

            let started = Instant::now();
            let mut output = Vec::new();
            let mut sent_count = 0_u8;
            let mut last_sent = started;

            loop {
                read_available(output_read.raw(), &mut output)?;
                let text = String::from_utf8_lossy(&output).to_string();

                if should_send_status_command(&text, started, last_sent, sent_count) {
                    write_all(input_write.raw(), b"/status\r")?;
                    sent_count += 1;
                    last_sent = Instant::now();
                }

                if sent_count > 0 && codex_status_output_ready(&text) {
                    process.terminate();
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
            read_available(output_read.raw(), &mut output)?;
            let text = String::from_utf8_lossy(&output).to_string();
            if codex_status_output_ready(&text) {
                return Ok(text);
            }
            Err("Codex CLI /status 自动查询失败，请粘贴 /status。".to_string())
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

    unsafe fn read_available(handle: HANDLE, output: &mut Vec<u8>) -> Result<(), String> {
        let mut available = 0_u32;
        if PeekNamedPipe(
            handle,
            null_mut(),
            0,
            null_mut(),
            &mut available,
            null_mut(),
        ) == 0
        {
            return Ok(());
        }

        while available > 0 {
            let mut buffer = vec![0_u8; available.min(4096) as usize];
            let mut read = 0_u32;
            if ReadFile(
                handle,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut read,
                null_mut(),
            ) == 0
            {
                return Ok(());
            }
            if read == 0 {
                return Ok(());
            }
            output.extend_from_slice(&buffer[..read as usize]);
            available = available.saturating_sub(read);
        }

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

    #[derive(Default)]
    struct Handle(HANDLE);

    impl Handle {
        fn raw(&self) -> HANDLE {
            self.0
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
    }
}

fn should_send_status_command(
    output: &str,
    started: Instant,
    last_sent: Instant,
    sent_count: u8,
) -> bool {
    if sent_count >= 8 {
        return false;
    }
    if sent_count == 0 {
        return started.elapsed() >= Duration::from_millis(1800) || codex_prompt_ready(output);
    }
    last_sent.elapsed() >= Duration::from_secs(5)
}

fn codex_prompt_ready(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("implement {feature}")
        || (lower.contains("model:") && lower.contains("directory:"))
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
            return Err("Codex CLI 查询超时，请粘贴 /status。".to_string());
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

fn to_command_error(error: StoreError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use crate::commands::command_list_contains;

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
    #[ignore]
    fn captures_real_codex_status_with_pty() {
        let output = super::run_codex_status_pty(std::time::Duration::from_secs(20)).unwrap();

        assert!(super::codex_status_output_ready(&output));
    }
}
