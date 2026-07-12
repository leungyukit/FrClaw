//! Streamable HTTP 传输实现。
//!
//! 协议：client POST JSON-RPC 2.0 request 到 `<server>/mcp` 端点；server 立即返回
//! 完整 JSON 响应（或 SSE 流；MVP 只实现 JSON 立即响应）。可选 `Mcp-Session-Id` header
//! 用来绑定一个 session；多请求间共享。

use crate::mcp::protocol::{
    CallToolResult, GetPromptResult, InitializeResult, JsonRpcRequest, JsonRpcResponse,
    ListPromptsResult, ListResourcesResult, ListToolsResult, Prompt, ReadResourceResult,
    Resource, Tool, ToolResult, MCP_PROTOCOL_VERSION,
};
use anyhow::{anyhow, Result};
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use reqwest::Client;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// 一个 MCP server 的连接 client。
pub struct McpClient {
    name: String,
    base_url: String,
    http: Client,
    next_id: AtomicU64,
    session_id: Mutex<Option<String>>,
    server_info: Mutex<Option<Value>>,
    capabilities: Mutex<Option<Value>>,
    /// server 在 initialize 之后告诉我们的 tool 列表（缓存）
    tools: Mutex<Vec<Tool>>,
    /// Round 15a ─ resources 列表（缓存）
    resources: Mutex<Vec<Resource>>,
    /// Round 15a ─ prompts 列表（缓存）
    prompts: Mutex<Vec<Prompt>>,
}

impl McpClient {
    pub fn new(name: impl Into<String>, base_url: impl Into<String>) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(format!("fr-claw/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| anyhow!("build http: {e}"))?;
        Ok(Self {
            name: name.into(),
            base_url: base_url.into(),
            http,
            next_id: AtomicU64::new(1),
            session_id: Mutex::new(None),
            server_info: Mutex::new(None),
            capabilities: Mutex::new(None),
            tools: Mutex::new(Vec::new()),
            resources: Mutex::new(Vec::new()),
            prompts: Mutex::new(Vec::new()),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn server_info(&self) -> Option<Value> {
        self.server_info.lock().unwrap().clone()
    }

    pub fn capabilities(&self) -> Option<Value> {
        self.capabilities.lock().unwrap().clone()
    }

    /// 取得缓存的 tool 列表（调 `list_tools()` 之后有效）。
    pub fn tools(&self) -> Vec<Tool> {
        self.tools.lock().unwrap().clone()
    }

    /// 一次性连接：initialize + notifications/initialized + list_tools 缓存。
    pub async fn connect(&self) -> Result<()> {
        // 1) initialize
        let init_resp = self
            .send_request("initialize", Some(crate::mcp::protocol::initialize_params()))
            .await?;
        let init: InitializeResult = serde_json::from_value(init_resp)?;
        *self.server_info.lock().unwrap() = init.server_info;
        *self.capabilities.lock().unwrap() = init.capabilities;

        // 2) notifications/initialized (notification 没有 response)
        let _ = self
            .send_request("notifications/initialized", Some(serde_json::json!({})))
            .await;

        // 3) tools/list
        self.refresh_tools().await?;
        // 4) resources/list + prompts/list（best-effort：server 可能没实现这俩 capability）
        let _ = self.refresh_resources().await;
        let _ = self.refresh_prompts().await;
        Ok(())
    }

    /// 重新拉一次 tools/list。
    pub async fn refresh_tools(&self) -> Result<()> {
        let resp = self
            .send_request("tools/list", Some(serde_json::json!({})))
            .await?;
        let parsed: ListToolsResult = serde_json::from_value(resp)?;
        *self.tools.lock().unwrap() = parsed.tools;
        Ok(())
    }

    /// 调用 server 上的一个 tool。
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<ToolResult> {
        let resp = self
            .send_request(
                "tools/call",
                Some(crate::mcp::protocol::tools_call_params(name, arguments)),
            )
            .await?;
        let parsed: CallToolResult = serde_json::from_value(resp)?;
        let content = parsed
            .content
            .iter()
            .map(|c| c.to_text())
            .collect::<Vec<_>>()
            .join("\n");
        Ok(ToolResult {
            content,
            is_error: parsed.is_error.unwrap_or(false),
        })
    }

    // ───────────────────────────────────────────────────────────
    // Round 15a: Resources API
    // ───────────────────────────────────────────────────────────

    /// 拉一次 resources/list 并缓存。
    pub async fn refresh_resources(&self) -> Result<()> {
        let resp = self
            .send_request("resources/list", Some(serde_json::json!({})))
            .await?;
        let parsed: ListResourcesResult = serde_json::from_value(resp)?;
        *self.resources.lock().unwrap() = parsed.resources;
        Ok(())
    }

    /// 当前缓存的 resources。
    pub fn resources(&self) -> Vec<Resource> {
        self.resources.lock().unwrap().clone()
    }

    /// 读一个 resource 的内容。
    pub async fn read_resource(&self, uri: &str) -> Result<ReadResourceResult> {
        let resp = self
            .send_request("resources/read", Some(serde_json::json!({ "uri": uri })))
            .await?;
        let parsed: ReadResourceResult = serde_json::from_value(resp)?;
        Ok(parsed)
    }

    // ───────────────────────────────────────────────────────────
    // Round 15a: Prompts API
    // ───────────────────────────────────────────────────────────

    /// 拉一次 prompts/list 并缓存。
    pub async fn refresh_prompts(&self) -> Result<()> {
        let resp = self
            .send_request("prompts/list", Some(serde_json::json!({})))
            .await?;
        let parsed: ListPromptsResult = serde_json::from_value(resp)?;
        *self.prompts.lock().unwrap() = parsed.prompts;
        Ok(())
    }

    /// 当前缓存的 prompts。
    pub fn prompts(&self) -> Vec<Prompt> {
        self.prompts.lock().unwrap().clone()
    }

    /// 取一个 prompt 模板。
    pub async fn get_prompt(&self, name: &str, args: Option<Value>) -> Result<GetPromptResult> {
        let resp = self
            .send_request(
                "prompts/get",
                Some(crate::mcp::protocol::prompts_get_params(name, args)),
            )
            .await?;
        let parsed: GetPromptResult = serde_json::from_value(resp)?;
        Ok(parsed)
    }

    /// 真正发一个 JSON-RPC request。
    async fn send_request(&self, method: &'static str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = JsonRpcRequest::new(id, method, params);
        let url = format!("{}/mcp", self.base_url.trim_end_matches('/'));

        let mut req = self
            .http
            .post(&url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .header(super::protocol::MCP_PROTOCOL_VERSION_HEADER, MCP_PROTOCOL_VERSION)
            .json(&body);

        if let Some(sid) = self.session_id.lock().unwrap().clone() {
            req = req.header("Mcp-Session-Id", sid);
        }

        let resp = req.send().await.map_err(|e| anyhow!("send: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!("MCP HTTP {}: {}", status, txt));
        }

        // 提取 session id header（如果 server 给了）
        if let Some(sid) = resp
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
        {
            *self.session_id.lock().unwrap() = Some(sid);
        }

        let resp: JsonRpcResponse = resp.json().await.map_err(|e| anyhow!("decode: {e}"))?;
        if let Some(err) = resp.error {
            return Err(anyhow!(
                "MCP error {}: {} (data: {:?})",
                err.code,
                err.message,
                err.data
            ));
        }
        Ok(resp.result.unwrap_or(Value::Null))
    }
}
