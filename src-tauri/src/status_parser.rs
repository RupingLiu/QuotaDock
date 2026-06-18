use crate::models::{ParseWarning, QuotaReading, QuotaSnapshot, SnapshotSource};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseResult {
    pub snapshot: QuotaSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseClock {
    captured_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuotaKind {
    FiveHour,
    Weekly,
}

impl ParseClock {
    pub fn now() -> Self {
        Self {
            captured_at: unix_timestamp_string(),
        }
    }

    #[cfg(test)]
    pub fn fixed(captured_at: &str) -> Self {
        Self {
            captured_at: captured_at.to_string(),
        }
    }
}

#[cfg(test)]
pub fn parse_status_text(raw_text: &str, clock: ParseClock) -> ParseResult {
    parse_status_text_with_source(raw_text, clock, SnapshotSource::PastedStatus)
}

pub fn parse_status_text_with_source(
    raw_text: &str,
    clock: ParseClock,
    source: SnapshotSource,
) -> ParseResult {
    let mut five_hour = QuotaReading::default();
    let mut weekly = QuotaReading::default();
    let mut active_window: Option<QuotaKind> = None;
    let mut unknown_lines = Vec::new();

    for line in raw_text.lines() {
        let cleaned = clean_terminal_line(line);
        let trimmed = cleaned.trim();
        if trimmed.is_empty() || is_generic_status_header(trimmed) {
            continue;
        }

        let labels = detect_windows(trimmed);
        if labels.len() == 1 {
            active_window = labels.first().copied();
        }

        let targets = if !labels.is_empty() {
            labels
        } else if line_has_quota_value(trimmed) {
            active_window.into_iter().collect()
        } else {
            Vec::new()
        };

        let mut matched = !targets.is_empty();
        for target in targets {
            let reading = match target {
                QuotaKind::FiveHour => &mut five_hour,
                QuotaKind::Weekly => &mut weekly,
            };

            if reading.remaining_percent.is_none() {
                if let Some(percent) = extract_percent(trimmed) {
                    reading.remaining_percent = Some(percent);
                    matched = true;
                }
            }
            if reading.reset_at.is_none() && reading.reset_countdown_seconds.is_none() {
                if let Some(reset_at) = extract_reset_at(trimmed) {
                    reading.reset_at = Some(reset_at);
                    matched = true;
                } else if let Some(seconds) = extract_countdown(trimmed) {
                    reading.reset_countdown_seconds = Some(seconds);
                    matched = true;
                }
            }
        }

        if !matched {
            unknown_lines.push(trimmed.to_string());
        }
    }

    let mut warnings = Vec::new();
    if !five_hour.has_value() {
        warnings.push(warning("missing-five-hour", "未识别到 5 小时额度。"));
    }
    if !weekly.has_value() {
        warnings.push(warning("missing-weekly", "未识别到 1 周额度。"));
    }
    if !unknown_lines.is_empty() && (five_hour.has_value() || weekly.has_value()) {
        warnings.push(warning("unknown-lines", "部分粘贴内容未被识别，已忽略。"));
    }
    if !five_hour.has_value() && !weekly.has_value() {
        warnings.push(warning("no-quota-fields", "没有找到可用的额度信息。"));
    }

    let status_message = if warnings
        .iter()
        .any(|warning| warning.code == "no-quota-fields")
    {
        "没有识别到 5 小时或 1 周额度，请检查 /status 内容。".to_string()
    } else if warnings.is_empty() {
        "已更新 5 小时与 1 周额度。".to_string()
    } else {
        "已更新可识别的额度，部分字段缺失。".to_string()
    };

    ParseResult {
        snapshot: QuotaSnapshot {
            id: clock.captured_at.clone(),
            source,
            captured_at: clock.captured_at,
            five_hour,
            weekly,
            raw_text: raw_text.to_string(),
            status_message,
            warnings,
        },
    }
}

fn clean_terminal_line(line: &str) -> String {
    let mut output = String::new();
    let bytes = line.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == 0x1b {
            index += 1;
            if index < bytes.len() && bytes[index] == b'[' {
                index += 1;
                while index < bytes.len() && !(0x40..=0x7e).contains(&bytes[index]) {
                    index += 1;
                }
                index += 1;
                continue;
            }
            if index < bytes.len() && bytes[index] == b']' {
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == 0x07 {
                        index += 1;
                        break;
                    }
                    if bytes[index] == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b'\\'
                    {
                        index += 2;
                        break;
                    }
                    index += 1;
                }
                continue;
            }
            index += 1;
            continue;
        }

        let Some(character) = line[index..].chars().next() else {
            break;
        };
        if !character.is_control() || character == '\t' {
            output.push(character);
        }
        index += character.len_utf8();
    }

    output
}

