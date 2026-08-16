//! Goal → DAG 自动拆解
//!
//! 调 LLM（带 system prompt 教它怎么拆），返回 `Vec<NodeSpec>` + `Vec<EdgeSpec>`。
//! 然后组装成 `PlanDag`。
//!
//! 拆解 prompt 设计的 4 个原则：
//! 1. 每节点是「可独立完成、有明确产出」的小任务（不是「研究」「分析」这种大词）
//! 2. 节点之间用边表达依赖（不是顺序号）
//! 3. 尽量「能并行的并行」（不要把可以并发的任务强行串起来）
//! 4. 失败兜底：至少一个能产生最终交付物的 leaf node

use super::dag::{PlanDag, PlanNode, PlanNodeStatus};
use super::goal::Goal;
use crate::llm::message::{Message, Role};
use crate::llm::provider::CompletionRequest;
use crate::llm::registry::FallbackChain;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSpec {
    pub id: String,
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub expected_output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeSpec {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecomposeOutput {
    pub nodes: Vec<NodeSpec>,
    pub edges: Vec<EdgeSpec>,
    /// 简短解释（给 LLM 输出 plan 后给用户看）
    #[serde(default)]
    pub rationale: Option<String>,
}

const DECOMPOSE_SYSTEM: &str = r#"你是 fr-claw 的"目标拆解器"。把用户给的高层 goal 拆成一张有向无环图（DAG），每个节点是一个可独立完成、有明确产出的任务。

输出 JSON 格式（必须严格遵守）：
```json
{
  "nodes": [
    { "id": "collect", "name": "采集本周价格", "prompt": "...", "expected_output": "json 数组" }
  ],
  "edges": [
    { "from": "collect", "to": "report" }
  ],
  "rationale": "为什么这么拆"
}
```

拆解原则：
1. **粒度要细**：每个节点应能在 1 次 LLM 调用 + 0-3 个工具调用内完成。不要写"研究"、"分析"这种大词，要写"采集竞品 A 本周价格"这种可执行的小任务。
2. **并行最大化**：可以并行的不要强串。如果 A 和 B 没依赖，它们可以同时跑。
3. **必须包含 leaf**：至少有 1 个能产出最终交付物的节点（如"生成报告"、"发送通知"）。
4. **边表依赖**：用 from→to 表达"from 完成后才能跑 to"，不要用顺序号。
5. **不要环**：A→B→A 会卡死。
6. **id 短小**：用动词（collect / analyze / report / send）别用 task_001。

只输出 JSON，不要解释。"#;

pub async fn decompose(
    chain: &Arc<FallbackChain>,
    alias: &str,
    goal: &Goal,
) -> Result<PlanDag> {
    let user_prompt = build_user_prompt(goal);

    let req = CompletionRequest {
        messages: vec![
            Message { role: Role::System, content: DECOMPOSE_SYSTEM.to_string(), tool_call_id: None, tool_calls: vec![], name: None },
            Message { role: Role::User, content: user_prompt, tool_call_id: None, tool_calls: vec![], name: None },
        ],
        tools: vec![],  // 拆解不用 tool
        temperature: Some(0.2),
        max_tokens: Some(2048),
        force_non_stream: true,
    };

    let (_used, resp) = chain
        .chat_with_fallback(alias, req)
        .await
        .map_err(|e| anyhow!("decompose LLM call failed: {:#}", e))?;

    let spec = parse_decompose_output(&resp.content)?;
    Ok(spec_to_dag(goal, &spec))
}

fn build_user_prompt(goal: &Goal) -> String {
    let mut s = format!("高层目标：{}\n", goal.description);
    if let Some(ctx) = &goal.context {
        s.push_str(&format!("\n上下文：{ctx}\n"));
    }
    if !goal.success_criteria.is_empty() {
        s.push_str("\n成功判据：\n");
        for c in &goal.success_criteria {
            s.push_str(&format!("- {c}\n"));
        }
    }
    s
}

fn parse_decompose_output(raw: &str) -> Result<DecomposeOutput> {
    // LLM 输出可能带 ```json ... ``` 包一层
    let cleaned = strip_code_fence(raw);
    serde_json::from_str(&cleaned).map_err(|e| {
        anyhow!(
            "decompose JSON parse failed: {e}\n--- raw LLM output ---\n{raw}\n--- end ---"
        )
    })
}

fn strip_code_fence(s: &str) -> String {
    let s = s.trim();
    // 去掉 ```json ... ```
    if let Some(rest) = s.strip_prefix("```json") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    if let Some(rest) = s.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    s.to_string()
}

fn spec_to_dag(goal: &Goal, spec: &DecomposeOutput) -> PlanDag {
    let mut dag = PlanDag::new(
        format!("plan-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S")),
        &goal.description,
    );
    for n in &spec.nodes {
        dag.add_node(PlanNode {
            id: n.id.clone(),
            name: n.name.clone(),
            prompt: n.prompt.clone(),
            expected_output: n.expected_output.clone(),
            max_retries: 2,
            retry_count: 0,
            status: PlanNodeStatus::Pending,
            runs: vec![],
        });
    }
    for e in &spec.edges {
        dag.add_edge(&e.from, &e.to);
    }
    // 校验：发现自环直接 drop
    dag.edges.retain(|e| e.from != e.to);
    dag.mark_ready_initial();
    dag
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_code_fence_basic() {
        assert_eq!(strip_code_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fence("```\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fence("{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn spec_to_dag_basic() {
        let goal = Goal::new("盯竞品");
        let json = r#"{
            "nodes": [
                {"id":"collect","name":"采价","prompt":"p1","expected_output":"json"},
                {"id":"compare","name":"对比","prompt":"p2"},
                {"id":"report","name":"报告","prompt":"p3"}
            ],
            "edges": [
                {"from":"collect","to":"compare"},
                {"from":"compare","to":"report"}
            ],
            "rationale": "线性"
        }"#;
        let spec: DecomposeOutput = serde_json::from_str(json).unwrap();
        let dag = spec_to_dag(&goal, &spec);
        assert_eq!(dag.nodes.len(), 3);
        assert_eq!(dag.edges.len(), 2);
        assert!(!dag.has_cycle());
        // collect 应 Ready，compare/report Pending
        assert_eq!(dag.nodes[0].status, PlanNodeStatus::Ready);
        assert_eq!(dag.nodes[1].status, PlanNodeStatus::Pending);
    }

    #[test]
    fn spec_to_dag_drops_self_loop() {
        let goal = Goal::new("test");
        let json = r#"{
            "nodes": [{"id":"a","name":"A","prompt":"p"}],
            "edges": [{"from":"a","to":"a"}]
        }"#;
        let spec: DecomposeOutput = serde_json::from_str(json).unwrap();
        let dag = spec_to_dag(&goal, &spec);
        // 自环被 drop
        assert!(dag.edges.is_empty());
    }
}
