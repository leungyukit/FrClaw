//! ChannelManager ── 把多个 channel 装一起，提供 broadcast / 单发。

use super::config::{ChannelConfig, ChannelKind, ChannelsFile};
use super::dingtalk::DingTalkChannel;
use super::lark::LarkChannel;
use super::webhook::WebhookChannel;
use super::wecom::WeComChannel;
use super::{Channel, OutboundMessage, SendReceipt};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct ChannelManager {
    /// name → channel
    channels: HashMap<String, Arc<dyn Channel>>,
}

impl std::fmt::Debug for ChannelManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelManager")
            .field("names", &self.channels.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ChannelManager {
    pub fn from_file() -> Self {
        let cfg = ChannelsFile::load_or_default();
        Self::from_config(&cfg)
    }

    pub fn from_config(cfg: &ChannelsFile) -> Self {
        let mut channels = HashMap::new();
        for c in &cfg.channels {
            if !c.enabled { continue; }
            if let Some(ch) = build_channel(c) {
                channels.insert(c.name.clone(), ch);
            }
        }
        Self { channels }
    }

    pub fn len(&self) -> usize { self.channels.len() }

    pub fn list(&self) -> Vec<String> {
        let mut v: Vec<String> = self.channels.keys().cloned().collect();
        v.sort();
        v
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Channel>> {
        self.channels.get(name).cloned()
    }

    /// 单发
    pub async fn send(&self, name: &str, msg: &OutboundMessage) -> Result<SendReceipt> {
        let ch = self.get(name)
            .ok_or_else(|| anyhow::anyhow!("channel `{name}` 不存在"))?;
        ch.send(msg).await
    }

    /// 广播到所有 channel
    pub async fn broadcast(&self, msg: &OutboundMessage) -> Vec<SendReceipt> {
        let mut out = Vec::new();
        for (name, ch) in &self.channels {
            match ch.send(msg).await {
                Ok(r) => out.push(r),
                Err(e) => out.push(SendReceipt::err(name, format!("{e:#}"))),
            }
        }
        out
    }
}

fn build_channel(c: &ChannelConfig) -> Option<Arc<dyn Channel>> {
    match c.kind {
        ChannelKind::Lark => Some(Arc::new(LarkChannel {
            name: c.name.clone(),
            webhook_url: c.webhook_url.clone(),
            secret: c.secret.clone(),
            dry_run: c.dry_run,
        })),
        ChannelKind::Dingtalk => Some(Arc::new(DingTalkChannel {
            name: c.name.clone(),
            webhook_url: c.webhook_url.clone(),
            secret: c.secret.clone(),
            dry_run: c.dry_run,
        })),
        ChannelKind::Wecom => Some(Arc::new(WeComChannel {
            name: c.name.clone(),
            webhook_url: c.webhook_url.clone(),
            dry_run: c.dry_run,
        })),
        ChannelKind::Webhook => Some(Arc::new(WebhookChannel {
            name: c.name.clone(),
            url: c.webhook_url.clone(),
            dry_run: c.dry_run,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::config::{ChannelConfig, ChannelKind, ChannelsFile};

    #[test]
    fn from_config_creates_all_kinds() {
        let mut file = ChannelsFile::default();
        file.channels.push(ChannelConfig {
            name: "lark-dev".into(),
            kind: ChannelKind::Lark,
            webhook_url: "https://open.feishu.cn/hook/X".into(),
            secret: None,
            enabled: true,
            dry_run: true,
        });
        file.channels.push(ChannelConfig {
            name: "dd".into(),
            kind: ChannelKind::Dingtalk,
            webhook_url: "https://oapi.dingtalk.com/robot/send?access_token=x".into(),
            secret: None,
            enabled: true,
            dry_run: true,
        });
        file.channels.push(ChannelConfig {
            name: "wecom".into(),
            kind: ChannelKind::Wecom,
            webhook_url: "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x".into(),
            secret: None,
            enabled: true,
            dry_run: true,
        });
        file.channels.push(ChannelConfig {
            name: "generic".into(),
            kind: ChannelKind::Webhook,
            webhook_url: "https://example.com/hook".into(),
            secret: None,
            enabled: true,
            dry_run: true,
        });
        let mgr = ChannelManager::from_config(&file);
        assert_eq!(mgr.len(), 4);
        assert_eq!(mgr.list(), vec!["dd", "generic", "lark-dev", "wecom"]);
    }

    #[test]
    fn disabled_skipped() {
        let mut file = ChannelsFile::default();
        file.channels.push(ChannelConfig {
            name: "x".into(),
            kind: ChannelKind::Webhook,
            webhook_url: "https://x".into(),
            secret: None,
            enabled: false,
            dry_run: true,
        });
        let mgr = ChannelManager::from_config(&file);
        assert_eq!(mgr.len(), 0);
    }
}
