//! PlanEngine ── PlanDAG 调度器
//!
//! 用法：
//! 1. `PlanEngine::new(store, chain, alias)` 构造
//! 2. `engine.start(goal)` → async 返回 PlanDag（LLM 拆解完，已 mark_ready_initial）
//! 3. `engine.run_plan(dag_id)` → 跑到 Completed/Failed/Cancelled
//!
//! 并发：每轮把 ready 节点用 tokio::JoinSet 并发执行；用 tokio::sync::Mutex<PlanDag> 共享 dag。
//! 进度：每完成一个 node 写回 store + propagate_done。

use super::dag::{PlanDag, PlanNodeStatus, PlanStatus};
use super::decompose::decompose;
use super::executor::{execute_with_heal, ExecOutcome, ExecOptions};
use super::goal::Goal;
use super::store::PlanStore;
use crate::llm::registry::FallbackChain;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinSet;

pub struct PlanEngine {
    store: Arc<PlanStore>,
    /// Plan 跑起来后 chain 不再被替换（用户即便 /config reload 也不影响已开始的 plan），
    /// 所以持有不可变 Arc 即可。chat_with_fallback 只需 &self。
    chain: Arc<FallbackChain>,
    default_alias: String,
    /// dag_id → (Arc<Mutex<PlanDag>>)
    /// 调度时 clone Arc，让 executor 也能改 dag
    live: Arc<AsyncMutex<HashMap<i64, Arc<AsyncMutex<PlanDag>>>>>,
}

