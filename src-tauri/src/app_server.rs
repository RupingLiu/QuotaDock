use crate::models::{ParseWarning, QuotaReading, QuotaSnapshot, SnapshotSource};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const INITIALIZE_REQUEST_ID: u64 = 1;
const RATE_LIMITS_REQUEST_ID: u64 = 2;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitsResult {
    rate_limits: Option<RateLimitBucket>,
    #[serde(default)]
    rate_limits_by_limit_id: std::collections::HashMap<String, RateLimitBucket>,
    rate_limit_reset_credits: Option<ResetCredits>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitBucket {
    #[allow(dead_code)]
    limit_id: String,
    primary: Option<RateLimitWindow>,
    secondary: Option<RateLimitWindow>,
    credits: Option<Credits>,
    plan_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitWindow {
    used_percent: f64,
    window_duration_mins: i64,
    resets_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Credits {
    balance: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResetCredits {
    available_count: u32,
}

pub fn fetch_rate_limits(
    mut command: Command,
    timeout: Duration,
    app_version: &str,
) -> Result<QuotaSnapshot, String> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动 Codex app-server 失败：{error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取 Codex app-server 输出。".to_string())?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "无法写入 Codex app-server。".to_string())?;
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("quotadock-app-server-reader".to_string())
        .spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if sender.send(line).is_err() {
                    break;
                }
            }
        })
        .map_err(|error| format!("启动 app-server 读取线程失败：{error}"))?;

    let mut guard = ChildGuard::new(child);
    write_message(
        &mut stdin,
        &json!({
            "method": "initialize",
            "id": INITIALIZE_REQUEST_ID,
            "params": {
                "clientInfo": {
                    "name": "quotadock",
                    "title": "QuotaDock",
                    "version": app_version
                }
            }
        }),
    )?;

    let started = Instant::now();
    wait_for_response(
        &receiver,
        guard.child_mut(),
        INITIALIZE_REQUEST_ID,
        started,
        timeout,
    )?;
    write_message(
        &mut stdin,
        &json!({ "method": "initialized", "params": {} }),
    )?;
    write_message(
        &mut stdin,
        &json!({ "method": "account/rateLimits/read", "id": RATE_LIMITS_REQUEST_ID }),
    )?;
    let response = wait_for_response(
        &receiver,
        guard.child_mut(),
        RATE_LIMITS_REQUEST_ID,
        started,
        timeout,
    )?;
    guard.finish();
    parse_rate_limits_value(response, unix_timestamp_string())
}

fn wait_for_response(
    receiver: &mpsc::Receiver<Result<String, std::io::Error>>,
    child: &mut Child,
    request_id: u64,
    started: Instant,
    timeout: Duration,
) -> Result<Value, String> {
    loop {
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            return Err("Codex app-server 查询超时。".to_string());
        }
        let wait = (timeout - elapsed).min(Duration::from_millis(250));
        match receiver.recv_timeout(wait) {
            Ok(Ok(line)) => {
                let message: Value = serde_json::from_str(&line)
                    .map_err(|error| format!("Codex app-server 返回了无效 JSON：{error}"))?;
                if message.get("id").and_then(Value::as_u64) != Some(request_id) {
                    continue;
                }
                if let Some(error) = message.get("error") {
                    let detail = error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("未知错误");
                    return Err(format!("Codex app-server 请求失败：{detail}"));
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| "Codex app-server 响应缺少 result。".to_string());
            }
            Ok(Err(error)) => return Err(format!("读取 Codex app-server 失败：{error}")),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if child
                    .try_wait()
                    .map_err(|error| format!("检查 Codex app-server 状态失败：{error}"))?
                    .is_some()
                {
                    return Err("Codex app-server 在返回额度前退出。".to_string());
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Codex app-server 输出已关闭。".to_string());
            }
        }
    }
}

fn write_message(stdin: &mut impl Write, message: &Value) -> Result<(), String> {
    serde_json::to_writer(&mut *stdin, message)
        .map_err(|error| format!("编码 Codex app-server 请求失败：{error}"))?;
    stdin
        .write_all(b"\n")
        .and_then(|_| stdin.flush())
        .map_err(|error| format!("写入 Codex app-server 失败：{error}"))
}

