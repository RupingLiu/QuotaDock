use crate::models::{ConfidenceState, ParseWarning, SnapshotSource, UsageSnapshot};
#[cfg(test)]
use crate::models::{ManualField, ManualUpdateInput};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseResult {
    pub snapshot: UsageSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseClock {
    parsed_at: String,
}

impl ParseClock {
    pub fn now() -> Self {
        Self {
            parsed_at: unix_timestamp_string(),
        }
    }

    #[cfg(test)]
    pub fn fixed(parsed_at: &str) -> Self {
        Self {
            parsed_at: parsed_at.to_string(),
        }
    }
}

impl ParseResult {
    #[cfg(test)]
    pub fn apply_manual_overlay(&mut self, input: ManualUpdateInput) {
        self.snapshot.source = SnapshotSource::Manual;
        self.snapshot.confidence = ConfidenceState::Manual;
        self.snapshot.remaining_percent = input.remaining_percent;
        self.snapshot.reset_at = input.reset_at;
        self.snapshot.credits_balance = input.credits_balance;
        self.snapshot.notes = input.notes.unwrap_or_default();
        self.snapshot.manual_fields = vec![
            ManualField::RemainingPercent,
            ManualField::ResetAt,
            ManualField::CreditsBalance,
            ManualField::Notes,
        ];
    }
}

pub fn parse_status_text(raw_text: &str, clock: ParseClock) -> ParseResult {
    let mut extracted = ExtractedStatus::default();
    let mut unknown_lines = Vec::new();

    for line in raw_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("codex status") {
            continue;
        }

        let mut matched = false;
        if extracted.model.is_none() {
            if let Some(model) = extract_model(trimmed) {
                extracted.model = Some(model);
                matched = true;
            }
        }
        if extracted.remaining_percent.is_none() {
            if let Some(percent) = extract_percent(trimmed) {
                extracted.remaining_percent = Some(percent);
                matched = true;
            }
        }
        if extracted.reset_at.is_none() {
            if let Some(reset_at) = extract_reset_at(trimmed) {
                extracted.reset_at = Some(reset_at);
                matched = true;
            }
        }
        if extracted.reset_countdown_seconds.is_none() {
            if let Some(seconds) = extract_countdown(trimmed) {
                extracted.reset_countdown_seconds = Some(seconds);
                matched = true;
            }
        }
        if extracted.credits_balance.is_none() {
            if let Some(balance) =
                extract_decimal_after_keywords(trimmed, &["credits balance", "balance"])
            {
                extracted.credits_balance = Some(balance);
                matched = true;
            }
        }
        if extracted.context_window.is_none() {
            if let Some(context) =
                extract_value_after_keywords(trimmed, &["context window", "context"])
            {
                extracted.context_window = Some(context);
                matched = true;
            }
        }

        if !matched {
            unknown_lines.push(trimmed.to_string());
        }
    }

    let mut warnings = Vec::new();
    let has_usage_field = extracted.remaining_percent.is_some()
        || extracted.reset_at.is_some()
        || extracted.reset_countdown_seconds.is_some()
        || extracted.credits_balance.is_some()
        || extracted.model.is_some()
        || extracted.context_window.is_some();

    if !has_usage_field {
        warnings.push(warning(
            "no-usage-fields",
            "No recognizable Codex usage fields were found.",
        ));
    } else {
        if extracted.remaining_percent.is_none() {
            warnings.push(warning(
                "missing-remaining-percent",
                "No remaining usage percentage was found.",
            ));
        }
        if extracted.reset_at.is_none() && extracted.reset_countdown_seconds.is_none() {
            warnings.push(warning(
                "missing-reset",
                "No reset time or countdown was found.",
            ));
        }
        if !unknown_lines.is_empty() {
            warnings.push(warning(
                "unknown-lines",
                "Some pasted lines were not recognized and were preserved in raw text.",
            ));
        }
    }

    let confidence = if !has_usage_field {
        ConfidenceState::Unavailable
    } else if extracted.remaining_percent.is_some()
        && (extracted.reset_at.is_some() || extracted.reset_countdown_seconds.is_some())
    {
        ConfidenceState::Fresh
    } else {
        ConfidenceState::Partial
    };

    ParseResult {
        snapshot: UsageSnapshot {
            id: clock.parsed_at.clone(),
            source: SnapshotSource::PastedStatus,
            parsed_at: clock.parsed_at,
            remaining_percent: extracted.remaining_percent,
            reset_at: extracted.reset_at,
            reset_countdown_seconds: extracted.reset_countdown_seconds,
            credits_balance: extracted.credits_balance,
            model: extracted.model,
            context_window: extracted.context_window,
            confidence,
            raw_text: raw_text.to_string(),
            manual_fields: Vec::new(),
            warnings,
            notes: String::new(),
        },
    }
}

