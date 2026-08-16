//! Plan DAG：节点 + 边 + 状态机。
//!
//! 状态机：
//!   pending → running → {done | failed | blocked | needs_human}
//!   failed → needs_human  (R3 自愈升级)
//!   blocked: dep 还没跑完

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum PlanNodeStatus {
    Pending,        // 等 dep
    Ready,          // dep 都完成，可以跑
    Running,        // 在跑
    Done,           // 成功
    Failed,         // 失败（自愈后还能重试）
    NeedsHuman,     // 升级到 plan mode
    Blocked,        // 永久 block（dep 永久失败）
}

impl PlanNodeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlanNodeStatus::Pending => "pending",
            PlanNodeStatus::Ready => "ready",
            PlanNodeStatus::Running => "running",
            PlanNodeStatus::Done => "done",
            PlanNodeStatus::Failed => "failed",
            PlanNodeStatus::NeedsHuman => "needs_human",
            PlanNodeStatus::Blocked => "blocked",
        }
    }
    pub fn is_terminal(&self) -> bool {
        matches!(self, PlanNodeStatus::Done | PlanNodeStatus::NeedsHuman | PlanNodeStatus::Blocked)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanNode {
    /// 节点 id（plan 内唯一）
    pub id: String,
    /// 显示名
    pub name: String,
    /// 自然语言 prompt（喂给 executor 调 LLM）
    pub prompt: String,
    /// 期望产出（给 LLM 看）
    #[serde(default)]
    pub expected_output: Option<String>,
    /// 最大自愈重试次数
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// 当前已重试次数
    #[serde(default)]
    pub retry_count: u32,
    /// 状态
    pub status: PlanNodeStatus,
    /// 跑过的历史
    #[serde(default)]
    pub runs: Vec<NodeRun>,
}

fn default_max_retries() -> u32 { 2 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRun {
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: PlanNodeStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    /// 自愈策略
    #[serde(default)]
    pub heal_strategy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanEdge {
    pub from: String, // upstream node id
    pub to: String,   // downstream node id
}

/// 整张 plan（DAG）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDag {
    pub id: i64,
    pub name: String,
    pub goal: String,
    pub nodes: Vec<PlanNode>,
    pub edges: Vec<PlanEdge>,
    pub created_at: i64,
    pub status: PlanStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlanStatus {
    Draft,        // 拆解完还没跑
    Running,
    Completed,    // 所有 node 都 done
    Failed,       // 至少一个 needs_human
    Cancelled,
}

impl PlanStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlanStatus::Draft => "draft",
            PlanStatus::Running => "running",
            PlanStatus::Completed => "completed",
            PlanStatus::Failed => "failed",
            PlanStatus::Cancelled => "cancelled",
        }
    }
    pub fn is_terminal(&self) -> bool {
        matches!(self, PlanStatus::Completed | PlanStatus::Failed | PlanStatus::Cancelled)
    }
}

impl PlanDag {
    pub fn new(name: impl Into<String>, goal: impl Into<String>) -> Self {
        Self {
            id: 0,
            name: name.into(),
            goal: goal.into(),
            nodes: Vec::new(),
            edges: Vec::new(),
            created_at: chrono::Utc::now().timestamp(),
            status: PlanStatus::Draft,
        }
    }

    pub fn add_node(&mut self, node: PlanNode) {
        self.nodes.push(node);
    }
    pub fn add_edge(&mut self, from: impl Into<String>, to: impl Into<String>) {
        self.edges.push(PlanEdge { from: from.into(), to: to.into() });
    }

    /// 找「现在可以跑」的节点（status=Ready 且没在跑）
    pub fn ready_nodes(&self) -> Vec<&PlanNode> {
        let done: HashSet<&str> = self
            .nodes
            .iter()
            .filter(|n| n.status == PlanNodeStatus::Done)
            .map(|n| n.id.as_str())
            .collect();
        let blocked_deps: HashSet<&str> = self
            .nodes
            .iter()
            .filter(|n| matches!(n.status, PlanNodeStatus::Failed | PlanNodeStatus::NeedsHuman | PlanNodeStatus::Blocked))
            .map(|n| n.id.as_str())
            .collect();
        self.nodes
            .iter()
            .filter(|n| {
                if n.status != PlanNodeStatus::Ready && n.status != PlanNodeStatus::Pending {
                    return false;
                }
                // 找所有 upstream
                let ups: Vec<&str> = self
                    .edges
                    .iter()
                    .filter(|e| e.to == n.id)
                    .map(|e| e.from.as_str())
                    .collect();
                if ups.is_empty() {
                    return n.status == PlanNodeStatus::Ready || n.status == PlanNodeStatus::Pending;
                }
                let all_done = ups.iter().all(|u| done.contains(u));
                let any_blocked = ups.iter().any(|u| blocked_deps.contains(u));
                if any_blocked {
                    false
                } else {
                    all_done
                }
            })
            .collect()
    }

    /// 拓扑序返回所有节点
    pub fn topo_order(&self) -> Vec<String> {
        let mut indeg: HashMap<&str, usize> = HashMap::new();
        for n in &self.nodes {
            indeg.entry(n.id.as_str()).or_insert(0);
        }
        for e in &self.edges {
            *indeg.entry(e.to.as_str()).or_insert(0) += 1;
        }
        let mut q: VecDeque<&str> = indeg
            .iter()
            .filter(|(_, &v)| v == 0)
            .map(|(k, _)| *k)
            .collect();
        let mut out = Vec::new();
        while let Some(n) = q.pop_front() {
            out.push(n.to_string());
            for e in &self.edges {
                if e.from == n {
                    if let Some(d) = indeg.get_mut(e.to.as_str()) {
                        *d -= 1;
                        if *d == 0 {
                            q.push_back(e.to.as_str());
                        }
                    }
                }
            }
        }
        out
    }