fn parse_rate_limits_value(result: Value, captured_at: String) -> Result<QuotaSnapshot, String> {
    let mut result: RateLimitsResult = serde_json::from_value(result)
        .map_err(|error| format!("解析 Codex app-server 额度失败：{error}"))?;
    let bucket = result
        .rate_limits_by_limit_id
        .remove("codex")
        .or(result.rate_limits)
        .ok_or_else(|| "Codex app-server 没有返回 codex 额度桶。".to_string())?;
    let mut five_hour = QuotaReading::default();
    let mut weekly = QuotaReading::default();
    for window in [bucket.primary.as_ref(), bucket.secondary.as_ref()]
        .into_iter()
        .flatten()
    {
        assign_window(window, &mut five_hour, &mut weekly);
    }

    let mut warnings = Vec::new();
    if !five_hour.has_usage() {
        warnings.push(warning(
            "missing-five-hour",
            "当前账户没有返回短周期额度窗口。",
        ));
    }
    if !weekly.has_usage() {
        warnings.push(warning(
            "missing-weekly",
            "当前账户没有返回长周期额度窗口。",
        ));
    }
    if !five_hour.has_usage() && !weekly.has_usage() {
        return Err("Codex app-server 没有返回可用的额度百分比。".to_string());
    }
    let status_message = if warnings.is_empty() {
        "已通过 Codex app-server 更新全部额度。".to_string()
    } else {
        "已通过 Codex app-server 更新当前账户提供的额度窗口。".to_string()
    };

    Ok(QuotaSnapshot {
        id: captured_at.clone(),
        source: SnapshotSource::CodexAppServer,
        captured_at,
        five_hour,
        weekly,
        plan_type: bucket.plan_type,
        credits_balance: bucket.credits.and_then(|credits| credits.balance),
        reset_credits_available: result
            .rate_limit_reset_credits
            .map(|credits| credits.available_count),
        raw_text: String::new(),
        status_message,
        warnings,
    })
}

fn assign_window(
    window: &RateLimitWindow,
    five_hour: &mut QuotaReading,
    weekly: &mut QuotaReading,
) {
    let reading = QuotaReading {
        remaining_percent: Some(remaining_percent(window.used_percent)),
        reset_at: Some(format!("unix:{}", window.resets_at)),
        reset_countdown_seconds: None,
    };
    let five_hour_distance = (window.window_duration_mins - 5 * 60).abs();
    let weekly_distance = (window.window_duration_mins - 7 * 24 * 60).abs();
    if five_hour_distance <= weekly_distance {
        if five_hour.remaining_percent.is_none() {
            *five_hour = reading;
        }
    } else if weekly.remaining_percent.is_none() {
        *weekly = reading;
    }
}

fn remaining_percent(used_percent: f64) -> u8 {
    (100.0 - used_percent).round().clamp(0.0, 100.0) as u8
}

fn warning(code: &str, message: &str) -> ParseWarning {
    ParseWarning {
        code: code.to_string(),
        message: message.to_string(),
    }
}

fn unix_timestamp_string() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{seconds}")
}

struct ChildGuard {
    child: Child,
    finished: bool,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self {
            child,
            finished: false,
        }
    }

    fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    fn finish(&mut self) {
        self.finished = true;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.finish();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_rate_limits_value, remaining_percent};
    use crate::models::SnapshotSource;
    use serde_json::json;

    #[test]
    fn maps_structured_rate_limits_by_window_duration() {
        let snapshot = parse_rate_limits_value(
            json!({
                "rateLimits": {
                    "limitId": "codex",
                    "primary": {
                        "usedPercent": 25,
                        "windowDurationMins": 300,
                        "resetsAt": 1_800_000_000
                    },
                    "secondary": {
                        "usedPercent": 68,
                        "windowDurationMins": 10_080,
                        "resetsAt": 1_800_100_000
                    },
                    "credits": { "balance": "12.5" },
                    "planType": "pro"
                },
                "rateLimitsByLimitId": {},
                "rateLimitResetCredits": { "availableCount": 2 }
            }),
            "unix:1000".to_string(),
        )
        .unwrap();

        assert_eq!(snapshot.source, SnapshotSource::CodexAppServer);
        assert_eq!(snapshot.five_hour.remaining_percent, Some(75));
        assert_eq!(snapshot.weekly.remaining_percent, Some(32));
        assert_eq!(snapshot.weekly.reset_at.as_deref(), Some("unix:1800100000"));
        assert_eq!(snapshot.plan_type.as_deref(), Some("pro"));
        assert_eq!(snapshot.credits_balance.as_deref(), Some("12.5"));
        assert_eq!(snapshot.reset_credits_available, Some(2));
    }

    #[test]
    fn accepts_accounts_with_only_a_weekly_window() {
        let snapshot = parse_rate_limits_value(
            json!({
                "rateLimits": {
                    "limitId": "codex",
                    "primary": {
                        "usedPercent": 68,
                        "windowDurationMins": 10_080,
                        "resetsAt": 1_800_100_000
                    },
                    "secondary": null,
                    "credits": null,
                    "planType": "prolite"
                },
                "rateLimitsByLimitId": {},
                "rateLimitResetCredits": null
            }),
            "unix:1000".to_string(),
        )
        .unwrap();

        assert_eq!(snapshot.five_hour.remaining_percent, None);
        assert_eq!(snapshot.weekly.remaining_percent, Some(32));
        assert!(snapshot
            .warnings
            .iter()
            .any(|warning| warning.code == "missing-five-hour"));
    }

    #[test]
    fn clamps_used_percent_when_computing_remaining() {
        assert_eq!(remaining_percent(-5.0), 100);
        assert_eq!(remaining_percent(25.4), 75);
        assert_eq!(remaining_percent(120.0), 0);
    }
}
