//! 会话模块。
//!
//! - [`chat`] —— ChatSession：内存里的 messages 列表 + system prompt
//! - [`store`] —— 把会话保存到 `~/.fr_cli/sessions/<name>.json`

pub mod chat;
pub mod store;
