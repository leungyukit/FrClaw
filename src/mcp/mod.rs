//! MCP (Model Context Protocol) 子模块。
//!
//! - [`protocol`] —— JSON-RPC 2.0 envelope + initialize / tools/list / tools/call 数据结构
//! - [`client`]    —— Streamable HTTP 传输实现（POST `/mcp` + Mcp-Session-Id）
//! - [`config`]    —— `~/.fr_cli/mcp_servers.json` 配置 schema
//! - [`registry`]  —— 把 server 工具并入全局 tool registry（`mcp.<server>.<tool>`）
//!
//! 入口 [`McpManager::discover`]：
//! ```ignore
//! let mgr = McpManager::discover()?;
//! mgr.connect_all()?;        // 启动期 connect 每个 auto_connect server
//! for server in mgr.connected() {
//!     for tool in server.tools() {
//!         // 注入到 LLM tools 列表
//!     }
//! }
//! ```

pub mod client;
pub mod config;
pub mod protocol;
pub mod registry;

pub use client::McpClient;
pub use config::{McpServerConfig, McpServersFile};
pub use protocol::{Tool, ToolResult as McpToolResult};
pub use registry::{McpManager, McpServerHandle};
