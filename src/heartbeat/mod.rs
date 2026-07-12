//! Heartbeat 主动唤醒。
//!
//! 设计：
//! - SOUL.md 的 `## heartbeat` 段定义 interval + directives
//! - 后台 tokio task 每 60s 查一次
//! - 到点 → spawn mini agent loop：system prompt 包含 SOUL + 「你是 Heartbeat」
//! - LLM 自由调工具 / 写一段 status 报告
//! - 状态写回 `~/.fr_cli/heartbeat_state.json`

pub mod policy;
pub mod registry;
pub mod runner;
pub mod state;

pub use policy::HeartbeatPolicy;
pub use registry::HeartbeatRegistry;
pub use state::{HeartbeatState, RunRecord};