fn detect_windows(line: &str) -> Vec<QuotaKind> {
    let lower = line.to_ascii_lowercase();
    let mut windows = Vec::new();

    if contains_any(
        &lower,
        &[
            "5h", "5 h", "5-hour", "5 hour", "5-hour", "5 hours", "5小时", "5 小时",
        ],
    ) {
        windows.push(QuotaKind::FiveHour);
    }
    if contains_any(
        &lower,
        &[
            "weekly",
            "1w",
            "1 w",
            "1-week",
            "1 week",
            "7d",
            "7 d",
            "week",
            "1周",
            "一周",
            "周额度",
        ],
    ) {
        windows.push(QuotaKind::Weekly);
    }

    windows
}

fn line_has_quota_value(line: &str) -> bool {
    line.contains('%') || contains_reset_keyword(line)
}

fn extract_percent(line: &str) -> Option<u8> {
    let hits = percent_hits(line);
    if hits.is_empty() {
        return None;
    }

    let lower = line.to_ascii_lowercase();
    let remaining_positions = keyword_positions(
        line,
        &["remaining", "left", "available", "remain", "剩余", "可用"],
    );
    if !remaining_positions.is_empty() {
        return hits
            .into_iter()
            .min_by_key(|hit| {
                remaining_positions
                    .iter()
                    .map(|position| hit.index.abs_diff(*position))
                    .min()
                    .unwrap_or(usize::MAX)
            })
            .map(|hit| hit.value);
    }

    if contains_any(&lower, &["used", "spent", "已用", "使用"]) {
        return None;
    }

    hits.first().map(|hit| hit.value)
}

#[derive(Debug, Clone, Copy)]
struct PercentHit {
    value: u8,
    index: usize,
}

fn percent_hits(line: &str) -> Vec<PercentHit> {
    let mut hits = Vec::new();
    let mut digits = String::new();
    let mut digit_start = 0_usize;

    for (index, character) in line.char_indices() {
        if character.is_ascii_digit() {
            if digits.is_empty() {
                digit_start = index;
            }
            digits.push(character);
            continue;
        }

        if character == '%' {
            if let Ok(value) = digits.parse::<u8>() {
                if value <= 100 {
                    hits.push(PercentHit {
                        value,
                        index: digit_start,
                    });
                }
            }
        }
        digits.clear();
    }

    hits
}

fn extract_reset_at(line: &str) -> Option<String> {
    if !contains_reset_keyword(line) {
        return None;
    }

    for token in line.split_whitespace() {
        let cleaned = token.trim_matches(|character: char| {
            character == ',' || character == ';' || character == ')' || character == '('
        });
        if let Some(value) = normalize_reset_timestamp(cleaned) {
            return Some(value);
        }
    }

    value_after_colon(line)
        .filter(|value| !value.contains('%') && !looks_like_countdown(value))
        .or_else(|| reset_phrase(line))
}

