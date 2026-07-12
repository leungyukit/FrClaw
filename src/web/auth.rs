//! Bearer Token 鉴权。
//!
//! Token 生成：32 字节随机 → hex 64 字符。
//! 持久化：`~/.fr_cli/web_token`。
//! 验证：HTTP header `Authorization: Bearer <token>`。

use anyhow::Result;
use rand::Rng;
use std::path::PathBuf;

pub fn token_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".fr_cli").join("web_token"))
        .unwrap_or_else(|| PathBuf::from("./web_token"))
}

pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    hex_encode(&bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// 加载持久化的 token；不存在则生成并保存。
pub fn load_or_create_token() -> Result<String> {
    let path = token_path();
    if path.exists() {
        let s = std::fs::read_to_string(&path)?;
        let s = s.trim().to_string();
        if !s.is_empty() {
            return Ok(s);
        }
    }
    let tok = generate_token();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&path, &tok)?;
    // 0600 权限
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(tok)
}

/// 校验请求里的 Bearer token 是否匹配。
pub fn check_bearer(headers: &axum::http::HeaderMap, expected: &str) -> bool {
    if let Some(h) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(s) = h.to_str() {
            if let Some(t) = s.strip_prefix("Bearer ") {
                return t.trim() == expected;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header::AUTHORIZATION, HeaderMap, HeaderValue};

    #[test]
    fn generate_token_is_64_hex() {
        let t = generate_token();
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn check_bearer_matches() {
        let t = "abc123";
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {t}")).unwrap());
        assert!(check_bearer(&h, t));
    }

    #[test]
    fn check_bearer_rejects_wrong() {
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, HeaderValue::from_static("Bearer wrong"));
        assert!(!check_bearer(&h, "right"));
    }

    #[test]
    fn check_bearer_rejects_missing() {
        let h = HeaderMap::new();
        assert!(!check_bearer(&h, "anything"));
    }

    #[test]
    fn check_bearer_rejects_wrong_scheme() {
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, HeaderValue::from_static("Basic dXNlcjpwYXNz"));
        assert!(!check_bearer(&h, "anything"));
    }
}
