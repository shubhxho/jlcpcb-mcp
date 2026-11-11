//! Shared HTTP client: timeouts, polite throttling, retries with backoff and a
//! small in-memory TTL cache so repeated tool calls don't hammer upstream.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use reqwest::{Method, StatusCode};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::error::{ApiError, Result};

const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; jlcpcb-mcp/",
    env!("CARGO_PKG_VERSION"),
    "; +https://github.com/shubhxho/jlcpcb-mcp)"
);

/// Tunables for [`HttpClient`].
#[derive(Debug, Clone)]
pub struct HttpConfig {
    /// Per-request timeout.
    pub timeout: Duration,
    /// Number of retries after the first attempt for retryable errors.
    pub retries: u32,
    /// Base delay for exponential backoff.
    pub backoff_base: Duration,
    /// Minimum spacing between request starts (global, across all tools).
    pub min_interval: Duration,
    /// Maximum number of cached responses.
    pub cache_capacity: usize,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(20),
            retries: 2,
            backoff_base: Duration::from_millis(600),
            min_interval: Duration::from_millis(150),
            cache_capacity: 512,
        }
    }
}

impl HttpConfig {
    /// Fast settings for tests (no throttle, tiny backoff).
    pub fn for_tests() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            retries: 1,
            backoff_base: Duration::from_millis(5),
            min_interval: Duration::ZERO,
            cache_capacity: 64,
        }
    }
}

struct CacheEntry {
    expires: Instant,
    value: Value,
}

/// A thin JSON-over-HTTP client.
pub struct HttpClient {
    client: reqwest::Client,
    config: HttpConfig,
    cache: Mutex<HashMap<String, CacheEntry>>,
    last_request: Mutex<Option<Instant>>,
}

/// A request description used by [`HttpClient::send_json`].
pub struct JsonRequest<'a> {
    pub service: &'static str,
    pub method: Method,
    pub url: &'a str,
    pub body: Option<String>,
    pub headers: Vec<(&'static str, String)>,
    /// Cache lifetime; `None` disables caching for this request.
    pub ttl: Option<Duration>,
}

impl HttpClient {
    /// Build a client with the given configuration.
    pub fn new(config: HttpConfig) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(config.timeout)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client (TLS backend unavailable?)");
        Self {
            client,
            config,
            cache: Mutex::new(HashMap::new()),
            last_request: Mutex::new(None),
        }
    }

    /// GET a JSON document.
    pub async fn get_json(
        &self,
        service: &'static str,
        url: &str,
        ttl: Option<Duration>,
    ) -> Result<Value> {
        self.send_json(JsonRequest {
            service,
            method: Method::GET,
            url,
            body: None,
            headers: Vec::new(),
            ttl,
        })
        .await
    }

    /// POST a JSON body and decode a JSON response.
    pub async fn post_json(
        &self,
        service: &'static str,
        url: &str,
        body: &Value,
        ttl: Option<Duration>,
    ) -> Result<Value> {
        self.send_json(JsonRequest {
            service,
            method: Method::POST,
            url,
            body: Some(body.to_string()),
            headers: Vec::new(),
            ttl,
        })
        .await
    }

    /// Number of live cache entries (for diagnostics).
    pub async fn cache_len(&self) -> usize {
        let now = Instant::now();
        self.cache
            .lock()
            .await
            .values()
            .filter(|e| e.expires > now)
            .count()
    }

    /// Send a request with caching, throttling and retries.
    pub async fn send_json(&self, req: JsonRequest<'_>) -> Result<Value> {
        let key = format!(
            "{} {} {}",
            req.method,
            req.url,
            req.body.as_deref().unwrap_or("")
        );
        if req.ttl.is_some() {
            let cache = self.cache.lock().await;
            if let Some(entry) = cache.get(&key)
                && entry.expires > Instant::now()
            {
                return Ok(entry.value.clone());
            }
        }

        let mut attempt = 0;
        let value = loop {
            match self.send_once(&req).await {
                Ok(v) => break v,
                Err(err) if err.is_retryable() && attempt < self.config.retries => {
                    let factor = if matches!(err, ApiError::RateLimited { .. }) {
                        4
                    } else {
                        1
                    };
                    let delay = self.config.backoff_base * factor * 2u32.pow(attempt);
                    tracing::warn!(service = req.service, %err, ?delay, "retrying request");
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        };

        if let Some(ttl) = req.ttl {
            let mut cache = self.cache.lock().await;
            let now = Instant::now();
            if cache.len() >= self.config.cache_capacity {
                cache.retain(|_, e| e.expires > now);
                if cache.len() >= self.config.cache_capacity {
                    // Still full of live entries: drop the one expiring soonest.
                    if let Some(k) = cache
                        .iter()
                        .min_by_key(|(_, e)| e.expires)
                        .map(|(k, _)| k.clone())
                    {
                        cache.remove(&k);
                    }
                }
            }
            cache.insert(
                key,
                CacheEntry {
                    expires: now + ttl,
                    value: value.clone(),
                },
            );
        }
        Ok(value)
    }

    async fn throttle(&self) {
        if self.config.min_interval.is_zero() {
            return;
        }
        let mut last = self.last_request.lock().await;
        if let Some(prev) = *last {
            let elapsed = prev.elapsed();
            if elapsed < self.config.min_interval {
                tokio::time::sleep(self.config.min_interval - elapsed).await;
            }
        }
        *last = Some(Instant::now());
    }

    async fn send_once(&self, req: &JsonRequest<'_>) -> Result<Value> {
        self.throttle().await;
        let service = req.service;
        let mut builder = self
            .client
            .request(req.method.clone(), req.url)
            .header("Accept", "application/json, text/plain, */*");
        if let Some(body) = &req.body {
            builder = builder
                .header("Content-Type", "application/json")
                .body(body.clone());
        }
        for (name, value) in &req.headers {
            builder = builder.header(*name, value);
        }

        let response = builder
            .send()
            .await
            .map_err(|e| map_reqwest(service, &e, self.config.timeout))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| map_reqwest(service, &e, self.config.timeout))?;

        if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::FORBIDDEN {
            return Err(ApiError::RateLimited {
                service,
                status: status.as_u16(),
            });
        }
        if !status.is_success() {
            return Err(ApiError::Http {
                service,
                status: status.as_u16(),
                body: text,
            });
        }
        serde_json::from_str(&text).map_err(|e| {
            let snippet: String = text.chars().take(120).collect();
            ApiError::Decode {
                service,
                message: format!("{e}; body starts with {snippet:?}"),
            }
        })
    }
}

fn map_reqwest(service: &'static str, err: &reqwest::Error, timeout: Duration) -> ApiError {
    if err.is_timeout() {
        ApiError::Timeout {
            service,
            secs: timeout.as_secs().max(1),
        }
    } else {
        // Include the source chain: reqwest's top-level message is terse.
        let mut message = err.to_string();
        let mut source = std::error::Error::source(err);
        while let Some(s) = source {
            message.push_str(": ");
            message.push_str(&s.to_string());
            source = s.source();
        }
        ApiError::Network { service, message }
    }
}
