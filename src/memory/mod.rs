//! 长期记忆层：
//!
//! - [`project`] —— 项目级 `.frcli.md` / `AGENTS.md` / `CLAUDE.md` / `.github/AGENTS.md`
//! - [`compressor`] —— 超过阈值时自动摘要老的会话轮次
//! - [`evolution`] —— **自我记忆进化引擎**（web 搜索版 + 用户对话版 + 召回索引）

pub mod compressor;
pub mod evolution;
pub mod project;
