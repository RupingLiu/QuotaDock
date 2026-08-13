use reqwest::blocking::{Client, Request, Response};
use reqwest::header::ACCEPT;
use reqwest::{redirect, StatusCode};
use std::error::Error as StdError;
use std::fmt;
use std::io::{self, Read};
use std::time::Duration;

const TOTAL_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024;
const USER_AGENT: &str = concat!("QuotaDock/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficialEndpoint {
    DeepSeekBalance,
    KimiBalance,
}

impl OfficialEndpoint {
    pub const fn url(self) -> &'static str {
        match self {
            Self::DeepSeekBalance => "https://api.deepseek.com/user/balance",
            Self::KimiBalance => "https://api.moonshot.cn/v1/users/me/balance",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpErrorKind {
    Timeout,
    Network,
    InvalidResponse,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HttpError {
    kind: HttpErrorKind,
}

impl HttpError {
    fn new(kind: HttpErrorKind) -> Self {
        Self { kind }
    }

    pub fn kind(self) -> HttpErrorKind {
        self.kind
    }
}

impl fmt::Debug for HttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            HttpErrorKind::Timeout => "请求超时。",
            HttpErrorKind::Network => "网络请求失败。",
            HttpErrorKind::InvalidResponse => "服务返回了无效响应。",
        })
    }
}

impl std::error::Error for HttpError {}

#[derive(Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: StatusCode,
    pub body: Vec<u8>,
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("body", &"<redacted>")
            .finish()
    }
}

#[derive(Clone)]
pub struct HttpClient {
    client: Client,
}

impl fmt::Debug for HttpClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HttpClient { redacted configuration }")
    }
}

impl HttpClient {
    pub fn new() -> Result<Self, HttpError> {
        Self::build(TOTAL_TIMEOUT, CONNECT_TIMEOUT, true, true)
    }

    fn build(
        total_timeout: Duration,
        connect_timeout: Duration,
        https_only: bool,
        use_system_proxy: bool,
    ) -> Result<Self, HttpError> {
        // reqwest's no-provider mode lets the application make the TLS choice explicitly.
        // Installation is process-global and idempotent: an already installed provider wins.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut builder = Client::builder()
            .timeout(total_timeout)
            .connect_timeout(connect_timeout)
            .user_agent(USER_AGENT)
            .redirect(redirect::Policy::none())
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd();
        if https_only {
            builder = builder.https_only(true);
        }
        if !use_system_proxy {
            builder = builder.no_proxy();
        }
        builder
            .build()
            .map(|client| Self { client })
            .map_err(|_| HttpError::new(HttpErrorKind::Network))
    }

    pub fn get_bearer(
        &self,
        endpoint: OfficialEndpoint,
        api_key: &str,
    ) -> Result<HttpResponse, HttpError> {
        let request = self.build_bearer_request(endpoint, api_key)?;
        self.execute(request)
    }

    fn build_bearer_request(
        &self,
        endpoint: OfficialEndpoint,
        api_key: &str,
    ) -> Result<Request, HttpError> {
        self.client
            .get(endpoint.url())
            .header(ACCEPT, "application/json")
            .bearer_auth(api_key)
            .build()
            .map_err(|error| map_reqwest_error(&error))
    }

    fn execute(&self, request: Request) -> Result<HttpResponse, HttpError> {
        let response = self
            .client
            .execute(request)
            .map_err(|error| map_reqwest_error(&error))?;
        read_response(response)
    }

    #[cfg(test)]
    fn for_http_test(timeout: Duration) -> Self {
        Self::build(timeout, timeout, false, false).expect("test HTTP client should build")
    }

    #[cfg(test)]
    fn get_test_url(&self, url: &str) -> Result<HttpResponse, HttpError> {
        let request = self
            .client
            .get(url)
            .build()
            .map_err(|error| map_reqwest_error(&error))?;
        self.execute(request)
    }

    #[cfg(test)]
    fn get_test_url_bearer(&self, url: &str, api_key: &str) -> Result<HttpResponse, HttpError> {
        let request = self
            .client
            .get(url)
            .bearer_auth(api_key)
            .build()
            .map_err(|error| map_reqwest_error(&error))?;
        self.execute(request)
    }
}

