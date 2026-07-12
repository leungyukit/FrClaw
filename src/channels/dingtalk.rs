//! 钉钉群机器人 webhook。
//!
//! 钉钉支持的消息类型：
//! - `text` ── 纯文本（支持 @手机号）
//! - `markdown` ── markdown（标题 + 文本）
//! - `link` / `actionCard` / `feedCard` ── 卡片
//!
//! 加签算法（钉钉比飞书简单）：
//!   string_to_sign = timestamp + "\n" + secret
//!   HMAC-SHA256(key=secret, msg=string_to_sign)  → base64 → url_encode

use super::{http_post_json, Channel, ChannelKind, OutboundMessage, SendReceipt};
use anyhow::Result;
use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};

pub struct DingTalkChannel {
    pub name: String,
    pub webhook_url: String,
    pub secret: Option<String>,
    pub dry_run: bool,
}

#[async_trait]
impl Channel for DingTalkChannel {
    fn name(&self) -> &str { &self.name }
    fn kind(&self) -> ChannelKind { ChannelKind::Dingtalk }

    async fn send(&self, msg: &OutboundMessage) -> Result<SendReceipt> {
        let (url, body) = self.build(msg).await?;
        if self.dry_run {
            return Ok(SendReceipt::ok(&self.name, Some(json!({ "url": url, "body": body }))));
        }
        match http_post_json(&url, &body, vec![]).await {
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
        let (url, body) = self.build(msg).await?;
        Ok(json!({ "url": url, "body": body }))
    }
}

impl DingTalkChannel {
    async fn build(&self, msg: &OutboundMessage) -> Result<(String, Value)> {
        let mut url = self.webhook_url.clone();
        let mut body = json!({});

        // 1) 加签 → URL 拼 &timestamp=&sign=
        if let Some(secret) = &self.secret {
            let ts = chrono::Utc::now().timestamp_millis();
            let string_to_sign = format!("{ts}\n{secret}");
            let key = hmac_sha256(secret.as_bytes(), string_to_sign.as_bytes())?;
            let sign = url_encode(&base64::engine::general_purpose::STANDARD.encode(&key));
            let sep = if url.contains('?') { '&' } else { '?' };
            url.push_str(&format!("{sep}timestamp={ts}&sign={sign}"));
        }

        // 2) 消息类型：有 title / 有 markdown 特征 → markdown；否则 text
        let has_md = msg.text.contains("**") || msg.text.contains("## ")
            || msg.text.contains('`') || msg.text.contains("\n```");
        if has_md {
            body["msgtype"] = json!("markdown");
            body["markdown"] = json!({
                "title": msg.title.clone().unwrap_or_else(|| "fr-claw".into()),
                "text": format!("## {}\n\n{}", msg.title.clone().unwrap_or_else(|| "fr-claw".into()), msg.text),
            });
        } else {
            body["msgtype"] = json!("text");
            body["text"] = json!({ "content": msg.text });
        }

        // 3) @列表（钉钉要手机号）
        if msg.at_all || !msg.at.is_empty() {
            body["at"] = json!({
                "atMobiles": msg.at,    // 钉钉 atMobiles 是手机号
                "isAtAll": msg.at_all,
            });
        }
        Ok((url, body))
    }
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Result<Vec<u8>> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| anyhow::anyhow!("hmac key: {e}"))?;
    mac.update(msg);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .flat_map(|b| if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
            vec![b as char]
        } else {
            format!("%{b:02X}").chars().collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dry_run_text() {
        let ch = DingTalkChannel { name: "d".into(), webhook_url: "https://oapi.dingtalk.com/robot/send?access_token=x".into(), secret: None, dry_run: true };
        let v = ch.dry_run(&OutboundMessage::text("hi")).await.unwrap();
        assert_eq!(v["body"]["msgtype"], "text");
    }

    #[tokio::test]
    async fn dry_run_with_sign_appends_query() {
        let ch = DingTalkChannel {
            name: "d".into(),
            webhook_url: "https://oapi.dingtalk.com/robot/send?access_token=x".into(),
            secret: Some("SEC".into()),
            dry_run: true,
        };
        let v = ch.dry_run(&OutboundMessage::text("hi")).await.unwrap();
        let url = v["url"].as_str().unwrap();
        assert!(url.contains("timestamp="));
        assert!(url.contains("sign="));
    }

    #[tokio::test]
    async fn dry_run_markdown() {
        let ch = DingTalkChannel { name: "d".into(), webhook_url: "https://x".into(), secret: None, dry_run: true };
        let v = ch.dry_run(&OutboundMessage {
            text: "**bold**\n```\ncode\n```".into(),
            title: Some("T".into()),
            at: vec![],
            at_all: false,
        }).await.unwrap();
        assert_eq!(v["body"]["msgtype"], "markdown");
    }

    #[test]
    fn url_encode_basic() {
        assert_eq!(url_encode("a b+c"), "a%20b%2Bc");
        assert_eq!(url_encode("_-."), "_-.");
    }
}
