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
    let mut weekly = QuotaReading::default();
    let mut weekly_active = false;
    let mut unknown_lines = Vec::new();

    for line in raw_text.lines() {
        let cleaned = clean_terminal_line(line);
        let trimmed = cleaned.trim();
        if trimmed.is_empty() || is_generic_status_header(trimmed) {
            continue;
        }
        if is_secondary_model_quota_section(trimmed) {
            break;
        }

        let weekly_label = is_weekly_quota_label(trimmed);
        if weekly_label {
            weekly_active = true;
        } else if is_quota_section_label(trimmed) {
            weekly_active = false;
            continue;
        }

        if !weekly_active {
            continue;
        }

        let mut matched = weekly_label;
        if weekly.remaining_percent.is_none() {
            if let Some(percent) = extract_percent(trimmed) {
                weekly.remaining_percent = Some(percent);
                matched = true;
            }
        }
        if weekly.reset_at.is_none() && weekly.reset_countdown_seconds.is_none() {
            if let Some(reset_at) = extract_reset_at(trimmed) {
                weekly.reset_at = Some(reset_at);
                matched = true;
            } else if let Some(seconds) = extract_countdown(trimmed) {
                weekly.reset_countdown_seconds = Some(seconds);
                matched = true;
            }
        }

        if !matched {
            unknown_lines.push(trimmed.to_string());
        }
    }

    let mut warnings = Vec::new();
    if !unknown_lines.is_empty() && weekly.has_usage() {
        warnings.push(warning("unknown-lines", "部分粘贴内容未被识别，已忽略。"));
    }
    if !weekly.has_usage() {
        warnings.push(warning("no-quota-fields", "没有找到可用的额度信息。"));
    }

    let status_message = if warnings
        .iter()
        .any(|warning| warning.code == "no-quota-fields")
    {
        "没有识别到 1 周额度，请检查 /status 内容。".to_string()
    } else if weekly.has_usage() && warnings.is_empty() {
        "已更新 1 周额度。".to_string()
    } else {
        "已更新 1 周额度，部分内容未识别。".to_string()
    };

    ParseResult {
        snapshot: QuotaSnapshot {
            id: clock.captured_at.clone(),
            source,
            captured_at: clock.captured_at,
            weekly,
            plan_type: None,
            credits_balance: None,
            reset_credits_available: None,
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

fn is_weekly_quota_label(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    contains_any(
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
    )
}

fn is_quota_section_label(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    contains_any(&lower, &["limit", "quota", "额度"])
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

    reset_phrase(line).or_else(|| {
        value_after_colon(line).filter(|value| !value.contains('%') && !looks_like_countdown(value))
    })
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

fn is_secondary_model_quota_section(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("codex-spark") && lower.contains("limit")
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
    fn parses_weekly_status_with_countdown() {
        let result = parse("Codex status\nWeekly limit\nRemaining: 46%\nResets in: 2h 15m");

        assert_eq!(result.snapshot.weekly.remaining_percent, Some(46));
        assert_eq!(result.snapshot.weekly.reset_countdown_seconds, Some(8_100));
        assert!(result.snapshot.warnings.is_empty());
    }

    #[test]
    fn parses_chinese_inline_status() {
        let result = parse("1周额度：剩余 62%，更新：周一 09:00");

        assert_eq!(result.snapshot.weekly.remaining_percent, Some(62));
        assert_eq!(
            result.snapshot.weekly.reset_at.as_deref(),
            Some("周一 09:00")
        );
    }

    #[test]
    fn picks_remaining_percent_when_used_is_also_present() {
        let result = parse("1w usage: 54% used, 46% remaining");

        assert_eq!(result.snapshot.weekly.remaining_percent, Some(46));
    }

    #[test]
    fn keeps_current_model_limits_before_spark_limits() {
        let result = parse(
            "Weekly limit: [======] 59% left (resets 07:00 on 25 Jun)\nGPT-5.3-Codex-Spark limit:\nWeekly limit: [======] 100% left (resets 21:51 on 25 Jun)",
        );

        assert_eq!(result.snapshot.weekly.remaining_percent, Some(59));
        assert_eq!(
            result.snapshot.weekly.reset_at.as_deref(),
            Some("07:00 on 25 Jun")
        );
    }

    #[test]
    fn does_not_fill_missing_current_fields_from_spark_limits() {
        let result = parse(
            "Weekly limit: [======] 59% left\nGPT-5.3-Codex-Spark limit:\nWeekly limit: [======] 100% left (resets 21:51 on 25 Jun)",
        );

        assert_eq!(result.snapshot.weekly.remaining_percent, Some(59));
        assert!(result.snapshot.weekly.reset_at.is_none());
    }

    #[test]
    fn parses_wrapped_weekly_reset_from_current_status_panel() {
        let result = parse(
            "Weekly limit:                [████░░░░░░░░░░░░░░░░] 22% left\n                              (resets 10:22 on 28 Jun)\nGPT-5.3-Codex-Spark limit:\nWeekly limit:                [████████████████████] 100% left\n                              (resets 14:40 on 2 Jul)",
        );

        assert_eq!(result.snapshot.weekly.remaining_percent, Some(22));
        assert_eq!(
            result.snapshot.weekly.reset_at.as_deref(),
            Some("10:22 on 28 Jun")
        );
    }

    #[test]
    fn parses_terminal_output_with_ansi_sequences() {
        let result =
            parse("\u{1b}[35mWeekly limit:\u{1b}[0m [====] 59% left (resets 07:00 on 25 Jun)");

        assert_eq!(result.snapshot.weekly.remaining_percent, Some(59));
    }

    #[test]
    fn a_new_non_weekly_quota_section_clears_the_active_section() {
        let result = parse("Weekly limit: 59% left\nDaily quota:\nResets in 10m");

        assert_eq!(result.snapshot.weekly.remaining_percent, Some(59));
        assert!(result.snapshot.weekly.reset_at.is_none());
        assert!(result.snapshot.weekly.reset_countdown_seconds.is_none());
    }

    #[test]
    fn accepts_current_weekly_status() {
        let result = parse(
            "Weekly limit: [███████████████████░] 93% left\n                              (resets 13:21 on 22 Jul)\nGPT-5.3-Codex-Spark Weekly limit: [████████████████████] 100% left",
        );

        assert_eq!(result.snapshot.weekly.remaining_percent, Some(93));
        assert_eq!(
            result.snapshot.weekly.reset_at.as_deref(),
            Some("13:21 on 22 Jul")
        );
        assert!(result.snapshot.has_usage());
        assert!(result.snapshot.warnings.is_empty());
        assert_eq!(result.snapshot.status_message, "已更新 1 周额度。");
    }

    #[test]
    fn reports_unknown_format() {
        let result = parse("all systems nominal");

        assert!(!result.snapshot.has_usage());
        assert!(result
            .snapshot
            .warnings
            .iter()
            .any(|warning| warning.code == "no-quota-fields"));
    }
}
