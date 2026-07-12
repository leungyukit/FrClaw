//! MCP 协议层（JSON-RPC 2.0 envelope + method schemas）。
//!
//! 遵循 MCP 协议 2025-06-18 spec：<https://modelcontextprotocol.io>
//!
//! 当前实现覆盖：
//! - `initialize` + `notifications/initialized`
//! - `tools/list` + `tools/call`
//! - `resources/list` + `resources/read` (Round 15a)
//! - `prompts/list` + `prompts/get` (Round 15a)
//!
//! 暂未实现：
//! - server-sent events 流响应
//! - `resources/subscribe` / `resources/updated`（资源订阅）
//! - `roots/list`（client 暴露给 server 的目录）

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 request envelope。
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: &'static str, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method,
            params,
        }
    }
}

/// JSON-RPC 2.0 response envelope。
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<u64>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

/// MCP 协议版本（spec 2025-06-18）。
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// client capabilities（最小集：roots / sampling / experimental）。
pub fn client_capabilities() -> Value {
    serde_json::json!({
        "roots": { "listChanged": false },
        "sampling": {},
        "experimental": {}
    })
}

/// client info。
pub fn client_info() -> Value {
    serde_json::json!({
        "name": "fr-claw",
        "version": env!("CARGO_PKG_VERSION")
    })
}

/// initialize request 的 params。
pub fn initialize_params() -> Value {
    serde_json::json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": client_capabilities(),
        "clientInfo": client_info()
    })
}

/// Server 在 initialize 响应里返回的 server info + capabilities。
#[derive(Debug, Clone, Deserialize)]
pub struct InitializeResult {
    #[serde(default)]
    pub protocol_version: Option<String>,
    #[serde(default)]
    pub server_info: Option<Value>,
    #[serde(default)]
    pub capabilities: Option<Value>,
    #[serde(default)]
    pub instructions: Option<String>,
}

/// 一条 tool 描述（server 端）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// JSON Schema object
    #[serde(default)]
    pub input_schema: Value,
}

impl Tool {
    /// 在 LLM tools 列表里显示的「qualified name」：`mcp.<server>.<tool>`
    pub fn qualified_name(&self, server: &str) -> String {
        format!("mcp__{}__{}", sanitize(server), sanitize(&self.name))
    }
}

/// tools/list 响应。
#[derive(Debug, Clone, Deserialize)]
pub struct ListToolsResult {
    pub tools: Vec<Tool>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// tools/call 参数。
pub fn tools_call_params(name: &str, arguments: Value) -> Value {
    serde_json::json!({
        "name": name,
        "arguments": arguments
    })
}

/// tools/call 响应（content 是 content block 数组；通常 1 个 text 元素）。
#[derive(Debug, Clone, Deserialize)]
pub struct CallToolResult {
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        data: String,
        #[serde(default)]
        mime_type: Option<String>,
    },
    Resource {
        resource: Value,
    },
    #[serde(other)]
    Other,
}

impl ContentBlock {
    /// 把 content 块转成单字符串（给 LLM 看）。
    pub fn to_text(&self) -> String {
        match self {
            ContentBlock::Text { text } => text.clone(),
            ContentBlock::Image { data, .. } => format!("[image: {} bytes]", data.len()),
            ContentBlock::Resource { resource } => format!("[resource: {}]", resource),
            ContentBlock::Other => "[unknown content]".into(),
        }
    }
}

// ───────────────────────────────────────────────────────────
// Round 15a: Resources API
// ───────────────────────────────────────────────────────────

/// 一条 resource 描述（server 端）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub uri: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
    /// 注解（audience / priority 等）
    #[serde(default)]
    pub annotations: Option<Value>,
}

