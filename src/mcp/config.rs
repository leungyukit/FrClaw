//! `~/.fr_cli/mcp_servers.json` 配置 schema。
//!
//! ```json
//! {
//!   "servers": [
//!     {
//!       "name": "filesystem",
//!       "url": "http://127.0.0.1:8123",
//!       "auto_connect": true,
//!       "enabled": true,
//!       "env": { "API_KEY": "..." },
//!       "headers": { "Authorization": "Bearer xxx" }
//!     }
//!   ]
//! }
//! ```

use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpServersFile {
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

/// 单个 server 配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// 内部 alias（如 `filesystem`）— 用于命名空间 / 命令路由
    pub name: String,
    /// Streamable HTTP 端点 base URL（POST `{url}/mcp`）
    pub url: String,
    #[serde(default = "default_true")]
    pub auto_connect: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 透传给 server 的环境变量（仅给本地 stdio server 用；HTTP server 多用于鉴权 header）
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// 额外的 HTTP header（如 `Authorization: Bearer xxx`）
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// 可选 timeout ms
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

fn default_true() -> bool {
    true
}

impl McpServerConfig {
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.timeout_ms.unwrap_or(30_000))
    }
}

pub fn servers_json_path() -> PathBuf {
    crate::config::paths::data_dir()
        .map(|d| d.join("mcp_servers.json"))
        .unwrap_or_else(|_| PathBuf::from("~/.fr_cli/mcp_servers.json"))
}

impl McpServersFile {
    pub fn load_or_default() -> Self {
        let path = servers_json_path();
        if !path.exists() {
            return Self::default();
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if text.trim().is_empty() {
            return Self::default();
        }
        match serde_json::from_str(&text) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("⚠️  mcp_servers.json 解析失败: {e} — 跳过");
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = servers_json_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, text)?;
        Ok(())
    }
}
