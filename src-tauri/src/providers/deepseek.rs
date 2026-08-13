use super::{captured_at_now, ProviderError};
use crate::http_client::{HttpClient, HttpResponse, OfficialEndpoint};
use crate::models::{DeepSeekBalance, DeepSeekSnapshot, ProviderErrorCategory, ProviderSnapshot};
use reqwest::StatusCode;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct BalanceResponse {
    is_available: bool,
    balance_infos: Vec<BalanceInfo>,
}

#[derive(Debug, Deserialize)]
struct BalanceInfo {
    currency: Currency,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}

#[derive(Debug, Deserialize)]
enum Currency {
    #[serde(rename = "CNY")]
    Cny,
    #[serde(rename = "USD")]
    Usd,
}

impl Currency {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Cny => "CNY",
            Self::Usd => "USD",
        }
    }
}

pub fn fetch(client: &HttpClient, api_key: &str) -> Result<ProviderSnapshot, ProviderError> {
    let response = client
        .get_bearer(OfficialEndpoint::DeepSeekBalance, api_key)
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

    let payload: BalanceResponse =
        serde_json::from_slice(&response.body).map_err(|_| ProviderError::invalid_response())?;
    let balances = payload
        .balance_infos
        .into_iter()
        .map(|balance| {
            if !is_decimal_string(&balance.total_balance)
                || !is_decimal_string(&balance.granted_balance)
                || !is_decimal_string(&balance.topped_up_balance)
            {
                return Err(ProviderError::new(
                    ProviderErrorCategory::InvalidResponse,
                    "DeepSeek 返回了无效的余额金额。",
                ));
            }
            Ok(DeepSeekBalance {
                currency: balance.currency.as_str().to_string(),
                total_balance: balance.total_balance,
                granted_balance: balance.granted_balance,
                topped_up_balance: balance.topped_up_balance,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ProviderSnapshot::DeepSeek(DeepSeekSnapshot {
        id: captured_at.clone(),
        captured_at,
        is_available: payload.is_available,
        balances,
    }))
}

fn is_decimal_string(value: &str) -> bool {
    let value = value.strip_prefix('-').unwrap_or(value);
    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    !whole.is_empty()
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && fraction.is_none_or(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
        && parts.next().is_none()
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

    fn parse_fixture(name: &str, body: &str) -> ProviderSnapshot {
        parse_response(response(StatusCode::OK, body), format!("unix:{name}"))
            .expect("fixture should parse")
    }

    #[test]
    fn parses_single_currency_string_amounts() {
        let snapshot = parse_fixture(
            "1000",
            include_str!("fixtures/deepseek-single-currency.json"),
        );
        let ProviderSnapshot::DeepSeek(snapshot) = snapshot else {
            panic!("expected DeepSeek snapshot");
        };

        assert!(snapshot.is_available);
        assert_eq!(snapshot.balances.len(), 1);
        assert_eq!(snapshot.balances[0].currency, "CNY");
        assert_eq!(snapshot.balances[0].topped_up_balance, "100.00");
    }

    #[test]
    fn preserves_multiple_currencies_and_decimal_text() {
        let snapshot = parse_fixture(
            "1001",
            include_str!("fixtures/deepseek-multi-currency.json"),
        );
        let ProviderSnapshot::DeepSeek(snapshot) = snapshot else {
            panic!("expected DeepSeek snapshot");
        };

        assert_eq!(snapshot.balances.len(), 2);
        assert_eq!(snapshot.balances[0].total_balance, "0.000000000000000001");
        assert_eq!(snapshot.balances[1].currency, "USD");
        assert_eq!(snapshot.balances[1].granted_balance, "2.3400");
    }

    #[test]
    fn accepts_zero_balances_and_unknown_fields() {
        let snapshot = parse_fixture(
            "1002",
            include_str!("fixtures/deepseek-zero-extra-fields.json"),
        );
        let ProviderSnapshot::DeepSeek(snapshot) = snapshot else {
            panic!("expected DeepSeek snapshot");
        };

        assert!(!snapshot.is_available);
        assert_eq!(snapshot.balances[0].total_balance, "0.00");
    }

    #[test]
    fn rejects_non_json_missing_fields_unknown_currency_and_non_decimal_strings() {
        for body in [
            "not-json",
            include_str!("fixtures/deepseek-missing-field.json"),
            r#"{"is_available":true,"balance_infos":[{"currency":"EUR","total_balance":"1","granted_balance":"0","topped_up_balance":"1"}]}"#,
            r#"{"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"NaN","granted_balance":"0","topped_up_balance":"1"}]}"#,
        ] {
            let error = parse_response(response(StatusCode::OK, body), "unix:1003".to_string())
                .unwrap_err();
            assert_eq!(error.category(), ProviderErrorCategory::InvalidResponse);
        }
    }

    #[test]
    fn maps_http_error_statuses_without_reading_error_bodies() {
        for (status, category) in [
            (
                StatusCode::UNAUTHORIZED,
                ProviderErrorCategory::Unauthorized,
            ),
            (
                StatusCode::TOO_MANY_REQUESTS,
                ProviderErrorCategory::RateLimited,
            ),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ProviderErrorCategory::Server,
            ),
            (
                StatusCode::SERVICE_UNAVAILABLE,
                ProviderErrorCategory::Server,
            ),
        ] {
            let secret_body = r#"{"error":"body must not escape"}"#;
            let error =
                parse_response(response(status, secret_body), "unix:1004".to_string()).unwrap_err();
            assert_eq!(error.category(), category);
            assert!(!format!("{error:?}").contains("body must not escape"));
        }
    }

    #[test]
    fn decimal_validation_is_strict_without_float_conversion() {
        for valid in ["0", "0.00", "12345678901234567890.0000000001", "-0.1"] {
            assert!(is_decimal_string(valid), "{valid}");
        }
        for invalid in ["", ".1", "1.", "1e3", "+1", "--1", "1.2.3"] {
            assert!(!is_decimal_string(invalid), "{invalid}");
        }
    }
}
