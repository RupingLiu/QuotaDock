pub mod deepseek;
pub mod kimi;

use crate::http_client::{HttpError, HttpErrorKind};
use crate::models::ProviderErrorCategory;
use reqwest::StatusCode;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderError {
    category: ProviderErrorCategory,
    message: &'static str,
}

impl ProviderError {
    pub fn new(category: ProviderErrorCategory, message: &'static str) -> Self {
        Self { category, message }
    }

    pub fn category(&self) -> ProviderErrorCategory {
        self.category
    }

    pub fn message(&self) -> &'static str {
        self.message
    }

    pub fn invalid_response() -> Self {
        Self::new(
            ProviderErrorCategory::InvalidResponse,
            "额度服务返回了无法识别的响应。",
        )
    }

    pub fn from_status(status: StatusCode) -> Self {
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Self::new(
                ProviderErrorCategory::Unauthorized,
                "API Key 无效、已失效或不属于当前平台。",
            ),
            StatusCode::PAYMENT_REQUIRED => Self::new(
                ProviderErrorCategory::InsufficientBalance,
                "账户额度或余额不足，服务拒绝了请求。",
            ),
            StatusCode::TOO_MANY_REQUESTS => Self::new(
                ProviderErrorCategory::RateLimited,
                "额度查询过于频繁，请稍后重试。",
            ),
            status if status.is_server_error() => Self::new(
                ProviderErrorCategory::Server,
                "额度服务暂时不可用，请稍后重试。",
            ),
            _ => Self::invalid_response(),
        }
    }
}

impl From<HttpError> for ProviderError {
    fn from(error: HttpError) -> Self {
        match error.kind() {
            HttpErrorKind::Timeout => {
                Self::new(ProviderErrorCategory::Timeout, "额度查询超时，请稍后重试。")
            }
            HttpErrorKind::Network => Self::new(
                ProviderErrorCategory::Network,
                "无法连接额度服务，请检查网络或代理。",
            ),
            HttpErrorKind::InvalidResponse => Self::invalid_response(),
        }
    }
}

impl fmt::Debug for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderError")
            .field("category", &self.category)
            .field("message", &self.message)
            .finish()
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for ProviderError {}

pub(crate) fn captured_at_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{seconds}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_map_to_stable_provider_categories() {
        assert_eq!(
            ProviderError::from_status(StatusCode::UNAUTHORIZED).category(),
            ProviderErrorCategory::Unauthorized
        );
        assert_eq!(
            ProviderError::from_status(StatusCode::PAYMENT_REQUIRED).category(),
            ProviderErrorCategory::InsufficientBalance
        );
        assert_eq!(
            ProviderError::from_status(StatusCode::TOO_MANY_REQUESTS).category(),
            ProviderErrorCategory::RateLimited
        );
        for status in [
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::SERVICE_UNAVAILABLE,
        ] {
            assert_eq!(
                ProviderError::from_status(status).category(),
                ProviderErrorCategory::Server
            );
        }
        assert_eq!(
            ProviderError::from_status(StatusCode::FOUND).category(),
            ProviderErrorCategory::InvalidResponse
        );
    }

    #[test]
    fn provider_error_never_contains_upstream_body_or_secret() {
        let secret = "never-display-this-key";
        let error = ProviderError::from_status(StatusCode::UNAUTHORIZED);
        let debug = format!("{error:?}");
        let display = error.to_string();

        assert!(!debug.contains(secret));
        assert!(!display.contains(secret));
        assert!(!debug.contains("upstream"));
    }
}