    /// 检测环
    pub fn has_cycle(&self) -> bool {
        self.topo_order().len() != self.nodes.len()
    }

    pub fn mark_ready_initial(&mut self) {
        // 启动时把没有 upstream 的节点置为 Ready
        let with_ups: HashSet<&str> = self
            .edges
            .iter()
            .map(|e| e.to.as_str())
            .collect();
        for n in &mut self.nodes {
            if !with_ups.contains(n.id.as_str()) && n.status == PlanNodeStatus::Pending {
                n.status = PlanNodeStatus::Ready;
            }
        }
    }

    /// 节点跑完后，更新下游状态
    pub fn propagate_done(&mut self, node_id: &str) {
        // 找下游
        let downs: Vec<String> = self
            .edges
            .iter()
            .filter(|e| e.from == node_id)
            .map(|e| e.to.clone())
            .collect();
        // 第一阶段：算好哪些下游要 mark Ready（全部在 immut borrow 下完成）
        let to_ready: Vec<String> = {
            let done_ids: HashSet<String> = self
                .nodes
                .iter()
                .filter(|n| n.status == PlanNodeStatus::Done)
                .map(|n| n.id.clone())
                .collect();
            downs
                .iter()
                .filter(|d| {
                    // 只考虑当前是 Pending 的下游
                    let cur_pending = self
                        .nodes
                        .iter()
                        .any(|n| n.id == **d && n.status == PlanNodeStatus::Pending);
                    if !cur_pending {
                        return false;
                    }
                    let ups: Vec<&str> = self
                        .edges
                        .iter()
                        .filter(|e| &e.to == *d)
                        .map(|e| e.from.as_str())
                        .collect();
                    ups.iter().all(|u| done_ids.contains(*u))
                })
                .cloned()
                .collect()
        };
        // 第二阶段：mut borrow，done_ids 已 drop
        for d in to_ready {
            if let Some(n) = self.nodes.iter_mut().find(|n| n.id == d) {
                n.status = PlanNodeStatus::Ready;
            }
        }
    }

    /// 节点永久失败时，block 所有下游
    pub fn propagate_blocked(&mut self, node_id: &str) {
        let downs: Vec<String> = self
            .edges
            .iter()
            .filter(|e| e.from == node_id)
            .map(|e| e.to.clone())
            .collect();
        for d in downs {
            if let Some(n) = self.nodes.iter_mut().find(|n| n.id == d) {
                if !n.status.is_terminal() {
                    n.status = PlanNodeStatus::Blocked;
                }
            }
        }
    }

    pub fn summary(&self) -> String {
        let n_done = self.nodes.iter().filter(|n| n.status == PlanNodeStatus::Done).count();
        let n_running = self.nodes.iter().filter(|n| n.status == PlanNodeStatus::Running).count();
        let n_ready = self.nodes.iter().filter(|n| n.status == PlanNodeStatus::Ready).count();
        let n_failed = self.nodes.iter().filter(|n| matches!(n.status, PlanNodeStatus::Failed | PlanNodeStatus::NeedsHuman)).count();
        format!(
            "{}: {} nodes — {} done, {} running, {} ready, {} failed",
            self.name,
            self.nodes.len(),
            n_done,
            n_running,
            n_ready,
            n_failed,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(id: &str) -> PlanNode {
        PlanNode {
            id: id.to_string(),
            name: id.to_string(),
            prompt: String::new(),
            expected_output: None,
            max_retries: 2,
            retry_count: 0,
            status: PlanNodeStatus::Pending,
            runs: vec![],
        }
    }

    #[test]
    fn topo_simple_dag() {
        let mut p = PlanDag::new("p", "g");
        p.add_node(n("a"));
        p.add_node(n("b"));
        p.add_node(n("c"));
        p.add_edge("a", "b");
        p.add_edge("a", "c");
        assert!(!p.has_cycle());
        let order = p.topo_order();
        assert_eq!(order.len(), 3);
        assert_eq!(order[0], "a");
    }

    #[test]
    fn detect_cycle() {
        let mut p = PlanDag::new("p", "g");
        p.add_node(n("a"));
        p.add_node(n("b"));
        p.add_edge("a", "b");
        p.add_edge("b", "a");
        assert!(p.has_cycle());
    }

    #[test]
    fn ready_propagation() {
        let mut p = PlanDag::new("p", "g");
        p.add_node(n("a"));
        p.add_node(n("b"));
        p.add_node(n("c"));
        p.add_edge("a", "b");
        p.add_edge("a", "c");
        p.mark_ready_initial();
        // a 应 Ready，b/c 仍 Pending
        assert_eq!(p.nodes[0].status, PlanNodeStatus::Ready);
        assert_eq!(p.nodes[1].status, PlanNodeStatus::Pending);
        // 标 a done
        p.nodes[0].status = PlanNodeStatus::Done;
        p.propagate_done("a");
        // b/c 应 Ready
        assert_eq!(p.nodes[1].status, PlanNodeStatus::Ready);
        assert_eq!(p.nodes[2].status, PlanNodeStatus::Ready);
    }

    #[test]
    fn blocked_propagation() {
        let mut p = PlanDag::new("p", "g");
        p.add_node(n("a"));
        p.add_node(n("b"));
        p.add_edge("a", "b");
        p.mark_ready_initial();
        // a 永久失败
        p.nodes[0].status = PlanNodeStatus::Blocked;
        p.propagate_blocked("a");
        assert_eq!(p.nodes[1].status, PlanNodeStatus::Blocked);
    }
}
