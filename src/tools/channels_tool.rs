//! Round 16 ─ Channels 工具（LLM 可见 2 个）。
//!
//! - `channel_send` ── 单发到指定 channel
//! - `channel_broadcast` ── 广播到所有 channel
//!
//! 实际执行走 `tools/registry.rs` 的 dispatch（用 AppContext 的 ChannelManager）。

use crate::channels::OutboundMessage;
use crate::llm::message::ToolDefinition;
use serde_json::{json, Value};

pub fn channel_send_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "channel_send",
        "发一条消息到指定的 channel（飞书/钉钉/企微/通用 webhook）。",
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "channel 名（/channels list 看）" },
                "text": { "type": "string", "description": "要发的文本（支持 markdown）" },
                "title": { "type": "string", "description": "可选标题" },
                "at": { "type": "array", "items": { "type": "string" }, "description": "@ 谁（user_id / 手机号）" },
                "at_all": { "type": "boolean", "description": "@全体" }
            },
            "required": ["name", "text"]
        }),
    )
}

pub fn channel_broadcast_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "channel_broadcast",
        "把一条消息广播到所有已配的 channel。",
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "title": { "type": "string" }
            },
            "required": ["text"]
        }),
    )
}

pub fn build_outbound(args: &Value) -> OutboundMessage {
    OutboundMessage {
        text: args.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        title: args.get("title").and_then(|v| v.as_str()).map(|s| s.to_string()),
        at: args.get("at")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        at_all: args.get("at_all").and_then(|v| v.as_bool()).unwrap_or(false),
    }
}
