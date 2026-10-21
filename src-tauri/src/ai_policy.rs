//! No environment switch can enable credentials/network in development or tests.
pub(crate) const REAL_ENABLED: bool = cfg!(all(feature = "real-provider", not(debug_assertions), not(test)));
pub(crate) fn require_real() -> crate::error::Result<()> {
    if REAL_ENABLED { Ok(()) } else { Err(crate::ai_network::failure("mock_only")) }
}
#[cfg(test)]
mod tests {
    #[test] fn development_cannot_access_real_credentials_or_network() {
        assert!(!super::REAL_ENABLED);
        assert_eq!(super::require_real().unwrap_err().code, "mock_only");
        assert_eq!(crate::credentials::read("https://unit.invalid/chat/completions").err().unwrap().code,"mock_only");
        assert!(!crate::credentials::present("https://unit.invalid/chat/completions"));
        assert_eq!(crate::credentials::put("https://unit.invalid/chat/completions",crate::credentials::Secret(b"fake-test-only".to_vec())).unwrap_err().code,"mock_only");
    }
    #[tokio::test] async fn development_network_stops_before_dns() {
        let url=crate::ai_network::endpoint("https://unit.invalid/chat/completions").unwrap();
        assert_eq!(crate::ai_network::client(&url).await.err().unwrap().code,"mock_only");
    }
}
