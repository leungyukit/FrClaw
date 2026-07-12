//! Hermes 后台任务引擎。
//!
//! 设计：
//! - 持久化任务队列（sqlite）
//! - 5-field cron 调度（自写解析器）
//! - 任务状态机：pending → approved → running → completed/failed/rejected
//! - 后台 worker：tokio task 每 30s 扫一次
//!
//! 不引 `tokio-cron-scheduler`（5-field cron 用 50 行 Rust 就能写完，少一层依赖）。

pub mod cron;
pub mod registry;
pub mod store;
pub mod task;
pub mod worker;

pub use cron::{CronExpr, CronField};
pub use registry::HermesRegistry;
pub use store::{TaskStore, TaskStoreHandle};
pub use task::{Task, TaskKind, TaskRun, TaskStatus};