#[derive(Debug, Default)]
struct ExtractedStatus {
    model: Option<String>,
    remaining_percent: Option<u8>,
    reset_at: Option<String>,
    reset_countdown_seconds: Option<i64>,
    credits_balance: Option<f64>,
    context_window: Option<String>,
}

fn extract_model(line: &str) -> Option<String> {
    if !contains_any(line, &["model", "active model"]) {
        return None;
    }
    if let Some(value) = value_after_colon(line) {
        return Some(value);
    }
    line.split_whitespace()
        .find(|token| token.starts_with("gpt-"))
        .map(clean_token)
}

fn extract_percent(line: &str) -> Option<u8> {
    let lower = line.to_ascii_lowercase();
    if !(lower.contains("remaining") || lower.contains(" left") || lower.contains("available")) {
        return None;
    }

    let tokens: Vec<&str> = line.split_whitespace().collect();
    for (index, token) in tokens.iter().enumerate() {
        let Some(value) = parse_percent_token(token) else {
            continue;
        };
        if value <= 100 && nearby_percent_label(&tokens, index) {
            return Some(value);
        }
    }
    None
}

fn parse_percent_token(token: &str) -> Option<u8> {
    let stripped = token
        .trim_matches(|character: char| {
            character == ':'
                || character == ','
                || character == ';'
                || character == '('
                || character == ')'
        })
        .trim_end_matches('%');
    stripped.parse::<u8>().ok()
}

fn nearby_percent_label(tokens: &[&str], index: usize) -> bool {
    let previous = index
        .checked_sub(1)
        .and_then(|previous| tokens.get(previous))
        .is_some_and(|token| is_remaining_label(token));
    let next = tokens
        .get(index + 1)
        .is_some_and(|token| is_remaining_label(token));
    previous || next
}

fn is_remaining_label(token: &str) -> bool {
    let cleaned = token
        .trim_matches(|character: char| {
            character == ':'
                || character == ','
                || character == ';'
                || character == '('
                || character == ')'
        })
        .to_ascii_lowercase();
    cleaned == "remaining" || cleaned == "left" || cleaned == "available"
}

fn extract_reset_at(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    if !(lower.contains("reset at") || lower.contains("resets at")) {
        return None;
    }

    if let Some(value) = value_after_colon(line) {
        return normalize_reset_timestamp(&value);
    }

    let lower = line.to_ascii_lowercase();
    let at_index = lower.rfind(" at ")?;
    normalize_reset_timestamp(line[at_index + 4..].trim())
}

fn extract_countdown(line: &str) -> Option<i64> {
    let lower = line.to_ascii_lowercase();
    if !(lower.contains("reset in") || lower.contains("resets in")) {
        return None;
    }

    let mut seconds = 0_i64;
    let tokens: Vec<&str> = line.split_whitespace().collect();
    for window in tokens.windows(2) {
        let value = window[0].parse::<i64>().ok();
        let unit = window[1].trim_matches(|character: char| character == ',' || character == ';');
        if let Some(value) = value {
            if unit.starts_with('h') {
                seconds += value * 3_600;
            } else if unit.starts_with('m') {
                seconds += value * 60;
            } else if unit.starts_with('s') {
                seconds += value;
            }
        }
    }
    if seconds > 0 {
        return Some(seconds);
    }

    let compact = line.replace(' ', "");
    Some(parse_compact_countdown(&compact)?)
}

