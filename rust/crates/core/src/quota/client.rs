use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::credentials::{CredentialReader, Credentials};
use super::model::{Provider, QuotaError, QuotaSnapshot};
use crate::dates::parse_http_date;
use crate::http::{HttpClient, HttpRequest, UreqClient};

const MAX_BODY_BYTES: usize = 1_048_576;
const DEFAULT_RETRY: Duration = Duration::from_secs(300);

pub type CredentialLoader = Arc<dyn Fn(Provider, bool) -> Result<Credentials, QuotaError> + Send + Sync>;
pub type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// Fetches one provider's usage from its fixed endpoint using the local login.
#[derive(Clone)]
pub struct QuotaClient {
    load_credentials: CredentialLoader,
    http: Arc<dyn HttpClient>,
    now: Clock,
}

impl Default for QuotaClient {
    fn default() -> Self {
        let reader = CredentialReader::from_process();
        Self::new(Arc::new(move |provider, interaction| reader.load(provider, interaction)), UreqClient::shared())
    }
}

impl QuotaClient {
    pub fn new(load_credentials: CredentialLoader, http: Arc<dyn HttpClient>) -> Self {
        Self { load_credentials, http, now: Arc::new(SystemTime::now) }
    }

    pub fn with_clock(mut self, now: Clock) -> Self {
        self.now = now;
        self
    }

    pub fn fetch(&self, provider: Provider, allow_interaction: bool) -> Result<QuotaSnapshot, QuotaError> {
        let credentials = (self.load_credentials)(provider, allow_interaction)?;
        let mut request = HttpRequest::get(provider.endpoint())
            .header("Authorization", format!("Bearer {}", credentials.access_token))
            .header("Accept", "application/json")
            .header("User-Agent", "Angelly");
        request.timeout = Duration::from_secs(10);
        match provider {
            Provider::Codex => {
                if let Some(account) = credentials.account_id.as_deref().filter(|a| !a.is_empty()) {
                    request = request.header("ChatGPT-Account-Id", account);
                }
            }
            Provider::Claude => request = request.header("anthropic-beta", "oauth-2025-04-20"),
        }
        let response = self.http.get(&request).map_err(|_| QuotaError::Network)?;
        match response.status {
            200 => {
                if response.body.len() > MAX_BODY_BYTES {
                    return Err(QuotaError::Unavailable);
                }
                QuotaSnapshot::decode(&response.body, provider)
            }
            401 => Err(QuotaError::Expired),
            403 => Err(QuotaError::Forbidden),
            429 => Err(QuotaError::RateLimited { until: self.retry_date(response.header("Retry-After")) }),
            _ => Err(QuotaError::Unavailable),
        }
    }