fn reset_phrase(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    for keyword in [
        "resets",
        "reset",
        "updates",
        "update",
        "refreshes",
        "refresh",
    ] {
        let Some(index) = lower.find(keyword) else {
            continue;
        };
        let segment = &line[index + keyword.len()..];
        let segment = segment
            .split(')')
            .next()
            .unwrap_or(segment)
            .trim()
            .trim_start_matches(|character: char| {
                character == ':' || character == '：' || character == '(' || character == ' '
            })
            .trim();
        if segment.is_empty()
            || segment.contains('%')
            || looks_like_countdown(segment)
            || segment.eq_ignore_ascii_case("at")
        {
            continue;
        }
        let segment = segment
            .strip_prefix("at:")
            .or_else(|| segment.strip_prefix("at："))
            .unwrap_or(segment)
            .trim();
        if !segment.is_empty() {
            return Some(segment.to_string());
        }
    }
    None
}

fn extract_countdown(line: &str) -> Option<i64> {
    if !contains_reset_keyword(line) && !line.contains('后') {
        return None;
    }

    let candidate = countdown_segment(line);
    let compact = strip_percent_segments(candidate)
        .replace(' ', "")
        .to_ascii_lowercase();
    let mut number = String::new();
    let mut seconds = 0_i64;

    for character in compact.chars() {
        if character.is_ascii_digit() {
            number.push(character);
            continue;
        }

        if number.is_empty() {
            continue;
        }

        let value = number.parse::<i64>().ok()?;
        match character {
            'd' | '天' => {
                seconds += value * 86_400;
                number.clear();
            }
            'h' | '时' => {
                seconds += value * 3_600;
                number.clear();
            }
            'm' | '分' => {
                seconds += value * 60;
                number.clear();
            }
            's' | '秒' => {
                seconds += value;
                number.clear();
            }
            _ => {}
        }
    }

    (seconds > 0).then_some(seconds)
}

fn contains_reset_keyword(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    contains_any(
        &lower,
        &[
            "reset", "resets", "renew", "update", "refresh", "重置", "刷新", "更新",
        ],
    )
}

fn countdown_segment(line: &str) -> &str {
    let lower = line.to_ascii_lowercase();
    let keywords = [
        "resets in",
        "reset in",
        "resets",
        "reset",
        "refresh",
        "update",
        "renew",
        "刷新",
        "更新",
        "重置",
    ];

    for keyword in keywords {
        if let Some(index) = lower.find(keyword) {
            return &line[index + keyword.len()..];
        }
    }

    line
}

fn strip_percent_segments(line: &str) -> String {
    let mut output = String::new();
    let mut digits = String::new();

    for character in line.chars() {
        if character.is_ascii_digit() {
            digits.push(character);
            continue;
        }

        if character == '%' {
            digits.clear();
            continue;
        }

        output.push_str(&digits);
        digits.clear();
        output.push(character);
    }

    output.push_str(&digits);
    output
}

fn looks_like_countdown(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    contains_any(
        &lower,
        &["h", "m", "s", "d", "小时", "分钟", "秒", "天", "后"],
    )
}

fn normalize_reset_timestamp(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_end_matches('.');
    if let Some(prefix) = trimmed.strip_suffix(" UTC") {
        let normalized = format!("{}:00Z", prefix.replace(' ', "T"));
        return is_iso_z_timestamp(&normalized).then_some(normalized);
    }
    is_iso_z_timestamp(trimmed).then_some(trimmed.to_string())
}

fn is_iso_z_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 | 10 | 13 | 16 | 19 => true,
            _ => byte.is_ascii_digit(),
        })
}

