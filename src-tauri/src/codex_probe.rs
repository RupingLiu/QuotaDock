use crate::models::{CodexHealth, CodexProbeStatus};
use crate::redaction::redact_diagnostic;
#[cfg(test)]
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ALLOWED_COMMANDS: &[&[&str]] = &[&["--version"], &["login", "status"], &["doctor", "--json"]];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    stdout: String,
    stderr: String,
    status_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    NotFound,
    TimedOut,
    Rejected,
    Failed(String),
}

pub trait CommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[&str],
        timeout: Duration,
    ) -> Result<CommandOutput, CommandError>;
}

#[derive(Debug, Default)]
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[&str],
        timeout: Duration,
    ) -> Result<CommandOutput, CommandError> {
        if !is_allowlisted(program, args) {
            return Err(CommandError::Rejected);
        }

        let mut child = Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    CommandError::NotFound
                } else {
                    CommandError::Failed(error.to_string())
                }
            })?;

        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => break,
                Ok(None) if start.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CommandError::TimedOut);
                }
                Err(error) => return Err(CommandError::Failed(error.to_string())),
            }
        }

        let output = child
            .wait_with_output()
            .map_err(|error| CommandError::Failed(error.to_string()))?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            status_code: output.status.code(),
        })
    }
}

#[cfg(test)]
#[derive(Debug, Default, Clone)]
pub struct MockCommandRunner {
    responses: HashMap<String, Result<CommandOutput, CommandError>>,
}

#[cfg(test)]
impl MockCommandRunner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn success(mut self, program: &str, args: &[&str], stdout: &str, stderr: &str) -> Self {
        self.responses.insert(
            command_key(program, args),
            Ok(CommandOutput {
                stdout: stdout.to_string(),
                stderr: stderr.to_string(),
                status_code: Some(0),
            }),
        );
        self
    }

    pub fn failure(
        mut self,
        program: &str,
        args: &[&str],
        status_code: i32,
        stdout: &str,
        stderr: &str,
    ) -> Self {
        self.responses.insert(
            command_key(program, args),
            Ok(CommandOutput {
                stdout: stdout.to_string(),
                stderr: stderr.to_string(),
                status_code: Some(status_code),
            }),
        );
        self
    }

    pub fn not_found(mut self, program: &str, args: &[&str]) -> Self {
        self.responses
            .insert(command_key(program, args), Err(CommandError::NotFound));
        self
    }

    pub fn timeout(mut self, program: &str, args: &[&str]) -> Self {
        self.responses
            .insert(command_key(program, args), Err(CommandError::TimedOut));
        self
    }
}

#[cfg(test)]
impl CommandRunner for MockCommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[&str],
        _timeout: Duration,
    ) -> Result<CommandOutput, CommandError> {
        if !is_allowlisted(program, args) {
            return Err(CommandError::Rejected);
        }
        self.responses
            .get(&command_key(program, args))
            .cloned()
            .unwrap_or(Err(CommandError::NotFound))
    }
}

pub fn probe_codex(timeout: Duration) -> CodexHealth {
    probe_codex_with_runner(&SystemCommandRunner, timeout)
}

pub fn probe_codex_with_runner(runner: &impl CommandRunner, timeout: Duration) -> CodexHealth {
    let mut diagnostics = Vec::new();

    let version_output = match run_probe_command(runner, &["--version"], timeout) {
        Ok(output) if output.status_code == Some(0) => output,
        Ok(output) => {
            diagnostics.push(format_command_failure("codex --version", &output));
            return health(
                CodexProbeStatus::Unavailable,
                false,
                None,
                None,
                None,
                diagnostics,
            );
        }
        Err(error) => {
            diagnostics.push(format_command_error("codex --version", error));
            return health(
                CodexProbeStatus::Unavailable,
                false,
                None,
                None,
                None,
                diagnostics,
            );
        }
    };

    let version = non_empty(redact_diagnostic(&version_output.stdout));
    let login_output = run_probe_command(runner, &["login", "status"], timeout);
    let authenticated = match login_output {
        Ok(output) if output.status_code == Some(0) => {
            push_output_diagnostics(&mut diagnostics, "codex login status", &output);
            if login_output_indicates_signed_out(&output) {
                Some(false)
            } else {
                Some(true)
            }
        }
        Ok(output) => {
            diagnostics.push(format_command_failure("codex login status", &output));
            if login_output_indicates_signed_out(&output) {
                Some(false)
            } else {
                None
            }
        }
        Err(error) => {
            diagnostics.push(format_command_error("codex login status", error));
            None
        }
    };

    let doctor_output = run_probe_command(runner, &["doctor", "--json"], timeout);
    let mut doctor_status = None;
    let doctor_ok = match doctor_output {
        Ok(output) if output.status_code == Some(0) => {
            if let Some(status) = parse_doctor_status(&output.stdout) {
                let ok = status == "ok";
                doctor_status = Some(status);
                push_output_diagnostics(&mut diagnostics, "codex doctor --json", &output);
                ok
            } else {
                diagnostics.push("codex doctor returned invalid JSON".to_string());
                push_output_diagnostics(&mut diagnostics, "codex doctor --json", &output);
                false
            }
        }
        Ok(output) => {
            diagnostics.push(format_command_failure("codex doctor --json", &output));
            false
        }
        Err(error) => {
            diagnostics.push(format_command_error("codex doctor --json", error));
            false
        }
    };

    let status = if authenticated == Some(false) {
        CodexProbeStatus::NotAuthenticated
    } else if authenticated == Some(true) && doctor_ok {
        CodexProbeStatus::Healthy
    } else {
        CodexProbeStatus::Warning
    };

    health(
        status,
        true,
        authenticated,
        version,
        doctor_status,
        diagnostics,
    )
}