fn read_response(mut response: Response) -> Result<HttpResponse, HttpError> {
    let status = response.status();
    if !status.is_success() {
        // Error classification is status-only. Never buffer or inspect an upstream error body.
        return Ok(HttpResponse {
            status,
            body: Vec::new(),
        });
    }

    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BODY_BYTES as u64)
    {
        return Err(HttpError::new(HttpErrorKind::InvalidResponse));
    }

    let mut body = Vec::new();
    response
        .by_ref()
        .take((MAX_RESPONSE_BODY_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .map_err(map_read_error)?;
    if body.len() > MAX_RESPONSE_BODY_BYTES {
        return Err(HttpError::new(HttpErrorKind::InvalidResponse));
    }

    Ok(HttpResponse { status, body })
}

fn map_reqwest_error(error: &reqwest::Error) -> HttpError {
    if error.is_timeout() {
        HttpError::new(HttpErrorKind::Timeout)
    } else {
        HttpError::new(HttpErrorKind::Network)
    }
}

fn map_read_error(error: io::Error) -> HttpError {
    let inner_reqwest_timeout = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<reqwest::Error>())
        .is_some_and(reqwest::Error::is_timeout);
    if error.kind() == io::ErrorKind::TimedOut
        || inner_reqwest_timeout
        || error_chain_contains_reqwest_timeout(&error)
    {
        HttpError::new(HttpErrorKind::Timeout)
    } else {
        HttpError::new(HttpErrorKind::Network)
    }
}

