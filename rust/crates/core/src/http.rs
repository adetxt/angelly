use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub timeout: Duration,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self { url: url.into(), headers: Vec::new(), timeout: Duration::from_secs(10) }
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_string(), value.into()));
        self
    }

    /// Case-insensitive lookup of a request header.
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self { status, headers: Vec::new(), body: body.into() }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError(pub String);

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HttpError {}

/// Blocking HTTP GET, abstracted so network-facing logic can be tested with fixtures.
/// Implementations must not follow redirects: bearer tokens stay on the fixed endpoint.
pub trait HttpClient: Send + Sync {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError>;
}

/// Upper bound on bytes read from a response body.
const MAX_BODY_BYTES: u64 = 8 * 1024 * 1024;

pub struct UreqClient {
    agent: ureq::Agent,
}

impl UreqClient {
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout_connect(Duration::from_secs(10))
            .build();
        Self { agent }
    }

    pub fn shared() -> Arc<dyn HttpClient> {
        Arc::new(Self::new())
    }
}

impl Default for UreqClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient for UreqClient {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        let mut call = self.agent.get(&request.url).timeout(request.timeout);
        for (name, value) in &request.headers {
            call = call.set(name, value);
        }
        let response = match call.call() {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => response,
            Err(e) => return Err(HttpError(e.to_string())),
        };
        let status = response.status();
        let headers = response
            .headers_names()
            .into_iter()
            .filter_map(|name| response.header(&name).map(|v| (name.clone(), v.to_string())))
            .collect();
        let mut body = Vec::new();
        response
            .into_reader()
            .take(MAX_BODY_BYTES)
            .read_to_end(&mut body)
            .map_err(|e| HttpError(e.to_string()))?;
        Ok(HttpResponse { status, headers, body })
    }
}

/// An [`HttpClient`] backed by a closure.
pub struct FnHttpClient<F>(pub F);

impl<F> HttpClient for FnHttpClient<F>
where
    F: Fn(&HttpRequest) -> Result<HttpResponse, HttpError> + Send + Sync,
{
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        (self.0)(request)
    }
}