fn run_probe_command(
    runner: &impl CommandRunner,
    args: &[&str],
    timeout: Duration,
) -> Result<CommandOutput, CommandError> {
    runner.run("codex", args, timeout)
}

fn health(
    status: CodexProbeStatus,
    available: bool,
    authenticated: Option<bool>,
    version: Option<String>,
    doctor_status: Option<String>,
    diagnostics: Vec<String>,
) -> CodexHealth {
    CodexHealth {
        status,
        available,
        authenticated,
        version,
        doctor_status,
        checked_at: Some(unix_timestamp_string()),
        diagnostics: diagnostics
            .into_iter()
            .map(|line| redact_diagnostic(&line))
            .collect(),
    }
}

fn is_allowlisted(program: &str, args: &[&str]) -> bool {
    program == "codex" && ALLOWED_COMMANDS.iter().any(|allowed| *allowed == args)
}

#[cfg(test)]
fn command_key(program: &str, args: &[&str]) -> String {
    format!("{program} {}", args.join(" "))
}

fn format_command_error(command: &str, error: CommandError) -> String {
    match error {
        CommandError::NotFound => format!("{command}: codex executable not found"),
        CommandError::TimedOut => format!("{command}: command timed out"),
        CommandError::Rejected => format!("{command}: command rejected by allowlist"),
        CommandError::Failed(message) => format!("{command}: {message}"),
    }
}

fn format_command_failure(command: &str, output: &CommandOutput) -> String {
    let details = [output.stdout.as_str(), output.stderr.as_str()]
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "{command}: exited with status {:?} {}",
        output.status_code, details
    )
}

fn push_output_diagnostics(diagnostics: &mut Vec<String>, command: &str, output: &CommandOutput) {
    if !output.stderr.trim().is_empty() {
        diagnostics.push(format!("{command}: {}", output.stderr));
    }
}

fn login_output_indicates_signed_out(output: &CommandOutput) -> bool {
    let details = format!("{} {}", output.stdout, output.stderr).to_ascii_lowercase();
    [
        "not logged",
        "not signed",
        "signed out",
        "login required",
        "not authenticated",
        "unauthenticated",
    ]
    .iter()
    .any(|marker| details.contains(marker))
}

fn parse_doctor_status(output: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(output).ok()?;
    value
        .get("status")
        .and_then(|status| status.as_str())
        .map(ToString::to_string)
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn unix_timestamp_string() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{seconds}")
}

#[cfg(test)]
mod tests {
    use crate::codex_probe::{
        probe_codex_with_runner, CommandError, CommandRunner, MockCommandRunner,
    };
    use crate::models::CodexProbeStatus;
    use std::time::Duration;

    #[test]
    fn reports_codex_missing_when_version_command_not_found() {
        let runner = MockCommandRunner::new().not_found("codex", &["--version"]);

        let health = probe_codex_with_runner(&runner, Duration::from_secs(1));

        assert_eq!(health.status, CodexProbeStatus::Unavailable);
        assert!(!health.available);
        assert!(health
            .diagnostics
            .iter()
            .any(|line| line.contains("not found")));
    }

