//! Request headers beyond the basics: stored site logins and headers imported
//! from a cURL command.
//!
//! Rules that keep secrets from leaking:
//! * stored logins are only sent over HTTPS (or to a loopback test server);
//! * everything here goes to the *original* host only. The engine does not
//!   attach it to mirror URLs, and reqwest drops `Authorization`/`Cookie` when
//!   a redirect leaves the host.

use std::collections::HashMap;

use reqwest::header::{HeaderName, HeaderValue};
use url::Url;

use crate::db::models::SiteCredential;

/// Headers the engine controls itself; a user-supplied value would break range
/// downloads, framing or compression handling.
const FORBIDDEN: &[&str] = &[
    "host",
    "content-length",
    "content-type",
    "range",
    "if-range",
    "if-match",
    "if-none-match",
    "if-modified-since",
    "if-unmodified-since",
    "accept-encoding",
    "connection",
    "keep-alive",
    "transfer-encoding",
    "te",
    "upgrade",
    "expect",
    "proxy-authorization",
    "proxy-connection",
    // Sent separately from the task's own fields:
    "user-agent",
    "referer",
];

pub fn is_forbidden_header(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    FORBIDDEN.contains(&lower.as_str()) || lower.starts_with(':')
}

fn is_loopback(host: Option<&str>) -> bool {
    matches!(host, Some("localhost") | Some("127.0.0.1") | Some("[::1]") | Some("::1"))
}

/// Headers that authenticate a request with a stored login.
///
/// * a token containing `=` is a cookie string → `Cookie`
/// * any other token → `Authorization: Bearer`
/// * otherwise a username + password → `Authorization: Basic`
pub fn headers_for_credential(url: &str, cred: &SiteCredential) -> Vec<(String, String)> {
    let Ok(parsed) = Url::parse(url) else { return Vec::new() };
    let secure = parsed.scheme() == "https" || is_loopback(parsed.host_str());
    if !secure {
        return Vec::new();
    }

    let mut out = Vec::new();
    let token = cred.token_or_cookie.as_deref().map(str::trim).filter(|t| !t.is_empty());
    if let Some(token) = token {
        if token.contains('=') {
            out.push(("Cookie".to_string(), token.to_string()));
        } else {
            out.push(("Authorization".to_string(), format!("Bearer {token}")));
        }
    }

    let has_authorization = out.iter().any(|(k, _)| k == "Authorization");
    if !has_authorization {
        if let (Some(user), Some(pass)) = (
            cred.username.as_deref().filter(|u| !u.is_empty()),
            cred.password.as_deref().filter(|p| !p.is_empty()),
        ) {
            out.push(("Authorization".to_string(), format!("Basic {}", base64(format!("{user}:{pass}").as_bytes()))));
        }
    }
    out
}

/// Validated, ready-to-send headers: the task's own headers first, then
/// `extra` (stored-login headers) for any name the task didn't already set.
pub fn build_header_list(
    task_headers: &HashMap<String, String>,
    extra: &[(String, String)],
) -> Vec<(HeaderName, HeaderValue)> {
    let mut out: Vec<(HeaderName, HeaderValue)> = Vec::new();
    let candidates = task_headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .chain(extra.iter().map(|(k, v)| (k.as_str(), v.as_str())));

    for (name, value) in candidates {
        if is_forbidden_header(name) {
            continue;
        }
        let (Ok(name), Ok(mut value)) = (HeaderName::from_bytes(name.trim().as_bytes()), HeaderValue::from_str(value.trim())) else {
            continue;
        };
        if out.iter().any(|(existing, _)| *existing == name) {
            continue;
        }
        value.set_sensitive(true);
        out.push((name, value));
    }
    out
}

/// Standard base64 (RFC 4648) with padding.
pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cred(user: Option<&str>, pass: Option<&str>, token: Option<&str>) -> SiteCredential {
        SiteCredential {
            id: "1".into(),
            domain: "example.com".into(),
            username: user.map(Into::into),
            password: pass.map(Into::into),
            token_or_cookie: token.map(Into::into),
            notes: None,
        }
    }

    #[test]
    fn base64_matches_the_rfc_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
    }

    #[test]
    fn credentials_are_never_sent_over_plain_http() {
        let c = cred(Some("u"), Some("p"), Some("tok"));
        assert!(headers_for_credential("http://files.example.com/a.zip", &c).is_empty());
        assert!(!headers_for_credential("https://files.example.com/a.zip", &c).is_empty());
        // Local test servers are fine.
        assert!(!headers_for_credential("http://127.0.0.1:8080/a.zip", &c).is_empty());
    }

    #[test]
    fn token_kinds_map_to_the_right_header() {
        let url = "https://example.com/f";
        assert_eq!(headers_for_credential(url, &cred(None, None, Some("sessionid=abc; csrf=1"))), vec![("Cookie".into(), "sessionid=abc; csrf=1".into())]);
        assert_eq!(headers_for_credential(url, &cred(None, None, Some("abc123"))), vec![("Authorization".into(), "Bearer abc123".into())]);
        assert_eq!(headers_for_credential(url, &cred(Some("user"), Some("pass"), None)), vec![("Authorization".into(), "Basic dXNlcjpwYXNz".into())]);
        // A username without a password isn't a usable login.
        assert!(headers_for_credential(url, &cred(Some("user"), None, None)).is_empty());
        // A bearer token wins over basic auth (one Authorization header only).
        let both = headers_for_credential(url, &cred(Some("u"), Some("p"), Some("tok")));
        assert_eq!(both.iter().filter(|(k, _)| k == "Authorization").count(), 1);
        assert!(both[0].1.starts_with("Bearer"));
    }

    #[test]
    fn engine_controlled_headers_are_dropped() {
        let mut h = HashMap::new();
        for k in ["Range", "Host", "Accept-Encoding", "User-Agent", "Content-Length", "If-Range", ":authority"] {
            h.insert(k.to_string(), "x".to_string());
        }
        h.insert("X-Token".to_string(), "t".to_string());
        h.insert("Cookie".to_string(), "a=b".to_string());
        let list = build_header_list(&h, &[]);
        let names: Vec<_> = list.iter().map(|(n, _)| n.as_str().to_string()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names.contains(&"x-token".to_string()) && names.contains(&"cookie".to_string()));
    }

    #[test]
    fn task_headers_take_precedence_over_stored_logins() {
        let mut h = HashMap::new();
        h.insert("authorization".to_string(), "Bearer mine".to_string());
        let list = build_header_list(&h, &[("Authorization".into(), "Bearer vault".into()), ("Cookie".into(), "v=1".into())]);
        let auth = list.iter().find(|(n, _)| n == "authorization").unwrap();
        assert_eq!(auth.1.to_str().unwrap(), "Bearer mine");
        assert!(list.iter().any(|(n, _)| n == "cookie"));
    }

    #[test]
    fn invalid_headers_are_skipped_not_fatal() {
        let mut h = HashMap::new();
        h.insert("Bad Name".to_string(), "v".to_string());
        h.insert("X-Ok".to_string(), "line1\nline2".to_string());
        h.insert("X-Fine".to_string(), "ok".to_string());
        let list = build_header_list(&h, &[]);
        assert_eq!(list.len(), 1);
    }
}
