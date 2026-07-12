//! UI 子模块。
//!
//! - [`colors`] —— 基于 `owo-colors` 的彩色输出（含 `NO_COLOR` 支持）
//! - [`banner`] —— 启动 banner 与分隔线
//! - [`markdown`] —— 极简 markdown 渲染（heading/code/list）
//! - [`markdown_stream`] —— Round 10 增量流式 markdown 渲染（行级 flush）

pub mod banner;
pub mod colors;
pub mod markdown;
pub mod markdown_stream;
