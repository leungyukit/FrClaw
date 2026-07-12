//! 企业微信群机器人 webhook。
//!
//! 支持消息类型：
//! - `text` ── 纯文本（支持 @userid）
//! - `markdown` ── markdown（仅支持有限子集）
//! - `template_card` ── 模板卡片
//!
//! 企微 markdown 限制：内容必须是 utf-8 字符串，**长度 ≤ 4096 字节**。

use super::{http_post_json, Channel, ChannelKind, OutboundMessage, SendReceipt};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct WeComChannel {
    pub name: String,
    pub webhook_url: String,
    pub dry_run: bool,
}

#[async_trait]
impl Channel for WeComChannel {
    fn name(&self) -> &str { &self.name }
    fn kind(&self) -> ChannelKind { ChannelKind::Wecom }

    async fn send(&self, msg: &OutboundMessage) -> Result<SendReceipt> {
        let body = self.build_body(msg);
        if self.dry_run {
            return Ok(SendReceipt::ok(&self.name, Some(body)));
        }
        match http_post_json(&self.webhook_url, &body, vec![]).await {
            Ok(v) => {
                let ok = v.get("errcode").and_then(|x| x.as_i64()).unwrap_or(-1) == 0;
                if ok {
                    Ok(SendReceipt::ok(&self.name, Some(v)))
                } else {
                    let err = v.get("errmsg").and_then(|x| x.as_str()).unwrap_or("unknown").to_string();
                    Ok(SendReceipt::err(&self.name, err))
                }
            }
            Err(e) => Ok(SendReceipt::err(&self.name, format!("{e:#}"))),
        }
    }

    async fn dry_run(&self, msg: &OutboundMessage) -> Result<Value> {
        Ok(self.build_body(msg))
    }
}

impl WeComChannel {
    fn build_body(&self, msg: &OutboundMessage) -> Value {
        // 企微 markdown 限长 4096 字节
        let content = if msg.text.len() > 4096 {
            let s: String = msg.text.chars().take(2000).collect();
            format!("{s}\n[截断，原文 > 4096 字节]")
        } else {
            msg.text.clone()
        };
        // markdown vs text 判定
        let has_md = content.contains("**") || content.contains("## ") || content.contains('`');
        if has_md {
            json!({
                "msgtype": "markdown",
                "markdown": { "content": content },
            })
        } else {
            json!({
                "msgtype": "text",
                "text": { "content": content, "mentioned_list": msg.at, "mentioned_mobile_list": [] },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dry_run_text() {
        let ch = WeComChannel { name: "w".into(), webhook_url: "https://x".into(), dry_run: true };
        let v = ch.dry_run(&OutboundMessage::text("hi")).await.unwrap();
        assert_eq!(v["msgtype"], "text");
        assert_eq!(v["text"]["content"], "hi");
    }

    #[tokio::test]
    async fn dry_run_markdown_truncates() {
        let ch = WeComChannel { name: "w".into(), webhook_url: "https://x".into(), dry_run: true };
        let long = "x".repeat(5000);
        let v = ch.dry_run(&OutboundMessage {
            text: format!("**bold** {long}"),
            title: None,
            at: vec![],
            at_all: false,
        }).await.unwrap();
        assert_eq!(v["msgtype"], "markdown");
        let content = v["markdown"]["content"].as_str().unwrap();
        assert!(content.len() <= 4096, "content too long: {}", content.len());
    }
}
