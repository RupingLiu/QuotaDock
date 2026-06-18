#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfficialLinks {
    pub usage_dashboard: &'static str,
    pub codex_help: &'static str,
    pub flexible_credits_help: &'static str,
}

pub const USAGE_DASHBOARD_URL: &str = "https://chatgpt.com/codex/settings/usage";
pub const CODEX_HELP_URL: &str =
    "https://help.openai.com/en/articles/11369540-using-codex-with-your-chatgpt-plan";
pub const FLEXIBLE_CREDITS_HELP_URL: &str =
    "https://help.openai.com/en/articles/12642688-using-credits-for-flexible-usage-in-chatgpt-freegopluspro";

pub fn official_links() -> OfficialLinks {
    OfficialLinks {
        usage_dashboard: USAGE_DASHBOARD_URL,
        codex_help: CODEX_HELP_URL,
        flexible_credits_help: FLEXIBLE_CREDITS_HELP_URL,
    }
}

#[cfg(test)]
mod tests {
    use crate::official_links::{official_links, USAGE_DASHBOARD_URL};

    #[test]
    fn usage_dashboard_link_is_centralized() {
        let links = official_links();

        assert_eq!(links.usage_dashboard, USAGE_DASHBOARD_URL);
        assert!(links.usage_dashboard.starts_with("https://chatgpt.com/"));
        assert!(links.codex_help.starts_with("https://help.openai.com/"));
        assert!(links
            .flexible_credits_help
            .starts_with("https://help.openai.com/"));
    }
}
