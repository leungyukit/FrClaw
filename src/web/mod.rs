//! Web 控制台入口。
//!
//! 启动一个 axum HTTP server，暴露：
//! - `GET  /`                    → 单页 chat UI（HTML）
//! - `GET  /api/status`          → 状态 JSON
//! - `GET  /api/skills`          → skills 列表
//! - `GET  /api/sandbox`         → sandbox 策略
//! - `GET  /api/mcp`             → MCP servers
//! - `GET  /api/rag`             → RAG stats
//! - `POST /api/chat`            → SSE 流式 chat
//! - `POST /api/command`         → 执行一个 REPL 命令（返回 JSON）
//!
//! 鉴权：Bearer Token（启动时随机生成，写入 `~/.fr_cli/web_token`，打印到 stdout）。
//! 关闭：`--no-auth`。

pub mod api;
pub mod auth;
pub mod chat;
pub mod server;
pub mod static_files;

pub use auth::{generate_token, load_or_create_token, token_path};
pub use server::{WebConfig, run_web_server};