fn parse_compact_countdown(value: &str) -> Option<i64> {
    let mut number = String::new();
    let mut seconds = 0_i64;
    for character in value.chars() {
        if character.is_ascii_digit() {
            number.push(character);
            continue;
        }

        if number.is_empty() {
            continue;
        }

        let parsed = number.parse::<i64>().ok()?;
        match character.to_ascii_lowercase() {
            'h' => seconds += parsed * 3_600,
            'm' => seconds += parsed * 60,
            's' => seconds += parsed,
            _ => {}
        }
        number.clear();
    }
    (seconds > 0).then_some(seconds)
}

fn extract_decimal_after_keywords(line: &str, keywords: &[&str]) -> Option<f64> {
    if !contains_any(line, keywords) {
        return None;
    }
    for token in line.split_whitespace() {
        let cleaned = token.trim_matches(|character: char| {
            character == ':' || character == ',' || character == ';' || character == '$'
        });
        if let Ok(value) = cleaned.parse::<f64>() {
            return Some(value);
        }
    }
    None
}

fn extract_value_after_keywords(line: &str, keywords: &[&str]) -> Option<String> {
    if !contains_any(line, keywords) {
        return None;
    }
    value_after_colon(line)
}

fn contains_any(line: &str, needles: &[&str]) -> bool {
    let lower = line.to_ascii_lowercase();
    needles.iter().any(|needle| lower.contains(needle))
}