fn value_after_colon(line: &str) -> Option<String> {
    line.rsplit_once('：')
        .or_else(|| line.split_once(':'))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn keyword_positions(line: &str, keywords: &[&str]) -> Vec<usize> {
    let lower = line.to_ascii_lowercase();
    keywords
        .iter()
        .filter_map(|keyword| lower.find(&keyword.to_ascii_lowercase()))
        .collect()
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn is_generic_status_header(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower == "codex status" || lower == "/status" || lower == "status"
}

fn warning(code: &str, message: &str) -> ParseWarning {
    ParseWarning {
        code: code.to_string(),
        message: message.to_string(),
    }
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
    use crate::status_parser::{parse_status_text, ParseClock};

    const CAPTURED_AT: &str = "2026-06-18T08:00:00Z";

    fn parse(raw: &str) -> crate::status_parser::ParseResult {
        parse_status_text(raw, ParseClock::fixed(CAPTURED_AT))
    }

    #[test]
    fn parses_complete_dual_window_status() {
        let result = parse(
            "Codex status\n5h limit\nRemaining: 72%\nResets in: 2h 15m\nWeekly limit\nRemaining: 46%\nReset at: 2026-06-23T09:00:00Z",
        );

        assert_eq!(result.snapshot.five_hour.remaining_percent, Some(72));
        assert_eq!(
            result.snapshot.five_hour.reset_countdown_seconds,
            Some(8_100)
        );
        assert_eq!(result.snapshot.weekly.remaining_percent, Some(46));
        assert_eq!(
            result.snapshot.weekly.reset_at.as_deref(),
            Some("2026-06-23T09:00:00Z")
        );
        assert!(result.snapshot.warnings.is_empty());
    }

    #[test]
    fn parses_chinese_inline_status() {
        let result =
            parse("5小时额度：剩余 88%，刷新 1小时30分钟后\n1周额度：剩余 62%，更新：周一 09:00");

        assert_eq!(result.snapshot.five_hour.remaining_percent, Some(88));
        assert_eq!(
            result.snapshot.five_hour.reset_countdown_seconds,
            Some(5_400)
        );
        assert_eq!(result.snapshot.weekly.remaining_percent, Some(62));
        assert_eq!(
            result.snapshot.weekly.reset_at.as_deref(),
            Some("周一 09:00")
        );
    }

    #[test]
    fn picks_remaining_percent_when_used_is_also_present() {
        let result = parse("5h usage: 28% used, 72% remaining\n1w usage: remaining 46%");

        assert_eq!(result.snapshot.five_hour.remaining_percent, Some(72));
        assert_eq!(result.snapshot.weekly.remaining_percent, Some(46));
    }

    #[test]
    fn keeps_current_model_limits_before_spark_limits() {
        let result = parse(
            "5h limit: [======] 44% left (resets 22:04)\nWeekly limit: [======] 59% left (resets 07:00 on 25 Jun)\nGPT-5.3-Codex-Spark limit:\n5h limit: [======] 100% left (resets 02:51 on 19 Jun)\nWeekly limit: [======] 100% left (resets 21:51 on 25 Jun)",
        );

        assert_eq!(result.snapshot.five_hour.remaining_percent, Some(44));
        assert_eq!(result.snapshot.five_hour.reset_at.as_deref(), Some("22:04"));
        assert_eq!(result.snapshot.weekly.remaining_percent, Some(59));
        assert_eq!(
            result.snapshot.weekly.reset_at.as_deref(),
            Some("07:00 on 25 Jun")
        );
    }

    #[test]
    fn parses_terminal_output_with_ansi_sequences() {
        let result = parse(
            "\u{1b}[36m5h limit:\u{1b}[0m [====] 44% left (resets 22:04)\n\u{1b}[35mWeekly limit:\u{1b}[0m [====] 59% left (resets 07:00 on 25 Jun)",
        );

        assert_eq!(result.snapshot.five_hour.remaining_percent, Some(44));
        assert_eq!(result.snapshot.weekly.remaining_percent, Some(59));
    }

    #[test]
    fn reports_partial_when_one_window_is_missing() {
        let result = parse("5h remaining: 31%\n5h reset in 10m");

        assert_eq!(result.snapshot.five_hour.remaining_percent, Some(31));
        assert!(result.snapshot.weekly.remaining_percent.is_none());
        assert!(result
            .snapshot
            .warnings
            .iter()
            .any(|warning| warning.code == "missing-weekly"));
    }

    #[test]
    fn reports_unknown_format() {
        let result = parse("all systems nominal");

        assert!(!result.snapshot.has_any_usage());
        assert!(result
            .snapshot
            .warnings
            .iter()
            .any(|warning| warning.code == "no-quota-fields"));
    }
}
