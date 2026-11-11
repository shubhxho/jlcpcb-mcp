//! Optional official JLCPCB Open API (<https://api.jlcpcb.com>).
//!
//! Enabled only when `JLCPCB_APP_ID`, `JLCPCB_API_KEY` and
//! `JLCPCB_API_SECRET` are all set. Requests are signed with HMAC-SHA256 as
//! `"{METHOD}\n{PATH}\n{TIMESTAMP}\n{NONCE}\n{BODY}\n"` and sent in an
//! `Authorization: JOP appid=…,accesskey=…,nonce=…,timestamp=…,signature=…`
//! header (scheme as implemented by the community `JLCPCB-MCP-Server`).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use reqwest::Method;
use serde_json::{Value, json};
use sha2::Sha256;

use crate::error::{ApiError, Result};
use crate::http::{HttpClient, JsonRequest};

pub const SERVICE: &str = "JLCPCB official API";
pub const DEFAULT_BASE: &str = "https://open.jlcpcb.com";
const DETAIL_PATH: &str = "/overseas/openapi/component/getComponentDetailByCode";

/// Credentials for the official API.
#[derive(Clone)]
pub struct Credentials {
    pub app_id: String,
    pub access_key: String,
    pub secret: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("app_id", &self.app_id)
            .field("access_key", &"<redacted>")
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl Credentials {
    /// Read from the environment; `None` unless all three are non-empty.
    pub fn from_env() -> Option<Self> {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        Some(Self {
            app_id: get("JLCPCB_APP_ID")?,
            access_key: get("JLCPCB_API_KEY")?,
            secret: get("JLCPCB_API_SECRET")?,
        })
    }
}

/// Compute the base64 HMAC-SHA256 signature.
pub fn sign(
    secret: &str,
    method: &str,
    path: &str,
    timestamp: u64,
    nonce: &str,
    body: &str,
) -> String {
    let message = format!("{method}\n{path}\n{timestamp}\n{nonce}\n{body}\n");
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(message.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// Build the `Authorization` header value.
pub fn auth_header(
    creds: &Credentials,
    method: &str,
    path: &str,
    timestamp: u64,
    nonce: &str,
    body: &str,
) -> String {
    let signature = sign(&creds.secret, method, path, timestamp, nonce, body);
    format!(
        "JOP appid=\"{}\",accesskey=\"{}\",nonce=\"{}\",timestamp=\"{}\",signature=\"{}\"",
        creds.app_id, creds.access_key, nonce, timestamp, signature
    )
}

fn nonce() -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    (0..32)
        .map(|_| CHARS[rand::random_range(0..CHARS.len())] as char)
        .collect()
}

/// Official API client.
#[derive(Clone)]
pub struct OfficialClient {
    http: Arc<HttpClient>,
    base: String,
    creds: Credentials,
}

impl OfficialClient {
    pub fn new(http: Arc<HttpClient>, base: impl Into<String>, creds: Credentials) -> Self {
        Self {
            http,
            base: base.into().trim_end_matches('/').to_string(),
            creds,
        }
    }

    /// Fetch the official detail record for one code (raw JSON object).
    pub async fn component_detail(&self, code: &str) -> Result<Option<Value>> {
        let body = json!({ "componentCodes": [code] }).to_string();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let auth = auth_header(&self.creds, "POST", DETAIL_PATH, ts, &nonce(), &body);
        let url = format!("{}{}", self.base, DETAIL_PATH);
        let v = self
            .http
            .send_json(JsonRequest {
                service: SERVICE,
                method: Method::POST,
                url: &url,
                body: Some(body),
                headers: vec![("Authorization", auth)],
                ttl: Some(Duration::from_secs(300)),
            })
            .await?;
        parse_detail(v)
    }
}

/// Extract the first record from a `getComponentDetailByCode` response.
pub fn parse_detail(v: Value) -> Result<Option<Value>> {
    let code = v.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code != 200 {
        return Err(ApiError::Api {
            service: SERVICE,
            code,
            message: v
                .get("message")
                .or_else(|| v.get("msg"))
                .and_then(Value::as_str)
                .unwrap_or(
                    "unknown error (check JLCPCB_APP_ID / JLCPCB_API_KEY / JLCPCB_API_SECRET)",
                )
                .to_string(),
        });
    }
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    let list = data
        .get("componentDetailResponseVOList")
        .cloned()
        .unwrap_or(data);
    Ok(match list {
        Value::Array(items) => items.into_iter().next(),
        Value::Object(_) => Some(list),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_is_deterministic_and_matches_reference() {
        // Reference computed independently with Python's hmac/hashlib/base64.
        let sig = sign("secret", "POST", "/p", 1_700_000_000, "abc", "{}");
        assert_eq!(sig, "AdRA1u/RtwYrOSWh+llbjM/nv6R4pJgDdIFnEJtvFMU=");
    }

    #[test]
    fn header_shape() {
        let creds = Credentials {
            app_id: "app".into(),
            access_key: "key".into(),
            secret: "secret".into(),
        };
        let h = auth_header(&creds, "POST", "/p", 1_700_000_000, "abc", "{}");
        assert!(h.starts_with("JOP appid=\"app\",accesskey=\"key\",nonce=\"abc\",timestamp=\"1700000000\",signature=\""));
        assert!(!format!("{creds:?}").contains("secret\""));
    }

    #[test]
    fn nonce_shape() {
        let n = nonce();
        assert_eq!(n.len(), 32);
        assert!(n.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn parse_list_and_errors() {
        let ok =
            json!({"code":200,"data":{"componentDetailResponseVOList":[{"componentCode":"C1"}]}});
        assert_eq!(parse_detail(ok).unwrap().unwrap()["componentCode"], "C1");
        let err = json!({"code":401,"message":"bad sig"});
        assert!(matches!(
            parse_detail(err),
            Err(ApiError::Api { code: 401, .. })
        ));
    }
}
