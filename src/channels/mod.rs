//! Round 16 ─ 多通讯通道（飞书 / 钉钉 / 企微 / 通用 Webhook）。
//!
//! 设计：每个 channel 都实现 `Channel` trait（name / send / dry_run）。
//! - 飞书：用「群机器人 webhook」（不限企业自建应用，任意群都能加）
//! - 钉钉：用「群机器人 webhook」（加签 / 不加签都支持）
//! - 企微：用「群机器人 webhook」
//! - 通用 Webhook：POST JSON 到任意 URL
//!
//! 当前只做**单向发送**（最实用，零准入成本）。要双向（接收 IM 消息 → fr 主动回复）
//! 需要 WebSocket 长连接、签名校验，那是 Round 17+ 的事。

pub mod config;
pub mod dingtalk;
pub mod lark;
pub mod registry;
pub mod webhook;
pub mod wecom;

pub use config::{ChannelConfig, ChannelKind, ChannelsFile};
pub use registry::ChannelManager;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// 一条要发的消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    /// 纯文本 / markdown 原文
    pub text: String,
    /// 可选标题
    #[serde(default)]
    pub title: Option<String>,
    /// @ 谁（user_id open_id 之类，平台相关）
    #[serde(default)]
    pub at: Vec<String>,
    /// at 全体
    #[serde(default)]
    pub at_all: bool,
}

impl OutboundMessage {
    pub fn text(s: impl Into<String>) -> Self {
        Self { text: s.into(), title: None, at: Vec::new(), at_all: false }
    }
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = Some(t.into());
        self
    }
}

/// Channel 抽象。
#[async_trait::async_trait]
pub trait Channel: Send + Sync {
    /// channel 标识
    fn name(&self) -> &str;
    /// channel 类型
    fn kind(&self) -> ChannelKind;
    /// 真正发出去
    async fn send(&self, msg: &OutboundMessage) -> Result<SendReceipt>;
    /// dry-run：只构造请求体，不发
    async fn dry_run(&self, msg: &OutboundMessage) -> Result<serde_json::Value>;
}

/// 发送回执（成功 / 失败 + 平台返回的原始响应）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendReceipt {
    pub channel: String,
    pub ok: bool,
    pub platform_response: Option<serde_json::Value>,
    pub error: Option<String>,
}

impl SendReceipt {
    pub fn ok(channel: impl Into<String>, resp: Option<serde_json::Value>) -> Self {
        Self { channel: channel.into(), ok: true, platform_response: resp, error: None }
    }
    pub fn err(channel: impl Into<String>, err: impl Into<String>) -> Self {
        Self {
            channel: channel.into(),
            ok: false,
            platform_response: None,
            error: Some(err.into()),
        }
    }
}

/// 简单 HTTP POST 工具（被各 channel 复用）。
pub(crate) async fn http_post_json(
    url: &str,
    body: &serde_json::Value,
    headers: Vec<(&str, &str)>,
) -> Result<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| anyhow::anyhow!("build http: {e}"))?;
    let mut req = client.post(url).json(body);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("send: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow::anyhow!("HTTP {}: {}", status, text));
    }
    // 飞书/钉钉/企微 都返回 JSON，但兼容空 body
    if text.is_empty() {
        Ok(serde_json::json!({ "raw": "" }))
    } else {
        serde_json::from_str(&text).or_else(|_| Ok(serde_json::json!({ "raw": text })))
    }
}