fn error_chain_contains_reqwest_timeout(error: &(dyn StdError + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(source) = current {
        if source
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_timeout)
        {
            return true;
        }
        current = source.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::AUTHORIZATION;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    fn spawn_server(
        response: Vec<u8>,
        response_delay: Duration,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            if !response_delay.is_zero() {
                thread::sleep(response_delay);
            }
            let _ = stream.write_all(&response);
        });
        (format!("http://{address}"), handle)
    }

    fn spawn_headers_then_delayed_body(
        status: &str,
        body: Vec<u8>,
        body_delay: Duration,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            let headers = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(headers.as_bytes());
            let _ = stream.flush();
            thread::sleep(body_delay);
            let _ = stream.write_all(&body);
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn official_endpoints_are_fixed_https_hosts_and_paths() {
        assert_eq!(
            OfficialEndpoint::DeepSeekBalance.url(),
            "https://api.deepseek.com/user/balance"
        );
        assert_eq!(
            OfficialEndpoint::KimiBalance.url(),
            "https://api.moonshot.cn/v1/users/me/balance"
        );
        for endpoint in [
            OfficialEndpoint::DeepSeekBalance,
            OfficialEndpoint::KimiBalance,
        ] {
            let url = reqwest::Url::parse(endpoint.url()).unwrap();
            assert_eq!(url.scheme(), "https");
            assert!(matches!(
                url.host_str(),
                Some("api.deepseek.com" | "api.moonshot.cn")
            ));
        }
    }

    #[test]
    fn bearer_header_is_sensitive_and_debug_output_is_redacted() {
        let client = HttpClient::new().unwrap();
        let secret = "test-secret-never-display";

        let request = client
            .build_bearer_request(OfficialEndpoint::DeepSeekBalance, secret)
            .unwrap();

        assert_eq!(request.method(), reqwest::Method::GET);
        assert_eq!(
            request.url().as_str(),
            OfficialEndpoint::DeepSeekBalance.url()
        );
        assert!(request.headers()[AUTHORIZATION].is_sensitive());
        assert_eq!(request.headers()[ACCEPT], "application/json");
        assert!(!format!("{request:?}").contains(secret));
    }

    #[test]
    fn redirects_are_not_followed_to_a_malicious_location() {
        let response = b"HTTP/1.1 302 Found\r\nLocation: https://evil.example/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec();
        let (url, server) = spawn_server(response, Duration::ZERO);
        let client = HttpClient::for_http_test(Duration::from_secs(1));

        let result = client.get_test_url(&url).unwrap();
        server.join().unwrap();

        assert_eq!(result.status, StatusCode::FOUND);
        assert!(result.body.is_empty());
    }

    #[test]
    fn bearer_redirect_never_connects_to_or_authorizes_the_malicious_service() {
        let malicious = TcpListener::bind("127.0.0.1:0").unwrap();
        malicious.set_nonblocking(true).unwrap();
        let malicious_address = malicious.local_addr().unwrap();
        let redirect = TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect_address = redirect.local_addr().unwrap();
        let (request_tx, request_rx) = mpsc::channel();
        let redirect_server = thread::spawn(move || {
            let (mut stream, _) = redirect.accept().unwrap();
            let mut request = [0_u8; 4096];
            let count = stream.read(&mut request).unwrap_or_default();
            request_tx.send(request[..count].to_vec()).unwrap();
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{malicious_address}/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(response.as_bytes());
        });
        let client = HttpClient::for_http_test(Duration::from_secs(1));

        let result = client
            .get_test_url_bearer(
                &format!("http://{redirect_address}/balance"),
                "redirect-secret",
            )
            .unwrap();
        redirect_server.join().unwrap();
        let origin_request = String::from_utf8_lossy(&request_rx.recv().unwrap()).to_lowercase();
        let deadline = Instant::now() + Duration::from_millis(150);
        let mut malicious_received_connection = false;
        while Instant::now() < deadline {
            match malicious.accept() {
                Ok((_stream, _)) => {
                    malicious_received_connection = true;
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("malicious listener failed: {error}"),
            }
        }

        assert_eq!(result.status, StatusCode::FOUND);
        assert!(origin_request.contains("authorization: bearer redirect-secret"));
        assert!(!malicious_received_connection);
    }

    #[test]
    fn non_success_statuses_do_not_read_or_limit_large_error_bodies() {
        for (status_line, expected_status) in [
            ("401 Unauthorized", StatusCode::UNAUTHORIZED),
            ("429 Too Many Requests", StatusCode::TOO_MANY_REQUESTS),
        ] {
            let (url, server) = spawn_headers_then_delayed_body(
                status_line,
                vec![b'x'; MAX_RESPONSE_BODY_BYTES + 1024],
                Duration::from_millis(400),
            );
            let client = HttpClient::for_http_test(Duration::from_secs(1));
            let started = Instant::now();

            let result = client.get_test_url(&url).unwrap();

            assert_eq!(result.status, expected_status);
            assert!(result.body.is_empty());
            assert!(started.elapsed() < Duration::from_millis(250));
            server.join().unwrap();
        }
    }

    #[test]
    fn oversized_content_length_is_rejected_before_parsing() {
        let length = MAX_RESPONSE_BODY_BYTES + 1;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n{}",
            "x".repeat(length)
        )
        .into_bytes();
        let (url, server) = spawn_server(response, Duration::ZERO);
        let client = HttpClient::for_http_test(Duration::from_secs(1));

        let error = client.get_test_url(&url).unwrap_err();
        server.join().unwrap();

        assert_eq!(error.kind(), HttpErrorKind::InvalidResponse);
    }

    #[test]
    fn streamed_body_without_content_length_is_also_bounded() {
        let body = "x".repeat(MAX_RESPONSE_BODY_BYTES + 1);
        let response = format!("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{body}").into_bytes();
        let (url, server) = spawn_server(response, Duration::ZERO);
        let client = HttpClient::for_http_test(Duration::from_secs(1));

        let error = client.get_test_url(&url).unwrap_err();
        server.join().unwrap();

        assert_eq!(error.kind(), HttpErrorKind::InvalidResponse);
    }

    #[test]
    fn timeout_and_transport_failure_have_stable_categories() {
        let response =
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_vec();
        let (url, server) = spawn_server(response, Duration::from_millis(150));
        let client = HttpClient::for_http_test(Duration::from_millis(30));

        let timeout = client.get_test_url(&url).unwrap_err();
        server.join().unwrap();
        let network_client = HttpClient::for_http_test(Duration::from_secs(1));
        let network = network_client
            .get_test_url("this is not a valid URL")
            .unwrap_err();

        assert_eq!(timeout.kind(), HttpErrorKind::Timeout);
        assert_eq!(network.kind(), HttpErrorKind::Network);
    }

    #[test]
    fn timeout_while_reading_body_is_found_through_the_io_error_source_chain() {
        let (url, server) =
            spawn_headers_then_delayed_body("200 OK", b"{}".to_vec(), Duration::from_millis(150));
        let client = HttpClient::for_http_test(Duration::from_millis(30));

        let error = client.get_test_url(&url).unwrap_err();
        server.join().unwrap();

        assert_eq!(error.kind(), HttpErrorKind::Timeout);
    }

    #[test]
    fn response_debug_redacts_success_and_error_bodies() {
        let response = HttpResponse {
            status: StatusCode::OK,
            body: b"sensitive-upstream-body".to_vec(),
        };

        let debug = format!("{response:?}");

        assert!(debug.contains("200"));
        assert!(!debug.contains("sensitive-upstream-body"));
    }

    #[test]
    fn no_test_helper_accepts_an_arbitrary_url_in_production_api() {
        fn production_signature(
            client: &HttpClient,
            endpoint: OfficialEndpoint,
            secret: &str,
        ) -> Result<HttpResponse, HttpError> {
            client.get_bearer(endpoint, secret)
        }

        let _ = production_signature;
    }
}
