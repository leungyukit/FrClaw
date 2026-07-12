//! Hooks 子模块：
//!
//! - [`events`] —— 输入 / 输出数据模型（JSON 给 hook 命令）
//! - [`config`] —— `~/.fr_cli/hooks.json` 配置 + matcher 匹配
//! - [`runner`] —— 执行 shell 命令 hook，处理 exit code 2 阻止逻辑
//!
//! 全局入口 [`dispatch_event`]：
//! ```ignore
//! use crate::hooks::dispatch_event;
//! let out = dispatch_event(HookEvent::PreToolUse, &HookInput::tool("shell", args)).await?;
//! if out.blocked { /* 不要调工具 */ }
//! if let Some(new_args) = out.modified_args { /* 用 new_args 替换 */ }
//! ```

pub mod config;
pub mod events;
pub mod runner;

pub use config::{HookEntry, HookHandler, HooksFile};
pub use events::{HookEvent, HookInput, HookOutput};
pub use runner::{dispatch_event, dispatch_event_blocking, run_hook};
