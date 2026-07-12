//! MCP Manager —— 加载配置 + 创建 client + 自动 connect + 提供工具查询接口。
//!
//! 暴露给 [`crate::tools::registry`] 的 API：
//! - [`McpManager::all_tools`] —— 所有已连上 server 的 tool（含 qualified name）
//! - [`McpManager::dispatch`] —— `mcp__<server>__<tool>` 形式的 dispatch

use crate::mcp::client::McpClient;
use crate::mcp::config::{McpServerConfig, McpServersFile};
use crate::mcp::protocol::{GetPromptResult, Prompt, ReadResourceResult, Resource, Tool};
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct McpServerHandle {
    pub name: String,
    pub url: String,
    pub status: ConnectionStatus,
    pub tools: Vec<Tool>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    Pending,
    Connected,
    Failed,
    Disabled,
}

#[derive(Clone)]
pub struct McpManager {
    inner: Arc<RwLock<ManagerState>>,
}

impl std::fmt::Debug for McpManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpManager").finish()
    }
}

/// 用 `parking_lot` 那种同步锁不好，async 友好用 `tokio::sync::RwLock`。
struct ManagerState {
    /// name → client
    clients: HashMap<String, Arc<McpClient>>,
    /// 启动期各 server 的连接状态（用于 `/mcp list`）
    statuses: HashMap<String, McpServerHandle>,
    /// 配置快照
    configs: HashMap<String, McpServerConfig>,
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ManagerState {
                clients: HashMap::new(),
                statuses: HashMap::new(),
                configs: HashMap::new(),
            })),
        }
    }

    /// 从 `mcp_servers.json` 加载并初始化；不自动 connect（让 caller 决定）。
    pub fn from_config() -> Self {
        let cfg = McpServersFile::load_or_default();
        let mut clients = HashMap::new();
        let mut statuses = HashMap::new();
        let mut configs = HashMap::new();
        for s in cfg.servers {
            let name = s.name.clone();
            if !s.enabled {
                statuses.insert(
                    name.clone(),
                    McpServerHandle {
                        name: name.clone(),
                        url: s.url.clone(),
                        status: ConnectionStatus::Disabled,
                        tools: vec![],
                        last_error: None,
                    },
                );
                continue;
            }
            match McpClient::new(name.clone(), s.url.clone()) {
                Ok(c) => {
                    clients.insert(name.clone(), Arc::new(c));
                    statuses.insert(
                        name.clone(),
                        McpServerHandle {
                            name: name.clone(),
                            url: s.url.clone(),
                            status: ConnectionStatus::Pending,
                            tools: vec![],
                            last_error: None,
                        },
                    );
                    configs.insert(name, s);
                }
                Err(e) => {
                    statuses.insert(
                        name.clone(),
                        McpServerHandle {
                            name,
                            url: s.url.clone(),
                            status: ConnectionStatus::Failed,
                            tools: vec![],
                            last_error: Some(e.to_string()),
                        },
                    );
                    configs.insert(s.name.clone(), s);
                }
            }
        }
        Self {
            inner: Arc::new(RwLock::new(ManagerState {
                clients,
                statuses,
                configs,
            })),
        }
    }

    /// auto_connect 跑一次；不阻塞（失败不致命，置为 Failed）。
    pub async fn connect_all(&self) {
        let to_connect: Vec<(String, Arc<McpClient>)> = {
            let st = self.inner.read().await;
            st.clients
                .iter()
                .filter(|(name, _)| {
                    st.configs
                        .get(*name)
                        .map(|c| c.auto_connect && c.enabled)
                        .unwrap_or(false)
                })
                .map(|(k, v)| (k.clone(), Arc::clone(v)))
                .collect()
        };
        for (name, client) in to_connect {
            let status = match client.connect().await {
                Ok(()) => ConnectionStatus::Connected,
                Err(e) => {
                    eprintln!("  ⚠️  MCP server `{name}` connect 失败: {e:#}");
                    ConnectionStatus::Failed
                }
            };
            let tools = client.tools();
            let last_error = if status == ConnectionStatus::Failed {
                // 拿错误信息
                Some(format!("connect 失败"))
            } else {
                None
            };
            let mut st = self.inner.write().await;
            if let Some(h) = st.statuses.get_mut(&name) {
                h.status = status.clone();
                h.tools = tools.clone();
                h.last_error = last_error;
            }
            // Failed 的 server 不保留 client（下次重连会重建）
            if matches!(status, ConnectionStatus::Failed) {
                st.clients.remove(&name);
            }
        }
    }

    /// 列出所有 server 的状态。
    pub async fn list_servers(&self) -> Vec<McpServerHandle> {
        let st = self.inner.read().await;
        let mut v: Vec<McpServerHandle> = st.statuses.values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// 重新 connect 某个 server（重置 client 重建）。
    pub async fn reconnect(&self, name: &str) -> Result<()> {
        let cfg = {
            let st = self.inner.read().await;
            st.configs.get(name).cloned()
        };
        let cfg = cfg.ok_or_else(|| anyhow::anyhow!("server `{name}` 未配置"))?;
        let client = Arc::new(McpClient::new(name.to_string(), cfg.url.clone())?);
        let status = match client.connect().await {
            Ok(()) => ConnectionStatus::Connected,
            Err(e) => {
                eprintln!("  ⚠️  reconnect `{name}` 失败: {e:#}");
                ConnectionStatus::Failed
            }
        };
        let tools = client.tools();
        let mut st = self.inner.write().await;
        if matches!(status, ConnectionStatus::Connected) {
            st.clients.insert(name.to_string(), client);
        } else {
            st.clients.remove(name);
        }
        if let Some(h) = st.statuses.get_mut(name) {
            h.status = status;
            h.tools = tools;
        }
        Ok(())
    }

    /// 全部 tool（含 qualified name `mcp__<server>__<tool>`）。
    pub async fn all_tools(&self) -> Vec<(String /*qualified*/, Tool, String /*server*/)> {
        let st = self.inner.read().await;
        let mut out = Vec::new();
        for (server, client) in &st.clients {
            for t in client.tools() {
                out.push((t.qualified_name(server), t, server.clone()));
            }
        }
        out
    }

    /// 调 server tool。
    pub async fn dispatch(
        &self,
        qualified_name: &str,
        arguments: Value,
    ) -> Result<Value> {
        // 解析 "mcp__<server>__<tool>"
        let rest = qualified_name
            .strip_prefix("mcp__")
            .ok_or_else(|| anyhow::anyhow!("not a mcp tool: {qualified_name}"))?;
        let (server, tool) = rest
            .split_once("__")
            .ok_or_else(|| anyhow::anyhow!("bad mcp tool name: {qualified_name}"))?;
        let client = {
            let st = self.inner.read().await;
            st.clients.get(server).cloned()
        };
        let client = client.ok_or_else(|| anyhow::anyhow!("mcp server `{server}` 未连接"))?;
        let res = client.call_tool(tool, arguments).await?;
        if res.is_error {
            // 错误也以 value 形式返回（不阻断）—— fr-cli 的 tools 协议
            Ok(serde_json::json!({
                "error": true,
                "content": res.content,
            }))
        } else {
            Ok(serde_json::json!({
                "content": res.content,
            }))
        }
    }

    /// 工具定义列表（注入到 LLM 端）。
    pub async fn tool_definitions(&self) -> Vec<crate::llm::message::ToolDefinition> {
        let mut out = Vec::new();
        for (qname, tool, _server) in self.all_tools().await {
            let mut def = crate::llm::message::ToolDefinition::from_json_schema(
                &qname,
                format!("[mcp] {}", tool.description.clone().unwrap_or_default()).as_str(),
                tool.input_schema.clone(),
            );
            // 默认改 type 不动（已经是 function）。
            let _ = &mut def;
            out.push(def);
        }
        out
    }

    // ───────────────────────────────────────────────────────────
    // Round 15a: Resources / Prompts 跨 server 查询
    // ───────────────────────────────────────────────────────────

    /// 列出所有 server 暴露的 resources（带 server 名前缀）。
    pub async fn all_resources(&self) -> Vec<(String /*server*/, Resource)> {
        let st = self.inner.read().await;
        let mut out = Vec::new();
        for (server, client) in &st.clients {
            for r in client.resources() {
                out.push((server.clone(), r));
            }
        }
        out
    }

    /// 列出所有 server 暴露的 prompts。
    pub async fn all_prompts(&self) -> Vec<(String /*server*/, Prompt)> {
        let st = self.inner.read().await;
        let mut out = Vec::new();
        for (server, client) in &st.clients {
            for p in client.prompts() {
                out.push((server.clone(), p));
            }
        }
        out
    }

    /// 读 resource：`<server>::<uri>` 或纯 uri（自动选第一个 server）。
    pub async fn read_resource(
        &self,
        server: &str,
        uri: &str,
    ) -> Result<ReadResourceResult> {
        let client = {
            let st = self.inner.read().await;
            st.clients.get(server).cloned()
        };
        let client =
            client.ok_or_else(|| anyhow::anyhow!("mcp server `{server}` 未连接"))?;
        client.read_resource(uri).await
    }

    /// 调 prompt 模板。
    pub async fn get_prompt(
        &self,
        server: &str,
        name: &str,
        args: Option<Value>,
    ) -> Result<GetPromptResult> {
        let client = {
            let st = self.inner.read().await;
            st.clients.get(server).cloned()
        };
        let client =
            client.ok_or_else(|| anyhow::anyhow!("mcp server `{server}` 未连接"))?;
        client.get_prompt(name, args).await
    }

    /// 重新拉某个 server 的 resources（用户主动触发）。
    pub async fn refresh_resources(&self, name: &str) -> Result<()> {
        let client = {
            let st = self.inner.read().await;
            st.clients.get(name).cloned()
        };
        let client =
            client.ok_or_else(|| anyhow::anyhow!("mcp server `{name}` 未连接"))?;
        client.refresh_resources().await
    }

    /// 重新拉某个 server 的 prompts。
    pub async fn refresh_prompts(&self, name: &str) -> Result<()> {
        let client = {
            let st = self.inner.read().await;
            st.clients.get(name).cloned()
        };
        let client =
            client.ok_or_else(|| anyhow::anyhow!("mcp server `{name}` 未连接"))?;
        client.refresh_prompts().await
    }
}
