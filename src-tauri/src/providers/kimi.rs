use super::{captured_at_now, ProviderError};
use crate::http_client::{HttpClient, HttpResponse, OfficialEndpoint};
use crate::models::{
    KimiSnapshot, KimiUsage, KimiUsageWindow, KimiUsageWindowUnit, ProviderErrorCategory,
    ProviderSnapshot,
};
use reqwest::StatusCode;
use serde_json::{Map, Value};

pub fn fetch(client: &HttpClient, api_key: &str) -> Result<ProviderSnapshot, ProviderError> {
    let response = client
        .get_bearer(OfficialEndpoint::KimiCodingUsage, api_key)
        .map_err(ProviderError::from)?;
    parse_response(response, captured_at_now())
}

fn parse_response(
    response: HttpResponse,
    captured_at: String,
) -> Result<ProviderSnapshot, ProviderError> {
    if response.status != StatusCode::OK {
        return Err(ProviderError::from_status(response.status));
    }

    let payload: Value =
        serde_json::from_slice(&response.body).map_err(|_| ProviderError::invalid_response())?;
    let object = payload
        .as_object()
        .ok_or_else(ProviderError::invalid_response)?;

    let total = match object.get("usage").and_then(Value::as_object) {
        Some(usage) => parse_usage(usage, None, None)?,
        None => None,
    };

    let mut limits = Vec::new();
    if let Some(items) = object.get("limits").and_then(Value::as_array) {
        for item in items.iter().filter_map(Value::as_object) {
            let Some(detail) = item.get("detail").and_then(Value::as_object) else {
                continue;
            };
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty());
            if let Some(usage) = parse_usage(detail, parse_window(item.get("window")), name)? {
                limits.push(usage);
            }
        }
    }

    if total.is_none() && limits.is_empty() {
        return Err(ProviderError::new(
            ProviderErrorCategory::InvalidResponse,
            "Kimi Coding Plan 未返回可识别的额度窗口。",
        ));
    }

    Ok(ProviderSnapshot::Kimi(KimiSnapshot {
        id: captured_at.clone(),
        captured_at,
        total,
        limits,
    }))
}

fn parse_usage(
    raw: &Map<String, Value>,
    window: Option<KimiUsageWindow>,
    fallback_name: Option<&str>,
) -> Result<Option<KimiUsage>, ProviderError> {
    let used = optional_integer_field(raw, "used")?;
    let limit = optional_integer_field(raw, "limit")?;
    if used.is_none() && limit.is_none() {
        return Ok(None);
    }
    let used = used.unwrap_or_else(|| "0".to_string());
    let limit = limit.unwrap_or_else(|| "0".to_string());
    if !is_non_negative_integer(&used) || !is_non_negative_integer(&limit) {
        return Err(ProviderError::new(
            ProviderErrorCategory::InvalidResponse,
            "Kimi Coding Plan 返回了无效的额度数值。",
        ));
    }
    Ok(Some(KimiUsage {
        name: raw
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .or(fallback_name)
            .map(str::to_string),
        window,
        used,
        limit,
        reset_at: raw
            .get("resetTime")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    }))
}

fn optional_integer_field(
    raw: &Map<String, Value>,
    field: &str,
) -> Result<Option<String>, ProviderError> {
    match raw.get(field) {
        None => Ok(None),
        Some(value) => integer_text(value).map(Some).ok_or_else(|| {
            ProviderError::new(
                ProviderErrorCategory::InvalidResponse,
                "Kimi Coding Plan 返回了无效的额度数值。",
            )
        }),
    }
}

fn parse_window(raw: Option<&Value>) -> Option<KimiUsageWindow> {
    let raw = raw?.as_object()?;
    let duration = raw.get("duration").and_then(non_negative_u64)?;
    let unit = match raw.get("timeUnit").and_then(Value::as_str)? {
        "TIME_UNIT_MINUTE" if duration >= 60 && duration % 60 == 0 => {
            return Some(KimiUsageWindow {
                duration: duration / 60,
                unit: KimiUsageWindowUnit::Hour,
            });
        }
        "TIME_UNIT_MINUTE" => KimiUsageWindowUnit::Minute,
        "TIME_UNIT_HOUR" => KimiUsageWindowUnit::Hour,
        "TIME_UNIT_DAY" => KimiUsageWindowUnit::Day,
        "TIME_UNIT_WEEK" => KimiUsageWindowUnit::Week,
        _ => return None,
    };
    Some(KimiUsageWindow { duration, unit })
}