fn value_after_colon(line: &str) -> Option<String> {
    let (label, value) = line.split_once(':')?;
    if label.chars().any(|character| character.is_ascii_digit()) {
        return None;
    }
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
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

fn clean_token(token: &str) -> String {
    token
        .trim_matches(|character: char| character == ':' || character == ',' || character == ';')
        .to_string()
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
    use crate::models::{ConfidenceState, ManualField};
    use crate::status_parser::{parse_status_text, ParseClock};

    const PARSED_AT: &str = "2026-06-18T04:55:00Z";

    fn parse(raw: &str) -> crate::status_parser::ParseResult {
        parse_status_text(raw, ParseClock::fixed(PARSED_AT))
    }

    #[test]
    fn parses_complete_status_text() {
        let result = parse(include_str!("../fixtures/status/complete-status.txt"));

        assert_eq!(result.snapshot.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(result.snapshot.remaining_percent, Some(72));
        assert_eq!(
            result.snapshot.reset_at.as_deref(),
            Some("2026-06-18T07:10:00Z")
        );
        assert_eq!(result.snapshot.credits_balance, Some(12.5));
        assert_eq!(
            result.snapshot.context_window.as_deref(),
            Some("200k tokens")
        );
        assert_eq!(result.snapshot.confidence, ConfidenceState::Fresh);
        assert_eq!(result.snapshot.parsed_at, PARSED_AT);
        assert!(result.snapshot.warnings.is_empty());
        assert!(result.snapshot.raw_text.contains("Usage remaining"));
    }

    #[test]
    fn missing_reset_is_partial_with_warning() {
        let result = parse(include_str!("../fixtures/status/missing-reset.txt"));

        assert_eq!(result.snapshot.remaining_percent, Some(45));
        assert_eq!(result.snapshot.reset_at, None);
        assert_eq!(result.snapshot.confidence, ConfidenceState::Partial);
        assert!(result
            .snapshot
            .warnings
            .iter()
            .any(|warning| warning.code == "missing-reset"));
    }

    #[test]
    fn missing_percent_is_partial_with_countdown_and_credit() {
        let result = parse(include_str!("../fixtures/status/missing-percent.txt"));

        assert_eq!(result.snapshot.remaining_percent, None);
        assert_eq!(result.snapshot.reset_countdown_seconds, Some(8_100));
        assert_eq!(result.snapshot.credits_balance, Some(9.25));
        assert_eq!(result.snapshot.confidence, ConfidenceState::Partial);
        assert!(result
            .snapshot
            .warnings
            .iter()
            .any(|warning| warning.code == "missing-remaining-percent"));
    }

    #[test]
    fn parses_reordered_lines() {
        let result = parse(include_str!("../fixtures/status/reordered-lines.txt"));

        assert_eq!(
            result.snapshot.model.as_deref(),
            Some("gpt-5.3-codex-spark")
        );
        assert_eq!(result.snapshot.remaining_percent, Some(64));
        assert_eq!(result.snapshot.reset_countdown_seconds, Some(5_400));
        assert_eq!(result.snapshot.confidence, ConfidenceState::Fresh);
    }

    #[test]
    fn accepts_extra_noise_without_failing_parse() {
        let result = parse(include_str!("../fixtures/status/extra-noise.txt"));

        assert_eq!(result.snapshot.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(result.snapshot.remaining_percent, Some(18));
        assert_eq!(
            result.snapshot.reset_at.as_deref(),
            Some("2026-06-18T09:00:00Z")
        );
        assert_eq!(result.snapshot.confidence, ConfidenceState::Fresh);
        assert!(result
            .snapshot
            .warnings
            .iter()
            .any(|warning| warning.code == "unknown-lines"));
    }

    #[test]
    fn unknown_format_is_unavailable_and_preserves_raw_text() {
        let raw = include_str!("../fixtures/status/unknown-format.txt");
        let result = parse(raw);

        assert_eq!(result.snapshot.confidence, ConfidenceState::Unavailable);
        assert_eq!(result.snapshot.raw_text, raw);
        assert!(result
            .snapshot
            .warnings
            .iter()
            .any(|warning| warning.code == "no-usage-fields"));
    }

    #[test]
    fn rejects_used_percent_invalid_reset_and_non_balance_credits() {
        let result = parse(include_str!("../fixtures/status/adversarial-status.txt"));

        assert_eq!(result.snapshot.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(result.snapshot.remaining_percent, None);
        assert_eq!(result.snapshot.reset_at, None);
        assert_eq!(result.snapshot.credits_balance, None);
        assert_eq!(result.snapshot.confidence, ConfidenceState::Partial);
        assert!(result
            .snapshot
            .warnings
            .iter()
            .any(|warning| warning.code == "missing-remaining-percent"));
        assert!(result
            .snapshot
            .warnings
            .iter()
            .any(|warning| warning.code == "missing-reset"));
    }

    #[test]
    fn mixed_used_and_remaining_line_uses_remaining_percent() {
        let result = parse("Model: gpt-5.5\nUsage: 82% used, 18% remaining\nReset in: 1h");

        assert_eq!(result.snapshot.remaining_percent, Some(18));
        assert_eq!(result.snapshot.confidence, ConfidenceState::Fresh);
    }

    #[test]
    fn mixed_used_and_remaining_prefix_line_uses_remaining_percent() {
        let result = parse("Model: gpt-5.5\nUsage: 82% used, remaining 18%\nReset in: 1h");

        assert_eq!(result.snapshot.remaining_percent, Some(18));
        assert_eq!(result.snapshot.confidence, ConfidenceState::Fresh);
    }

    #[test]
    fn manual_overlay_replaces_fields_and_marks_manual() {
        let mut result = parse(include_str!("../fixtures/status/missing-percent.txt"));

        result.apply_manual_overlay(crate::models::ManualUpdateInput {
            remaining_percent: Some(22),
            reset_at: Some("2026-06-18T11:00:00Z".to_string()),
            credits_balance: None,
            notes: Some("Corrected from official dashboard".to_string()),
        });

        assert_eq!(result.snapshot.confidence, ConfidenceState::Manual);
        assert_eq!(result.snapshot.remaining_percent, Some(22));
        assert_eq!(
            result.snapshot.reset_at.as_deref(),
            Some("2026-06-18T11:00:00Z")
        );
        assert_eq!(result.snapshot.credits_balance, None);
        assert_eq!(result.snapshot.notes, "Corrected from official dashboard");
        assert_eq!(
            result.snapshot.manual_fields,
            vec![
                ManualField::RemainingPercent,
                ManualField::ResetAt,
                ManualField::CreditsBalance,
                ManualField::Notes
            ]
        );
    }
}
