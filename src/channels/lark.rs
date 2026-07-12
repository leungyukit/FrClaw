//! 飞书群机器人 webhook。
//!
//! 飞书「群机器人」支持两种消息类型：
//! - `text` ── 纯文本
//! - `post` ── 富文本（标题 + 段落 + @列表）
//! - `interactive` ── 卡片（按钮、字段、表格）
//!
//! 加签方式（可选）：
//! - webhook URL 带 `/hook/<token>` → 不加签
//! - webhook URL 带 `?secret=XYZ` 或 config 提供 `secret` → 走加签逻辑
//!
//! 加签算法（HmacSHA256，timestamp + key）：
//!   string_to_sign = `timestamp\n` + secret
//!   HMAC-SHA256(key=string_to_sign, msg="")  → base64
//!   POST body: `{ timestamp, sign, msg_type, content }`

use super::{http_post_json, Channel, ChannelKind, OutboundMessage, SendReceipt};
use anyhow::Result;
use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};

pub struct LarkChannel {
    pub name: String,
    pub webhook_url: String,
    pub secret: Option<String>,
    pub dry_run: bool,
}

#[async_trait]
impl Channel for LarkChannel {
    fn name(&self) -> &str { &self.name }
    fn kind(&self) -> ChannelKind { ChannelKind::Lark }

    async fn send(&self, msg: &OutboundMessage) -> Result<SendReceipt> {
        let body = self.build_body(msg).await?;
        if self.dry_run {
            return Ok(SendReceipt::ok(&self.name, Some(body)));
        }
        match http_post_json(&self.webhook_url, &body, vec![]).await {
            Ok(v) => {
                // 飞书返回 {"StatusCode":0,"StatusMessage":"success","Extra":null,"Data":null}
                let ok = v.get("StatusCode").and_then(|x| x.as_i64()).unwrap_or(-1) == 0
                    || v.get("code").and_then(|x| x.as_i64()).unwrap_or(-1) == 0;
                if ok {
                    Ok(SendReceipt::ok(&self.name, Some(v)))
                } else {
                    let err = v.get("msg").and_then(|x| x.as_str()).unwrap_or("unknown").to_string();
                    Ok(SendReceipt::err(&self.name, err))
                }
            }
            Err(e) => Ok(SendReceipt::err(&self.name, format!("{e:#}"))),
        }
    }

    async fn dry_run(&self, msg: &OutboundMessage) -> Result<Value> {
        self.build_body(msg).await
    }
}

impl LarkChannel {
    async fn build_body(&self, msg: &OutboundMessage) -> Result<Value> {
        // 1) 如果有 secret → 加签
        let mut body = if let Some(secret) = &self.secret {
            let ts = chrono::Utc::now().timestamp();
            let string_to_sign = format!("{ts}\n{secret}");
            let key = hmac_sha256(string_to_sign.as_bytes(), b"")?;
            let sign = base64::engine::general_purpose::STANDARD.encode(key);
            json!({
                "timestamp": ts.to_string(),
                "sign": sign,
            })
        } else {
            json!({})
        };

        // 2) 消息类型选择
        let uses_post = msg.title.is_some() || msg.text.contains('\n') || msg.text.contains("**");
        if uses_post {
            body["msg_type"] = json!("post");
            let line = json!([
                { "tag": "text", "text": msg.text.clone() }
            ]);
            body["content"] = json!({
                "post": {
                    "zh_cn": {
                        "title": msg.title.clone().unwrap_or_else(|| "fr-claw".into()),
                        "content": [line],
                    }
                }
            });
        } else {
            body["msg_type"] = json!("text");
            body["content"] = json!({ "text": msg.text });
        }

        if msg.at_all || !msg.at.is_empty() {
            body["at"] = json!({
                "atUserIds": msg.at,
                "isAtAll": msg.at_all,
            });
        }
        Ok(body)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dry_run_text_body() {
        let ch = LarkChannel {
            name: "test".into(),
            webhook_url: "https://open.feishu.cn/hook/X".into(),
            secret: None,
            dry_run: true,
        };
        let body = ch.dry_run(&OutboundMessage::text("hello")).await.unwrap();
        assert_eq!(body["msg_type"], "text");
        assert_eq!(body["content"]["text"], "hello");
    }

    #[tokio::test]
    async fn dry_run_post_body() {
        let ch = LarkChannel {
            name: "test".into(),
            webhook_url: "https://open.feishu.cn/hook/X".into(),
            secret: None,
            dry_run: true,
        };
        let body = ch.dry_run(&OutboundMessage {
            text: "**bold** body".into(),
            title: Some("T".into()),
            at: vec![],
            at_all: false,
        }).await.unwrap();
        assert_eq!(body["msg_type"], "post");
        assert_eq!(body["content"]["post"]["zh_cn"]["title"], "T");
    }

    #[tokio::test]
    async fn dry_run_with_sign() {
        let ch = LarkChannel {
            name: "test".into(),
            webhook_url: "https://open.feishu.cn/hook/X".into(),
            secret: Some("mysecret".into()),
            dry_run: true,
        };
        let body = ch.dry_run(&OutboundMessage::text("hi")).await.unwrap();
        assert!(body["timestamp"].is_string());
        assert!(body["sign"].is_string());
        // sign 是 base64 编码
        let s = body["sign"].as_str().unwrap();
        assert!(base64::engine::general_purpose::STANDARD.decode(s).is_ok());
    }
}