/// resources/list 响应。
#[derive(Debug, Clone, Deserialize)]
pub struct ListResourcesResult {
    pub resources: Vec<Resource>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// resources/read 返回的内容容器（contents 数组）。
#[derive(Debug, Clone, Deserialize)]
pub struct ReadResourceResult {
    pub contents: Vec<ResourceContent>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceContent {
    Text {
        uri: String,
        text: String,
        #[serde(default, rename = "mimeType")]
        mime_type: Option<String>,
    },
    Blob {
        uri: String,
        blob: String, // base64
        #[serde(default, rename = "mimeType")]
        mime_type: Option<String>,
    },
    #[serde(other)]
    Other,
}

impl ResourceContent {
    pub fn uri(&self) -> Option<&str> {
        match self {
            ResourceContent::Text { uri, .. } => Some(uri),
            ResourceContent::Blob { uri, .. } => Some(uri),
            ResourceContent::Other => None,
        }
    }
    pub fn to_text(&self) -> String {
        match self {
            ResourceContent::Text { text, .. } => text.clone(),
            ResourceContent::Blob { blob, .. } => format!("[blob {} bytes]", blob.len()),
            ResourceContent::Other => "[unknown]".into(),
        }
    }
}

// ───────────────────────────────────────────────────────────
// Round 15a: Prompts API
// ───────────────────────────────────────────────────────────

/// 一条 prompt 描述（server 端）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// 参数定义（不是 JSON Schema，是 spec 自定义格式）
    #[serde(default)]
    pub arguments: Vec<PromptArgument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: Option<bool>,
}

/// prompts/list 响应。
#[derive(Debug, Clone, Deserialize)]
pub struct ListPromptsResult {
    pub prompts: Vec<Prompt>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// prompts/get 参数。
pub fn prompts_get_params(name: &str, args: Option<Value>) -> Value {
    let mut p = serde_json::json!({ "name": name });
    if let Some(a) = args {
        p["arguments"] = a;
    }
    p
}

/// prompts/get 响应。
#[derive(Debug, Clone, Deserialize)]
pub struct GetPromptResult {
    #[serde(default)]
    pub description: Option<String>,
    pub messages: Vec<PromptMessage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromptMessage {
    pub role: String, // "user" / "assistant"
    pub content: PromptMessageContent,
}

/// prompt 消息内容：通常是 text；也有 image / resource 等。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptMessageContent {
    Text { text: String },
    Image { data: String, #[serde(default, rename = "mimeType")] mime_type: Option<String> },
    Resource { resource: Value },
    #[serde(other)]
    Other,
}

impl PromptMessage {
    pub fn to_text(&self) -> String {
        match &self.content {
            PromptMessageContent::Text { text } => text.clone(),
            PromptMessageContent::Image { data, .. } => format!("[image {}B]", data.len()),
            PromptMessageContent::Resource { resource } => format!("[resource: {}]", resource),
            PromptMessageContent::Other => "[unknown]".into(),
        }
    }
}

/// tool 执行结果的高层封装（给 tool dispatcher）。
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }
    pub fn err(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

/// 把 server / tool name 里的「非标识符字符」替换为 `_`，让 LLM 调起来合法。
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_basic() {
        assert_eq!(sanitize("hello"), "hello");
        assert_eq!(sanitize("hello-world"), "hello_world");
        assert_eq!(sanitize("hello world"), "hello_world");
        assert_eq!(sanitize("中文"), "__");
    }

    #[test]
    fn tool_qualified_name() {
        let t = Tool {
            name: "list_files".into(),
            description: None,
            input_schema: serde_json::json!({}),
        };
        assert_eq!(t.qualified_name("filesystem"), "mcp__filesystem__list_files");
    }

    #[test]
    fn jsonrpc_envelope() {
        let r = JsonRpcRequest::new(1, "tools/list", None);
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"jsonrpc\":\"2.0\""));
        assert!(s.contains("\"method\":\"tools/list\""));
    }
}
/// MCP 协议版本 header 名（spec 2025-06-18 用 `MCP-Protocol-Version`）。
pub const MCP_PROTOCOL_VERSION_HEADER: &str = "MCP-Protocol-Version";