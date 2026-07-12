//! API key 解析。
//!
//! - env 变量（`API_KEY_ENV` 字段值）
//! - `~/.fr_cli/keys.json` 的覆盖值
//!
//! 这是个轻量级实现——避免引入 secret manager，
//! 但 `keys.json` 已是 `0600` 权限文件，普通用户足够。

use crate::config::paths;
use crate::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 解析 provider 的 API key：
/// - 优先环境变量
/// - 其次 `keys.json` 内显式保存的值
pub fn resolve(provider_key_env: Option<&str>, alias: &str) -> Option<String> {
    if let Some(env) = provider_key_env {
        if let Ok(v) = std::env::var(env) {
            if !v.trim().is_empty() {
                return Some(v);
            }
        }
    }
    from_keys_json(alias)
}

fn from_keys_json(alias: &str) -> Option<String> {
    let path = match paths::keys_json_path() {
        Ok(p) => p,
        Err(_) => return None,
    };
    if !path.exists() {
        return None;
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return None,
    };
    let map: BTreeMap<String, Value> = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(_) => return None,
    };
    map.get(alias)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

/// 把 key 写入 `keys.json`（0600 权限，unix 上）。
pub fn set_key(alias: &str, key: &str) -> Result<()> {
    let path = paths::keys_json_path()?;
    let mut map: BTreeMap<String, Value> = if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        serde_json::from_str(&text).unwrap_or_default()
    } else {
        BTreeMap::new()
    };
    map.insert(alias.to_string(), json!(key));
    let text = serde_json::to_string_pretty(&map)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perm = std::fs::Permissions::from_mode(0o600);
        let _ = std::fs::set_permissions(&path, perm);
    }
    Ok(())
}