    #[test]
    fn reports_signed_in_success_with_doctor_json() {
        let runner = MockCommandRunner::new()
            .success("codex", &["--version"], "codex-cli 1.2.3", "")
            .success(
                "codex",
                &["login", "status"],
                "Signed in as user@example.com",
                "",
            )
            .success("codex", &["doctor", "--json"], r#"{"status":"ok"}"#, "");

        let health = probe_codex_with_runner(&runner, Duration::from_secs(1));

        assert_eq!(health.status, CodexProbeStatus::Healthy);
        assert!(health.available);
        assert_eq!(health.authenticated, Some(true));
        assert_eq!(health.version.as_deref(), Some("codex-cli 1.2.3"));
        assert_eq!(health.doctor_status.as_deref(), Some("ok"));
        assert!(!health.diagnostics.join("\n").contains("user@example.com"));
    }

    #[test]
    fn reports_not_authenticated_from_login_status() {
        let runner = MockCommandRunner::new()
            .success("codex", &["--version"], "codex-cli 1.2.3", "")
            .failure("codex", &["login", "status"], 1, "", "not logged in")
            .success("codex", &["doctor", "--json"], r#"{"status":"ok"}"#, "");

        let health = probe_codex_with_runner(&runner, Duration::from_secs(1));

        assert_eq!(health.status, CodexProbeStatus::NotAuthenticated);
        assert_eq!(health.authenticated, Some(false));
    }

    #[test]
    fn reports_warning_when_login_status_times_out() {
        let runner = MockCommandRunner::new()
            .success("codex", &["--version"], "codex-cli 1.2.3", "")
            .timeout("codex", &["login", "status"])
            .success("codex", &["doctor", "--json"], r#"{"status":"ok"}"#, "");

        let health = probe_codex_with_runner(&runner, Duration::from_secs(1));

        assert_eq!(health.status, CodexProbeStatus::Warning);
        assert_eq!(health.authenticated, None);
        assert!(health
            .diagnostics
            .iter()
            .any(|line| line.contains("login status") && line.contains("timed out")));
    }

    #[test]
    fn reports_warning_for_ambiguous_login_status_failure() {
        let runner = MockCommandRunner::new()
            .success("codex", &["--version"], "codex-cli 1.2.3", "")
            .failure("codex", &["login", "status"], 2, "", "network error")
            .success("codex", &["doctor", "--json"], r#"{"status":"ok"}"#, "");

        let health = probe_codex_with_runner(&runner, Duration::from_secs(1));

        assert_eq!(health.status, CodexProbeStatus::Warning);
        assert_eq!(health.authenticated, None);
    }

    #[test]
    fn reports_warning_from_doctor_warning_json() {
        let runner = MockCommandRunner::new()
            .success("codex", &["--version"], "codex-cli 1.2.3", "")
            .success("codex", &["login", "status"], "Signed in", "")
            .success(
                "codex",
                &["doctor", "--json"],
                r#"{"status":"warning"}"#,
                "",
            );

        let health = probe_codex_with_runner(&runner, Duration::from_secs(1));

        assert_eq!(health.status, CodexProbeStatus::Warning);
        assert_eq!(health.doctor_status.as_deref(), Some("warning"));
    }

    #[test]
    fn reports_warning_for_invalid_doctor_json() {
        let runner = MockCommandRunner::new()
            .success("codex", &["--version"], "codex-cli 1.2.3", "")
            .success("codex", &["login", "status"], "Signed in", "")
            .success("codex", &["doctor", "--json"], "not json", "");

        let health = probe_codex_with_runner(&runner, Duration::from_secs(1));

        assert_eq!(health.status, CodexProbeStatus::Warning);
        assert!(health
            .diagnostics
            .iter()
            .any(|line| line.contains("doctor returned invalid JSON")));
    }

    #[test]
    fn reports_warning_for_doctor_command_failure() {
        let runner = MockCommandRunner::new()
            .success("codex", &["--version"], "codex-cli 1.2.3", "")
            .success("codex", &["login", "status"], "Signed in", "")
            .failure("codex", &["doctor", "--json"], 2, "", "doctor failed");

        let health = probe_codex_with_runner(&runner, Duration::from_secs(1));

        assert_eq!(health.status, CodexProbeStatus::Warning);
        assert!(health
            .diagnostics
            .iter()
            .any(|line| line.contains("doctor failed")));
    }

    #[test]
    fn reports_timeout_without_hanging() {
        let runner = MockCommandRunner::new().timeout("codex", &["--version"]);

        let health = probe_codex_with_runner(&runner, Duration::from_millis(1));

        assert_eq!(health.status, CodexProbeStatus::Unavailable);
        assert!(health
            .diagnostics
            .iter()
            .any(|line| line.contains("timed out")));
    }

    #[test]
    fn rejects_non_allowlisted_commands_in_runner() {
        let runner = MockCommandRunner::new().success("codex", &["auth", "token"], "secret", "");

        let result = runner.run("codex", &["auth", "token"], Duration::from_secs(1));

        assert_eq!(result, Err(CommandError::Rejected));
    }
}