impl PlanEngine {
    /// 从带 RwLock 的 chain 构造（典型用法：clone 一份当前 chain 给 engine）
    pub fn new(
        store: Arc<PlanStore>,
        chain: Arc<FallbackChain>,
        default_alias: impl Into<String>,
    ) -> Self {
        Self {
            store,
            chain,
            default_alias: default_alias.into(),
            live: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    /// 1) 调 LLM 拆 goal → 2) 存 store → 3) 标记 ready → 4) 启动调度
    pub async fn start(&self, goal: Goal) -> Result<i64> {
        let mut dag = decompose(&self.chain, &self.default_alias, &goal).await?;
        if dag.has_cycle() {
            return Err(anyhow!("decomposed DAG has cycle, refusing to start"));
        }
        if dag.nodes.is_empty() {
            return Err(anyhow!("LLM returned empty plan"));
        }
        dag.mark_ready_initial();
        dag.status = PlanStatus::Running;
        let id = self.store.save(&dag)?;
        dag.id = id;
        self.live
            .lock()
            .await
            .insert(id, Arc::new(AsyncMutex::new(dag)));
        Ok(id)
    }

    /// 拿当前 in-memory dag（start 之后）
    pub async fn snapshot(&self, id: i64) -> Result<Option<PlanDag>> {
        let live = self.live.lock().await;
        if let Some(m) = live.get(&id) {
            Ok(Some(m.lock().await.clone()))
        } else {
            Ok(self.store.get(id)?)
        }
    }

    /// 跑 plan（同步等所有 node 完成 / 永久失败 / 升级人审批）
    pub async fn run_plan(&self, id: i64) -> Result<RunSummary> {
        let dag_arc = {
            let live = self.live.lock().await;
            live.get(&id)
                .cloned()
                .ok_or_else(|| anyhow!("plan not in live cache; call start() first or load from store. id = {}", id))?
        };

        let alias = self.default_alias.clone();
        let chain = self.chain.clone();
        let store = self.store.clone();

        let mut join_set: JoinSet<NodeResult> = JoinSet::new();
        let total_nodes = dag_arc.lock().await.nodes.len();

        // 启动所有 Ready node
        let initial_ready = {
            let dag = dag_arc.lock().await;
            dag.ready_nodes().into_iter().map(|n| n.id.clone()).collect::<Vec<_>>()
        };
        for nid in initial_ready {
            join_set.spawn(schedule_node(
                dag_arc.clone(),
                store.clone(),
                chain.clone(),
                alias.clone(),
                nid,
            ));
        }

        // 主循环：等任意 node 完成 → 处理 → 拉新的 ready → 重复
        while let Some(joined) = join_set.join_next().await {
            let res = joined.map_err(|e| anyhow!("node task join error: {}", e))?;
            match res {
                NodeResult::Done { node_id, .. } => {
                    let mut dag = dag_arc.lock().await;
                    dag.propagate_done(&node_id);
                    // 拉新 ready
                    let next = dag.ready_nodes().into_iter().map(|n| n.id.clone()).collect::<Vec<_>>();
                    drop(dag);
                    for nid in next {
                        join_set.spawn(schedule_node(
                            dag_arc.clone(),
                            store.clone(),
                            chain.clone(),
                            alias.clone(),
                            nid,
                        ));
                    }
                }
                NodeResult::NeedsHuman { node_id, error: _ } => {
                    let mut dag = dag_arc.lock().await;
                    if let Some(n) = dag.nodes.iter_mut().find(|n| n.id == node_id) {
                        n.status = PlanNodeStatus::NeedsHuman;
                    }
                    dag.propagate_blocked(&node_id);
                }
                NodeResult::Failed { node_id, error: _ } => {
                    let mut dag = dag_arc.lock().await;
                    if let Some(n) = dag.nodes.iter_mut().find(|n| n.id == node_id) {
                        n.status = PlanNodeStatus::Blocked;
                    }
                    dag.propagate_blocked(&node_id);
                }
            }
            // 写盘（best-effort）
            let snapshot = dag_arc.lock().await.clone();
            let _ = store.save(&snapshot);
        }

        // 收尾
        let final_dag = dag_arc.lock().await.clone();
        let summary = final_dag.summary();
        let all_done = final_dag
            .nodes
            .iter()
            .all(|n| n.status == PlanNodeStatus::Done);
        let any_blocked = final_dag
            .nodes
            .iter()
            .any(|n| n.status == PlanNodeStatus::NeedsHuman);
        let status = if all_done {
            PlanStatus::Completed
        } else if any_blocked {
            PlanStatus::Failed
        } else {
            PlanStatus::Cancelled
        };

        let mut final_dag = final_dag;
        final_dag.status = status;
        let _ = self.store.save(&final_dag);

        Ok(RunSummary {
            plan_id: final_dag.id,
            total_nodes,
            summary,
            status,
        })
    }
}

pub struct RunSummary {
    pub plan_id: i64,
    pub total_nodes: usize,
    pub summary: String,
    pub status: PlanStatus,
}

#[allow(dead_code)]
enum NodeResult {
    Done { node_id: String, output: String },
    NeedsHuman { node_id: String, error: String },
    Failed { node_id: String, error: String },
}

async fn schedule_node(
    dag_arc: Arc<AsyncMutex<PlanDag>>,
    store: Arc<PlanStore>,
    chain: Arc<FallbackChain>,
    alias: String,
    node_id: String,
) -> NodeResult {
    // 1. 拿 upstream 输出
    let upstream_outputs: Vec<String> = {
        let dag = dag_arc.lock().await;
        let ups: Vec<String> = dag
            .edges
            .iter()
            .filter(|e| e.to == node_id)
            .map(|e| e.from.clone())
            .collect();
        ups.iter()
            .filter_map(|up| {
                dag.nodes
                    .iter()
                    .find(|n| n.id == *up && n.status == PlanNodeStatus::Done)
                    .and_then(|n| n.runs.last().and_then(|r| r.output.clone()))
            })
            .collect()
    };

    // 2. 跑（含自愈）
    let opts = ExecOptions { alias: alias.clone(), max_tokens: 2048, temperature: 0.5 };
    let outcome = {
        let mut dag = dag_arc.lock().await;
        let node = match dag.nodes.iter_mut().find(|n| n.id == node_id) {
            Some(n) => n,
            None => {
                return NodeResult::Failed {
                    node_id,
                    error: "node 不存在（被中途删了？）".into(),
                };
            }
        };
        match execute_with_heal(&chain, node, &upstream_outputs, &opts).await {
            Ok(o) => o,
            Err(e) => {
                return NodeResult::Failed { node_id, error: format!("executor crashed: {e:#}") };
            }
        }
    };

    match outcome {
        ExecOutcome::Done(output) => {
            let snapshot = dag_arc.lock().await.clone();
            let _ = store.save(&snapshot);
            NodeResult::Done { node_id, output }
        }
        ExecOutcome::NeedsHuman { last_error } => {
            NodeResult::NeedsHuman { node_id, error: last_error }
        }
        ExecOutcome::Failed { last_error } => {
            NodeResult::Failed { node_id, error: last_error }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::registry::FallbackChain;

    #[tokio::test]
    async fn engine_lifecycle() {
        let store = Arc::new(PlanStore::open_in_memory().unwrap());
        let chain = Arc::new(FallbackChain::new());
        let engine = PlanEngine::new(store.clone(), chain, "noop");
        // 没 goal 也能用，只是没真跑 decompose
        assert!(engine.live.lock().await.is_empty());
    }
}
