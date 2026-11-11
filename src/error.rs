//! Error type shared by every data source.
//!
//! Messages are written for the end user (they are surfaced verbatim as MCP
//! tool errors), so each variant says what went wrong and what to do next.

use thiserror::Error;

/// Errors produced while talking to JLCPCB / LCSC or validating tool input.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum ApiError {
    /// DNS, TLS or connection failure.
    #[error(
        "network error contacting {service}: {message}. Check your internet connection and retry."
    )]
    Network {
        service: &'static str,
        message: String,
    },

    /// The request did not complete within the configured timeout.
    #[error(
        "{service} did not respond within {secs}s. The service may be slow or unreachable; retry shortly."
    )]
    Timeout { service: &'static str, secs: u64 },

    /// HTTP 429 / 403-style throttling.
    #[error(
        "{service} is rate limiting or blocking requests (HTTP {status}). Wait about a minute before retrying, and avoid issuing many searches in parallel."
    )]
    RateLimited { service: &'static str, status: u16 },

    /// Any other non-success HTTP status.
    #[error("{service} returned HTTP {status}{}", body_hint(.body))]
    Http {
        service: &'static str,
        status: u16,
        body: String,
    },

    /// The HTTP call succeeded but the JSON envelope reported a failure.
    #[error("{service} rejected the request (code {code}): {message}")]
    Api {
        service: &'static str,
        code: i64,
        message: String,
    },

    /// The response could not be decoded; the upstream format probably changed.
    #[error(
        "could not understand the {service} response ({message}). The upstream API format may have changed."
    )]
    Decode {
        service: &'static str,
        message: String,
    },

    /// The requested part / category does not exist.
    #[error("{0}")]
    NotFound(String),

    /// The tool arguments are invalid.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// An optional integration is not configured.
    #[error("{0}")]
    NotConfigured(String),
}

fn body_hint(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        let snippet: String = trimmed.chars().take(160).collect();
        format!(": {snippet}")
    }
}

impl ApiError {
    /// Whether retrying the same request might succeed.
    pub fn is_retryable(&self) -> bool {
        match self {
            ApiError::Network { .. } | ApiError::Timeout { .. } | ApiError::RateLimited { .. } => {
                true
            }
            ApiError::Http { status, .. } => *status >= 500,
            _ => false,
        }
    }
}

/// Convenience alias.
pub type Result<T, E = ApiError> = std::result::Result<T, E>;
