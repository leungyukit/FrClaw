//! REPL 模块。
//!
//! - [`runner`] —— 主循环（rustyline 输入 + 命令路由 / 普通对话分发）
//! - [`command`] —— 内置命令路由器
//! - [`commands`] —— 具体命令实现

pub mod command;
pub mod commands;
pub mod context;
pub mod runner;