    /// When a rate-limited request may be retried: `Retry-After` as seconds or an HTTP
    /// date, else five minutes from now.
    fn retry_date(&self, header: Option<&str>) -> SystemTime {
        let now = (self.now)();
        if let Some(header) = header {
            if let Ok(seconds) = header.parse::<f64>() {
                if seconds.is_finite() && seconds >= 0.0 {
                    return now + Duration::from_secs_f64(seconds.clamp(1.0, 10.0 * 365.0 * 86_400.0));
                }
            }
            if let Some(date) = parse_http_date(header).filter(|d| *d > now) {
                return date;
            }
        }
        now + DEFAULT_RETRY
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{FnHttpClient, HttpError, HttpResponse};
    use std::time::UNIX_EPOCH;

    fn credentials(account: Option<&str>) -> CredentialLoader {
        let account = account.map(str::to_string);
        Arc::new(move |_, interaction| {
            assert!(!interaction);
            Ok(Credentials { access_token: "secret-fixture".into(), account_id: account.clone() })
        })
    }

    #[test]
    fn usage_requests_stay_on_provider_endpoint_with_required_headers() {
        for provider in Provider::ALL {
            let http = FnHttpClient(move |request: &HttpRequest| {
                assert_eq!(request.header_value("Authorization"), Some("Bearer secret-fixture"));
                assert_eq!(request.header_value("Accept"), Some("application/json"));
                assert_eq!(request.header_value("User-Agent"), Some("Angelly"));
                let body = if provider == Provider::Codex {
                    assert_eq!(request.url, "https://chatgpt.com/backend-api/wham/usage");
                    assert_eq!(request.header_value("ChatGPT-Account-Id"), Some("account-fixture"));
                    r#"{"rate_limit":{"primary_window":{"used_percent":25,"limit_window_seconds":18000}}}"#
                } else {
                    assert_eq!(request.url, "https://api.anthropic.com/api/oauth/usage");
                    assert_eq!(request.header_value("anthropic-beta"), Some("oauth-2025-04-20"));
                    r#"{"five_hour":{"utilization":25}}"#
                };
                Ok(HttpResponse::new(200, body))
            });
            let client = QuotaClient::new(credentials(Some("account-fixture")), Arc::new(http));
            assert_eq!(client.fetch(provider, false).unwrap().ring_window().unwrap().remaining_percent, 75.0);
        }
    }

    #[test]
    fn empty_account_id_sends_no_account_header() {
        let http = FnHttpClient(|request: &HttpRequest| {
            assert_eq!(request.header_value("ChatGPT-Account-Id"), None);
            Ok(HttpResponse::new(200, r#"{"rate_limit":{"primary_window":{"used_percent":1,"limit_window_seconds":18000}}}"#))
        });
        let client = QuotaClient::new(credentials(Some("")), Arc::new(http));
        assert!(client.fetch(Provider::Codex, false).is_ok());
    }

    #[test]
    fn http_failures_are_sanitized_and_rate_limits_respect_retry_after() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        for status in [401u16, 403, 429, 500, 302] {
            let http = FnHttpClient(move |_: &HttpRequest| {
                Ok(HttpResponse::new(status, "do not show this response or secret-fixture").with_header("Retry-After", "600"))
            });
            let client = QuotaClient::new(credentials(None), Arc::new(http)).with_clock(Arc::new(move || now));
            let expected = match status {
                401 => QuotaError::Expired,
                403 => QuotaError::Forbidden,
                429 => QuotaError::RateLimited { until: now + Duration::from_secs(600) },
                _ => QuotaError::Unavailable,
            };
            assert_eq!(client.fetch(Provider::Codex, false), Err(expected), "{status}");
        }
    }

    #[test]
    fn retry_after_http_dates_and_fallbacks() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let client = QuotaClient::new(credentials(None), Arc::new(FnHttpClient(|_: &HttpRequest| Err(HttpError(String::new())))))
            .with_clock(Arc::new(move || now));
        assert_eq!(client.retry_date(Some("0")), now + Duration::from_secs(1));
        assert_eq!(client.retry_date(Some("-5")), now + DEFAULT_RETRY);
        assert_eq!(client.retry_date(Some("soon")), now + DEFAULT_RETRY);
        assert_eq!(client.retry_date(None), now + DEFAULT_RETRY);
        assert_eq!(client.retry_date(Some("Sun, 06 Nov 1994 08:49:37 GMT")), now + DEFAULT_RETRY);
        assert_eq!(client.retry_date(Some("Fri, 15 Jan 2027 08:10:00 GMT")), now + Duration::from_secs(600));
    }

    #[test]
    fn transport_errors_and_oversized_bodies() {
        let failing = FnHttpClient(|_: &HttpRequest| Err(HttpError("connection refused to secret-fixture".into())));
        let client = QuotaClient::new(credentials(None), Arc::new(failing));
        assert_eq!(client.fetch(Provider::Claude, false), Err(QuotaError::Network));

        let large = FnHttpClient(|_: &HttpRequest| Ok(HttpResponse::new(200, vec![b' '; MAX_BODY_BYTES + 1])));
        let client = QuotaClient::new(credentials(None), Arc::new(large));
        assert_eq!(client.fetch(Provider::Claude, false), Err(QuotaError::Unavailable));
    }

    #[test]
    fn credential_errors_pass_through_without_a_request() {
        let loader: CredentialLoader = Arc::new(|_, _| Err(QuotaError::NotSignedIn));
        let http = FnHttpClient(|_: &HttpRequest| panic!("no request without credentials"));
        let client = QuotaClient::new(loader, Arc::new(http));
        assert_eq!(client.fetch(Provider::Codex, false), Err(QuotaError::NotSignedIn));
    }
}
