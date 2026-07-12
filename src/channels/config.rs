//! `~/.fr_cli/channels.json` 配置。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Lark,      // 飞书
    Dingtalk,  // 钉钉
    Wecom,     // 企业微信
    Webhook,   // 通用
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub name: String,
    pub kind: ChannelKind,
    /// webhook URL（必填）
    pub webhook_url: String,
    /// 可选 secret（钉钉/飞书加签用）
    #[serde(default)]
    pub secret: Option<String>,
    /// 启用？默认 true
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 仅 dry-run（不真发）
    #[serde(default)]
    pub dry_run: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelsFile {
    #[serde(default)]
    pub channels: Vec<ChannelConfig>,
}

impl ChannelsFile {
    pub fn path() -> PathBuf {
        dirs::home_dir()
            .map(|h| h.join(".fr_cli").join("channels.json"))
            .unwrap_or_else(|| PathBuf::from("./channels.json"))
    }

    pub fn load_or_default() -> Self {
        let p = Self::path();
        if !p.exists() {
            return Self::default();
        }
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let p = Self::path();
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let s = serde_json::to_string_pretty(self)?;
        std::fs::write(&p, s)
            .map_err(|e| anyhow::anyhow!("写 channels.json 失败: {e}"))?;
        Ok(())
    }
}
