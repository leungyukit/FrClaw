//! SOUL.md 持久身份。
//!
//! 仿 OpenClaw 的 SOUL.md 概念 —— 用一个或多个 markdown 文件定义 AI 的
//! persona / voice / tone / 价值观 / 偏好 / 准则。启动期读入，注入到
//! system prompt 尾段。
//!
//! 多源合并（按优先级叠加）：
//! 1. `~/.fr_cli/soul.md`（全局默认身份）
//! 2. cwd 下的 `SOUL.md`（项目级身份，会覆盖全局同名段）
//! 3. cwd 下的 `AGENTS.md`（兼容 OpenClaw / Claude Code 习惯）
//!
//! 不引 `pulldown-cmark` 之类——SOUL.md 是给 LLM 读的，markdown 仅作
//! 视觉分隔，原文注入即可。

pub mod loader;

pub use loader::{SoulContent, SoulSource};