fn integer_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if is_non_negative_integer(value) => Some(value.clone()),
        Value::Number(value) if is_non_negative_integer(&value.to_string()) => {
            Some(value.to_string())
        }
        _ => None,
    }
}

fn non_negative_u64(value: &Value) -> Option<u64> {
    integer_text(value)?.parse().ok()
}

fn is_non_negative_integer(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: StatusCode, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            body: body.as_bytes().to_vec(),
        }
    }

    fn parse_fixture(name: &str, body: &str) -> KimiSnapshot {
        let snapshot = parse_response(response(StatusCode::OK, body), format!("unix:{name}"))
            .expect("fixture should parse");
        let ProviderSnapshot::Kimi(snapshot) = snapshot else {
            panic!("expected Kimi snapshot");
        };
        snapshot
    }

    #[test]
    fn parses_total_and_five_hour_coding_plan_windows() {
        let snapshot = parse_fixture("2000", include_str!("fixtures/kimi-coding-usage.json"));
        let total = snapshot.total.expect("total usage");
        assert_eq!(total.used, "17");
        assert_eq!(total.limit, "100");
        assert_eq!(total.window, None);
        assert_eq!(snapshot.limits.len(), 2);
        assert_eq!(snapshot.limits[0].name.as_deref(), Some("Code"));
        assert_eq!(
            snapshot.limits[0].window,
            Some(KimiUsageWindow {
                duration: 5,
                unit: KimiUsageWindowUnit::Hour,
            })
        );
        assert_eq!(
            snapshot.limits[0].reset_at.as_deref(),
            Some("2030-01-01T05:00:00Z")
        );
        assert_eq!(
            snapshot.limits[1].window,
            Some(KimiUsageWindow {
                duration: 7,
                unit: KimiUsageWindowUnit::Day,
            })
        );
    }

    #[test]
    fn preserves_large_integer_strings_without_float_conversion() {
        let snapshot = parse_fixture(
            "2001",
            include_str!("fixtures/kimi-coding-large-usage.json"),
        );
        let total = snapshot.total.expect("total usage");
        assert_eq!(total.used, "123456789012345678901234567890");
        assert_eq!(total.limit, "999999999999999999999999999999");
    }

    #[test]
    fn accepts_missing_total_when_a_valid_limit_exists() {
        let snapshot = parse_fixture(
            "2002",
            r#"{"limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},"detail":{"used":"0","limit":"100"}}]}"#,
        );
        assert!(snapshot.total.is_none());
        assert_eq!(snapshot.limits[0].used, "0");
    }

    #[test]
    fn rejects_empty_non_json_negative_decimal_and_malformed_detail_payloads() {
        for body in [
            "not-json",
            "{}",
            r#"{"usage":{"used":"-1","limit":"100"}}"#,
            r#"{"usage":{"used":"1.5","limit":"100"}}"#,
            r#"{"limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},"detail":{"used":{},"limit":[]}}]}"#,
        ] {
            let error =
                parse_response(response(StatusCode::OK, body), "unix:bad".to_string()).unwrap_err();
            assert_eq!(error.category(), ProviderErrorCategory::InvalidResponse);
        }
    }

    #[test]
    fn maps_http_error_statuses_without_exposing_error_body() {
        for (status, category) in [
            (
                StatusCode::UNAUTHORIZED,
                ProviderErrorCategory::Unauthorized,
            ),
            (
                StatusCode::NOT_FOUND,
                ProviderErrorCategory::InvalidResponse,
            ),
            (
                StatusCode::TOO_MANY_REQUESTS,
                ProviderErrorCategory::RateLimited,
            ),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ProviderErrorCategory::Server,
            ),
        ] {
            let error = parse_response(
                response(status, r#"{"secret":"must not escape"}"#),
                "unix:error".to_string(),
            )
            .unwrap_err();
            assert_eq!(error.category(), category);
            assert!(!format!("{error:?}").contains("must not escape"));
        }
    }
}
