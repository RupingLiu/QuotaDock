use super::{captured_at_now, ProviderError};
use crate::http_client::{HttpClient, HttpResponse, OfficialEndpoint};
use crate::models::{KimiRegion, KimiSnapshot, ProviderErrorCategory, ProviderSnapshot};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Number;

#[derive(Debug, Deserialize)]
struct BalanceResponse {
    code: i64,
    data: BalanceData,
    scode: String,
    status: bool,
}

#[derive(Debug, Deserialize)]
struct BalanceData {
    available_balance: Number,
    voucher_balance: Number,
    cash_balance: Number,
}

pub fn fetch(client: &HttpClient, api_key: &str) -> Result<ProviderSnapshot, ProviderError> {
    let response = client
        .get_bearer(OfficialEndpoint::KimiBalance, api_key)
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
    if payload.code != 0 || !payload.status {
        return Err(ProviderError::new(
            ProviderErrorCategory::InvalidResponse,
            "Kimi 余额服务返回了失败状态。",
        ));
    }
    if payload.scode.is_empty() {
        return Err(ProviderError::invalid_response());
    }
    if !number_is_non_negative(&payload.data.voucher_balance) {
        return Err(ProviderError::new(
            ProviderErrorCategory::InvalidResponse,
            "Kimi 返回了无效的负数代金券余额。",
        ));
    }

    Ok(ProviderSnapshot::Kimi(KimiSnapshot {
        id: captured_at.clone(),
        captured_at,
        region: KimiRegion::China,
        currency: "CNY".to_string(),
        available_balance: payload.data.available_balance.to_string(),
        cash_balance: payload.data.cash_balance.to_string(),
        voucher_balance: payload.data.voucher_balance.to_string(),
    }))
}

fn number_is_non_negative(number: &Number) -> bool {
    let representation = number.to_string();
    let Some(magnitude) = representation.strip_prefix('-') else {
        return true;
    };
    let mantissa = magnitude
        .split_once(['e', 'E'])
        .map_or(magnitude, |(mantissa, _)| mantissa);
    mantissa
        .bytes()
        .filter(|byte| *byte != b'.')
        .all(|byte| byte == b'0')
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
    fn parses_normal_balance_and_assigns_china_currency() {
        let snapshot = parse_fixture("2000", include_str!("fixtures/kimi-normal.json"));
        let ProviderSnapshot::Kimi(snapshot) = snapshot else {
            panic!("expected Kimi snapshot");
        };

        assert_eq!(snapshot.region, KimiRegion::China);
        assert_eq!(snapshot.currency, "CNY");
        assert_eq!(snapshot.available_balance, "49.58894");
        assert_eq!(snapshot.voucher_balance, "46.58893");
        assert_eq!(snapshot.cash_balance, "3.00001");
    }

    #[test]
    fn preserves_zero_and_negative_cash_without_recomputing_available_balance() {
        let zero = parse_fixture("2001", include_str!("fixtures/kimi-zero.json"));
        let negative = parse_fixture("2002", include_str!("fixtures/kimi-negative-cash.json"));
        let ProviderSnapshot::Kimi(zero) = zero else {
            panic!("expected Kimi snapshot");
        };
        let ProviderSnapshot::Kimi(negative) = negative else {
            panic!("expected Kimi snapshot");
        };

        assert_eq!(zero.available_balance, "0.0000");
        assert_eq!(negative.available_balance, "50.0000");
        assert_eq!(negative.cash_balance, "-0.4100");
        assert_eq!(negative.voucher_balance, "50.0000");
    }

    #[test]
    fn arbitrary_precision_json_numbers_are_not_routed_through_f64() {
        let snapshot = parse_fixture(
            "2003",
            include_str!("fixtures/kimi-arbitrary-precision.json"),
        );
        let ProviderSnapshot::Kimi(snapshot) = snapshot else {
            panic!("expected Kimi snapshot");
        };

        assert_eq!(
            snapshot.available_balance,
            "12345678901234567890.12345678901234567890"
        );
        assert_eq!(snapshot.cash_balance, "-0.00000000000000000001");
    }

    #[test]
    fn rejects_nonzero_code_false_status_non_json_and_missing_fields() {
        for body in [
            include_str!("fixtures/kimi-code-failure.json"),
            include_str!("fixtures/kimi-status-failure.json"),
            include_str!("fixtures/kimi-missing-field.json"),
            "not-json",
        ] {
            let error = parse_response(response(StatusCode::OK, body), "unix:2005".to_string())
                .unwrap_err();
            assert_eq!(error.category(), ProviderErrorCategory::InvalidResponse);
        }
    }

    #[test]
    fn rejects_string_amounts_because_official_contract_requires_numbers() {
        let body = r#"{
            "code": 0,
            "data": {
                "available_balance": "1.00",
                "voucher_balance": 0,
                "cash_balance": 1
            },
            "scode": "0x0",
            "status": true
        }"#;

        let error =
            parse_response(response(StatusCode::OK, body), "unix:2006".to_string()).unwrap_err();

        assert_eq!(error.category(), ProviderErrorCategory::InvalidResponse);
    }

    #[test]
    fn rejects_negative_voucher_balance_without_float_conversion() {
        let error = parse_response(
            response(
                StatusCode::OK,
                include_str!("fixtures/kimi-negative-voucher.json"),
            ),
            "unix:2007".to_string(),
        )
        .unwrap_err();

        assert_eq!(error.category(), ProviderErrorCategory::InvalidResponse);
        assert!(number_is_non_negative(
            &serde_json::from_str::<Number>("-0.000e100").unwrap()
        ));
    }

    #[test]
    fn maps_http_error_statuses_without_exposing_error_body() {
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
            let error = parse_response(
                response(status, r#"{"secret":"must not escape"}"#),
                "unix:2008".to_string(),
            )
            .unwrap_err();
            assert_eq!(error.category(), category);
            assert!(!format!("{error:?}").contains("must not escape"));
        }
    }
}
