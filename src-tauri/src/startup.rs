#[cfg(windows)]
mod platform {
    use std::io;
    use std::path::{Path, PathBuf};
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "QuotaDock";

    pub fn is_enabled() -> Result<bool, String> {
        let current = current_exe()?;
        let Some(command) = read_command()? else {
            return Ok(false);
        };
        Ok(command_targets_current_exe(&command, &current))
    }

    pub fn set_enabled(enabled: bool) -> Result<bool, String> {
        if enabled {
            write_command(&format!("\"{}\"", current_exe()?.display()))?;
            Ok(true)
        } else {
            delete_command()?;
            Ok(false)
        }
    }

    pub fn toggle() -> Result<bool, String> {
        let next = !is_enabled()?;
        set_enabled(next)
    }

    fn current_exe() -> Result<PathBuf, String> {
        std::env::current_exe().map_err(|error| format!("读取 QuotaDock 程序路径失败：{error}"))
    }

    fn read_command() -> Result<Option<String>, String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ) {
            Ok(key) => key,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("读取开机自启动状态失败：{error}")),
        };

        match key.get_value::<String, _>(VALUE_NAME) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("读取开机自启动状态失败：{error}")),
        }
    }

    fn write_command(command: &str) -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey_with_flags(RUN_KEY, KEY_WRITE)
            .map_err(|error| format!("打开开机自启动注册表失败：{error}"))?;
        key.set_value(VALUE_NAME, &command)
            .map_err(|error| format!("设置开机自启动失败：{error}"))
    }

    fn delete_command() -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey_with_flags(RUN_KEY, KEY_WRITE) {
            Ok(key) => key,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("打开开机自启动注册表失败：{error}")),
        };

        match key.delete_value(VALUE_NAME) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("关闭开机自启动失败：{error}")),
        }
    }

    fn command_targets_current_exe(command: &str, current: &Path) -> bool {
        let target = first_command_token(command)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(command));
        same_path(&target, current)
    }

    fn first_command_token(command: &str) -> Option<String> {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return None;
        }

        if let Some(rest) = trimmed.strip_prefix('"') {
            return rest.split_once('"').map(|(path, _)| path.to_string());
        }

        trimmed.split_whitespace().next().map(str::to_string)
    }

    fn same_path(left: &Path, right: &Path) -> bool {
        match (left.canonicalize(), right.canonicalize()) {
            (Ok(left), Ok(right)) => left == right,
            _ => left
                .to_string_lossy()
                .eq_ignore_ascii_case(&right.to_string_lossy()),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::first_command_token;

        #[test]
        fn extracts_quoted_command_path() {
            assert_eq!(
                first_command_token(r#""C:\Program Files\QuotaDock\quotadock.exe" --startup"#),
                Some(r"C:\Program Files\QuotaDock\quotadock.exe".to_string())
            );
        }

        #[test]
        fn extracts_unquoted_command_path() {
            assert_eq!(
                first_command_token(r"C:\Tools\quotadock.exe --startup"),
                Some(r"C:\Tools\quotadock.exe".to_string())
            );
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn is_enabled() -> Result<bool, String> {
        Ok(false)
    }

    pub fn set_enabled(_enabled: bool) -> Result<bool, String> {
        Err("开机自启动当前仅支持 Windows。".to_string())
    }

    pub fn toggle() -> Result<bool, String> {
        Err("开机自启动当前仅支持 Windows。".to_string())
    }
}

pub use platform::{is_enabled, toggle};
