//! HermesRegistry：全局注册表 + 后台 tick 协程。
//!
//! 启动时调 `HermesRegistry::start(ctx)` 在后台跑每 30s 一次的 tick。
//! 提供 add / list / approve / reject / run_now / delete 等操作。

use super::store::TaskStoreHandle;
use super::task::Task;
use super::worker::Worker;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

#[derive(Clone)]
pub struct HermesRegistry {
    pub store: TaskStoreHandle,
    pub worker: Arc<Worker>,
}

impl HermesRegistry {
    pub fn new(store: TaskStoreHandle) -> Self {
        let worker = Arc::new(Worker::new(store.clone()));
        Self { store, worker }
    }

    /// 启动后台 tick 任务（每 30s 一次）。
    /// 返回 tokio::JoinHandle 方便测试 shutdown。
    pub fn start_background(&self) -> tokio::task::JoinHandle<()> {
        let worker = self.worker.clone();
        tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(30));
            // 第一次立即跑
            tick.tick().await;
            loop {
                tick.tick().await;
                if let Err(e) = worker.tick().await {
                    eprintln!("  [hermes] tick error: {e:#}");
                }
            }
        })
    }

    /// 一次性 tick（同步触发；测试用）。
    pub async fn tick_now(&self) -> Result<usize> {
        self.worker.tick().await
    }

    pub fn add_task(
        &self,
        name: &str,
        kind: crate::hermes::task::TaskKind,
        args: &str,
        cron_expr: Option<&str>,
        approval_mode: &str,
    ) -> Result<i64> {
        self.store.add(name, kind, args, cron_expr, approval_mode)
    }

    pub fn list(&self) -> Result<Vec<Task>> {
        self.store.list()
    }

    pub fn get(&self, id: i64) -> Result<Option<Task>> {
        self.store.get(id)
    }

    pub fn approve(&self, id: i64, by: &str) -> Result<()> {
        self.store.approve(id, by)
    }

    pub fn reject(&self, id: i64) -> Result<()> {
        self.store.reject(id)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.store.delete(id)
    }

    /// 手动触发一次（无论 cron 状态）。
    pub async fn run_now(&self, id: i64) -> Result<()> {
        let task = self
            .store
            .get(id)?
            .ok_or_else(|| anyhow::anyhow!("task #{id} 不存在"))?;
        // 如果 pending 也允许跑（不强制审批）
        super::worker::run_task(self.store.clone(), task).await
    }
}

/// 启动 hermes（构造 registry + 后台 tick）。
/// 独立函数，构造时不依赖 AppContext —— AppContext 里再放 hermes 字段。
pub fn start() -> HermesRegistry {
    let db_path = crate::config::paths::hermes_db_path();
    let store = match super::store::TaskStore::open(&db_path) {
        Ok(s) => Arc::new(s),
        Err(_) => Arc::new(super::store::TaskStore::open_in_memory().expect("in-mem hermes fallback")),
    };
    let reg = HermesRegistry::new(store);
    reg.start_background();
    reg
}

#[allow(dead_code)]
pub fn _unused_ctx_marker() {}
