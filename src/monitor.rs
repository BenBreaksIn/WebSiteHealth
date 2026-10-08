use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use reqwest::{Client, Url, redirect::Policy};
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct Target {
    request_url: Url,
    display_url: String,
}

impl Target {
    pub fn display_url(&self) -> &str {
        &self.display_url
    }

    pub fn request_url(&self) -> &str {
        self.request_url.as_str()
    }

    pub fn parse(raw: &str) -> Result<Self, &'static str> {
        // Url::parse also accepts inputs such as "http:example.com". Require
        // explicit absolute URLs and reject characters it would silently trim.
        if raw.chars().any(|c| c.is_whitespace() || c.is_control())
            || !raw
                .split_once("://")
                .is_some_and(|(scheme, _)| is_http_scheme(scheme))
        {
            return Err("expected an absolute http:// or https:// URL without whitespace");
        }
        let mut url = Url::parse(raw).map_err(|_| "invalid URL")?;
        if !is_http_scheme(url.scheme()) || url.host_str().is_none() {
            return Err("expected an HTTP(S) URL with a host");
        }
        if has_credentials(&url) {
            return Err("URLs containing usernames or passwords are not supported");
        }
        // Fragments are never sent to the server; queries may contain secrets.
        url.set_fragment(None);
        let mut display = url.clone();
        display.set_query(None);
        Ok(Self {
            request_url: url,
            display_url: display.into(),
        })
    }
}

fn is_http_scheme(scheme: &str) -> bool {
    scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
}

fn has_credentials(url: &Url) -> bool {
    !url.username().is_empty() || url.password().is_some()
}

pub fn client(timeout: Duration) -> Result<Client, reqwest::Error> {
    Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(timeout)
        .referer(false)
        .retry(reqwest::retry::never())
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() > 10 {
                attempt.error("redirect limit reached")
            } else if !is_http_scheme(attempt.url().scheme()) || has_credentials(attempt.url()) {
                attempt.error("unsupported redirect target")
            } else {
                attempt.follow()
            }
        }))
        .build()
}

#[derive(Debug, Serialize)]
pub struct CheckResult {
    /// One-based position in the original URL list, useful when queries differ.
    pub target: usize,
    pub timestamp: String,
    pub url: String,
    pub healthy: bool,
    pub status: Option<u16>,
    pub error: Option<&'static str>,
    pub response_time_ms: u128,
}

pub async fn check(client: Client, target: Target, index: usize) -> CheckResult {
    let start = Instant::now();
    // Measure receipt of the final response headers, including redirects.
    // Bodies are deliberately not downloaded or retained.
    let response = client.get(target.request_url).send().await;
    let response_time_ms = start.elapsed().as_millis();
    let (status, error, healthy) = match response {
        Ok(response) => (
            Some(response.status().as_u16()),
            None,
            response.status().is_success(),
        ),
        // Do not format reqwest errors: they may contain a URL or a query token.
        Err(error) => (None, Some(error_summary(&error)), false),
    };
    CheckResult {
        target: index + 1,
        timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        url: target.display_url,
        healthy,
        status,
        error,
        response_time_ms,
    }
}

fn error_summary(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "request timed out"
    } else if error.is_redirect() {
        "redirect failed"
    } else if error.is_connect() {
        "connection failed (DNS, TCP, or TLS)"
    } else {
        "request failed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_targets_without_echoing_input() {
        for raw in [
            "example.com",
            "ftp://example.com",
            "file:///etc/passwd",
            "http:example.com",
            "https://",
            "http://[invalid",
            "https://example.com:99999",
            " https://example.com",
            "https://example.com/\nsecret",
            "https://user:password@example.com",
            "https://user@example.com",
            "https://:password@example.com",
        ] {
            assert!(Target::parse(raw).is_err(), "accepted {raw:?}");
        }
    }

    #[test]
    fn preserves_query_for_request_but_omits_it_from_display() {
        let target = Target::parse("HTTPS://example.com/health?token=secret#fragment").unwrap();
        assert_eq!(
            target.request_url.as_str(),
            "https://example.com/health?token=secret"
        );
        assert_eq!(target.display_url, "https://example.com/health");
    }

    #[test]
    fn accepts_localhost_and_ipv6() {
        assert!(Target::parse("http://localhost:8080/").is_ok());
        assert!(Target::parse("http://[::1]:8080/").is_ok());
    }
}
