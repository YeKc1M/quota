//! Normalize the current Kimi Code `/usages` response. Contract reference:
//! MoonshotAI/kimi-code, packages/oauth/src/managed-usage.ts.

use crate::usage::{ProviderUsage, UsageWindow};
use serde_json::Value;
use std::time::Duration;

use super::local::{read_local_credentials, LocalCredentialError};

const USAGE_URL: &str = "https://api.kimi.com/coding/v1/usages";

pub fn normalize_usage_response(raw: &Value) -> ProviderUsage {
    let windows: Vec<UsageWindow> = [
        ("limit_5h", "5h"),
        ("limit_7d", "Wk"),
        ("limit_month_total", "Month"),
        ("limit_month_code", "Code"),
    ]
    .into_iter()
    .filter_map(|(key, label)| {
        let entry = raw.get("usages")?.get(key)?;
        let value = entry.get("used_ratio")?;
        let used = value
            .as_f64()
            .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite())?;
        let reset_at = entry
            .get("reset_time")
            .and_then(Value::as_str)
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.timestamp());
        Some(UsageWindow {
            label: label.to_string(),
            remaining_percent: Some(((1.0 - used) * 100.0).round().clamp(0.0, 100.0) as i32),
            reset_at,
        })
    })
    .collect();
    let note = windows.is_empty().then(|| "No usage data".to_string());

    ProviderUsage {
        provider: "kimi".to_string(),
        account_label: None,
        windows,
        note,
    }
}

#[derive(Debug)]
pub enum LocalUsageError {
    Credentials(LocalCredentialError),
    Unauthorized,
    Request(String),
}

impl std::fmt::Display for LocalUsageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Credentials(inner) => write!(formatter, "{}", inner),
            Self::Unauthorized => write!(
                formatter,
                "Kimi Code rejected the stored token. Run Kimi Code to refresh it, or use /login."
            ),
            Self::Request(detail) => {
                write!(formatter, "Kimi Code usage request failed: {}", detail)
            }
        }
    }
}

pub async fn fetch_local_usage() -> Result<ProviderUsage, LocalUsageError> {
    let credentials = read_local_credentials().map_err(LocalUsageError::Credentials)?;
    fetch_usage_at(USAGE_URL, &credentials.access_token).await
}

// The endpoint is private so tests can use a local server without exposing a
// production override that could send the user's OAuth token elsewhere.
async fn fetch_usage_at(url: &str, access_token: &str) -> Result<ProviderUsage, LocalUsageError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| LocalUsageError::Request("could not create HTTP client".to_string()))?;
    let response = client
        .get(url)
        .bearer_auth(access_token)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|err| {
            LocalUsageError::Request(
                if err.is_timeout() {
                    "request timed out"
                } else {
                    "could not send request"
                }
                .to_string(),
            )
        })?;

    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(LocalUsageError::Unauthorized);
    }
    if !status.is_success() {
        return Err(LocalUsageError::Request(format!(
            "HTTP {}",
            status.as_u16()
        )));
    }
    let body = response
        .bytes()
        .await
        .map_err(|_| LocalUsageError::Request("could not read response".to_string()))?;
    let raw: Value = serde_json::from_slice(&body)
        .map_err(|_| LocalUsageError::Request("invalid JSON response".to_string()))?;
    Ok(normalize_usage_response(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn request(status: u16, body: &str) -> Result<ProviderUsage, LocalUsageError> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/usages", listener.local_addr().unwrap());
        let body = body.to_string();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(request.starts_with("get /usages http/1.1\r\n"));
            assert!(request.contains("authorization: bearer test-secret\r\n"));
            assert!(request.contains("accept: application/json\r\n"));
            write!(
                stream,
                "HTTP/1.1 {} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status,
                body.len(),
                body
            )
            .unwrap();
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(fetch_usage_at(&url, "test-secret"));
        server.join().unwrap();
        result
    }

    #[test]
    fn fetches_with_bearer_auth_and_normalizes_the_response() {
        let usage = request(200, r#"{"usages":{"limit_5h":{"used_ratio":0.18}}}"#).unwrap();
        assert_eq!(usage.compact_token().as_deref(), Some("5h 82%"));
    }

    #[test]
    fn rejected_tokens_ask_the_cli_to_renew_without_exposing_the_response() {
        for status in [401, 403] {
            let error = request(status, "test-secret response-secret").unwrap_err();
            assert!(matches!(error, LocalUsageError::Unauthorized));
            assert!(error.to_string().contains("/login"));
            assert!(!format!("{} {:?}", error, error).contains("secret"));
        }
    }

    #[test]
    fn reports_http_and_json_errors_without_raw_bodies() {
        for (status, body, expected) in [
            (500, "response-secret", "HTTP 500"),
            (302, "response-secret", "HTTP 302"),
            (
                200,
                r#"{"test-secret":"response-secret"#,
                "invalid JSON response",
            ),
        ] {
            let error = request(status, body).unwrap_err();
            let rendered = format!("{} {:?}", error, error);
            assert!(rendered.contains(expected));
            assert!(!rendered.contains("secret"));
        }
    }
}
