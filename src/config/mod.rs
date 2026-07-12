//! 配置模块。
//!
//! - [`paths`] —— `~/.fr_cli/` 路径常量与目录创建
//! - [`models`] —— `models.yaml` 解析 + 内置 fallback
//! - [`keys`] —— API key 解析（env 优先，keys.json 次之）
//! - [`settings`] —— `settings.toml`（autonomous 模式、limit、lang）

pub mod keys;
pub mod models;
pub mod paths;
pub mod settings;
