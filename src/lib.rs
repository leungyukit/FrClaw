//! FrClaw —— 终端 AI 助手 / AI 编程伙伴（Rust 重写自 fr-cli，对标 OpenClaw 生态）
//!
//! 一个命令行 AI 助手，主打多模型、工具调用与会话持久化。
//!
//! 模块组织：
//! - `cli` —— 命令行参数解析与启动分发
//! - `config` —— `~/.fr_cli/` 配置加载与 `models.yaml` 解析
//! - `llm` —— LLM Provider trait 与 OpenAI 兼容实现
//! - `agent` —— MasterAgent ReAct 循环（think → tool → observe）
//! - `session` —— 会话消息持久化
//! - `memory` —— 项目记忆自动加载 + 上下文压缩 + 进化
//! - `hooks` —— 4 类事件钩子（PreToolUse/PostToolUse/UserPromptSubmit/SessionStart）
//! - `tools` —— 内置工具 + 授权层 + Plan mode + Sub-agent
//! - `repl` —— REPL 主循环与内置命令路由
//! - `ui` —— 颜色、banner、markdown 渲染
//! - `rag` —— RAG 个人知识库（sqlite + hashing embedding fallback）
//! - `sandbox` —— Round 9 沙箱隔离（路径/命令检查 + 可选 macOS sandbox-exec）
//! - `web` —— Round 11 Web 控制台（axum + Bearer + SSE）
//! - `hermes` —— Round 12 后台任务引擎（持久队列 + cron）
//! - `soul` —— Round 13 SOUL.md 持久身份（多源合并 + 注入 system）
//! - `heartbeat` —— Round 14 主动唤醒（按 SOUL 的 heartbeat 段定时跑）
//! - `error` —— 统一错误类型

pub mod agent;
pub mod channels;
pub mod cli;
pub mod config;
pub mod error;
pub mod export;
pub mod heartbeat;
pub mod tts;
pub mod hermes;
pub mod hooks;
pub mod llm;
pub mod mcp;
pub mod memory;
pub mod rag;
pub mod repl;
pub mod sandbox;
pub mod session;
pub mod skills;
pub mod soul;
pub mod tools;
pub mod ui;
pub mod voice;
pub mod web;

pub use error::{Error, Result};
