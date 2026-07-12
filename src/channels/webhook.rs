//! 通用 Webhook ── POST JSON 到任意 URL。
//!
//! Body schema（最简也最通用）：
//! ```json
//! { "title": "...", "text": "...", "at": [...], "at_all": false, "source": "fr-claw" }
//! ```
//!
//! 接收方（Slack / Discord / n8n / Bark / 自建）按需取字段。

use super::{http_post_json, Channel, ChannelKind, OutboundMessage, SendReceipt};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct WebhookChannel {
    pub name: String,
    pub url: String,
    pub dry_run: bool,
}

impl WebhookChannel {
    fn build_body(&self, msg: &OutboundMessage) -> Value {
        json!({
            "source": "fr-claw",
            "title": msg.title,
            "text": msg.text,
            "at": msg.at,
            "at_all": msg.at_all,
        })
    }
}

#[async_trait]
impl Channel for WebhookChannel {
    fn name(&self) -> &str { &self.name }
    fn kind(&self) -> ChannelKind { ChannelKind::Webhook }

    async fn send(&self, msg: &OutboundMessage) -> Result<SendReceipt> {
        let body = self.build_body(msg);
        if self.dry_run {
            return Ok(SendReceipt::ok(&self.name, Some(body)));
        }
        match http_post_json(&self.url, &body, vec![]).await {
            Ok(v) => Ok(SendReceipt::ok(&self.name, Some(v))),
            Err(e) => Ok(SendReceipt::err(&self.name, format!("{e:#}"))),
        }
    }

    async fn dry_run(&self, msg: &OutboundMessage) -> Result<Value> {
        Ok(self.build_body(msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dry_run_body_shape() {
        let ch = WebhookChannel { name: "n8n".into(), url: "https://x".into(), dry_run: true };
        let body = ch.dry_run(&OutboundMessage::text("hi").title("T")).await.unwrap();
        assert_eq!(body["source"], "fr-claw");
        assert_eq!(body["title"], "T");
        assert_eq!(body["text"], "hi");
    }
}
