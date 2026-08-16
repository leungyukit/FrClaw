//! Plan Engine ── Autonomous Goal → DAG → Concurrent Execution + Self-Heal
//!
//! 三个能力合在一起：
//! 1. **目标→任务自动拆解**（decompose.rs）：LLM 把用户给的高层 goal 拆成 DAG
//! 2. **多 plan 并发调度**（scheduler.rs + dag.rs）：DAG 节点按 dep 解锁并发跑
//! 3. **任务级自愈**（self_heal.rs + executor.rs）：失败 N 次 → 换工具 → 升级 plan
//!
//! 存储：复用 hermes 的 sqlite（`tasks` + `task_runs`），新增 `plans` / `plan_edges` 两表

pub mod dag;
pub mod decompose;
pub mod executor;
pub mod goal;
pub mod scheduler;
pub mod self_heal;
pub mod store;

pub use dag::{PlanDag, PlanEdge, PlanNode, PlanNodeStatus, PlanStatus};
pub use goal::Goal;
pub use scheduler::PlanEngine;
pub use store::PlanStore;
