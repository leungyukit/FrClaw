//! Executor ── 跑一个 PlanNode
//!
//! 给一个 node，调 LLM（带 system prompt 教它怎么产 expected_output）。
//! 成功 → Done；失败 → 抛 error 给 caller 决定 heal。

use super::dag::{NodeRun, PlanNode, PlanNodeStatus};
use super::self_heal::{decide_heal, mutate_prompt, HealAction};
use crate::llm::message::{Message, Role};
use crate::llm::provider::CompletionRequest;
use crate::llm::registry::FallbackChain;
use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;

const NODE_SYSTEM: &str = r#"你是 fr-claw 的 Plan Executor。正在执行一个有向无环图（DAG）中的某个节点。

输入：你只能看到一个独立任务（不要做超范围的事）。如果任务信息不够，请输出 "ERROR: <缺什么信息>" 让我升级到 plan mode。

输出：尽量用结构化形式（json / 列表 / markdown）方便下游节点消费。如果任务要求"自然语言一段话"，也直接写。

只输出任务结果，不要重复任务描述。"#;

pub struct ExecOptions {
    pub alias: String,
    pub max_tokens: u32,
    pub temperature: f32,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self {
            alias: String::new(),
            max_tokens: 2048,
            temperature: 0.5,
        }
    }
}

/// 跑一个 node；返回 (NodeRun, error_if_failed)
/// - retry_strategy 由 decide_heal 决定
/// - 调用方根据 error 决定是否再调一次
pub async fn execute_node(
    chain: &Arc<FallbackChain>,
    node: &mut PlanNode,
    upstream_outputs: &[String],  // 上游节点的输出（拼到 user prompt）
    opts: &ExecOptions,
) -> Result<NodeRun> {
    let started = Utc::now().timestamp();
    node.status = PlanNodeStatus::Running;
    let user_prompt = build_user_prompt(node, upstream_outputs);
    let req = CompletionRequest {
        messages: vec![
            Message { role: Role::System, content: NODE_SYSTEM.to_string(), tool_call_id: None, tool_calls: vec![], name: None },
            Message { role: Role::User, content: user_prompt, tool_call_id: None, tool_calls: vec![], name: None },
        ],
        tools: vec![],
        temperature: Some(opts.temperature),
        max_tokens: Some(opts.max_tokens),
        force_non_stream: true,
    };
    let run = match chain.chat_with_fallback(&opts.alias, req).await {
        Ok((_used, resp)) => NodeRun {
            started_at: started,
            finished_at: Some(Utc::now().timestamp()),
            status: PlanNodeStatus::Done,
            output: Some(resp.content),
            error: None,
            heal_strategy: None,
        },
        Err(e) => NodeRun {
            started_at: started,
            finished_at: Some(Utc::now().timestamp()),
            status: PlanNodeStatus::Failed,
            output: None,
            error: Some(format!("{e:#}")),
            heal_strategy: Some(format!("{:?}", decide_heal(node, &format!("{e}")))),
        },
    };
    Ok(run)
}

fn build_user_prompt(node: &PlanNode, upstream_outputs: &[String]) -> String {
    let mut s = String::new();
    s.push_str(&format!("# 任务\n{}\n", node.prompt));
    if let Some(exp) = &node.expected_output {
        s.push_str(&format!("\n# 期望产出\n{exp}\n"));
    }
    if !upstream_outputs.is_empty() {
        s.push_str("\n# 上游节点的输出（按顺序）\n");
        for (i, o) in upstream_outputs.iter().enumerate() {
            s.push_str(&format!("\n## 上游 #{} 输出\n{}\n", i + 1, o));
        }
    }
    s
}

/// Self-Heal wrapper：失败时根据 decide_heal 决定下一步
pub enum ExecOutcome {
    Done(String),
    /// 升级人审批
    NeedsHuman { last_error: String },
    /// 真的搞不动
    Failed { last_error: String },
}

pub async fn execute_with_heal(
    chain: &Arc<FallbackChain>,
    node: &mut PlanNode,
    upstream_outputs: &[String],
    opts: &ExecOptions,
) -> Result<ExecOutcome> {
    // 重试循环：跑 → 失败 → 决定 heal → 改 prompt → 再跑
    // 关键：mutate_prompt 只在本次循环内用，不写回 node.prompt（保持 plan 干净）
    let max_attempts = (node.max_retries as usize) + 1;
    let original_prompt = node.prompt.clone();
    let mut last_error = String::new();
    for attempt in 0..max_attempts {
        if attempt > 0 {
            // 用上一轮的 error 决定 heal action
            let action = decide_heal(node, &last_error);
            // 临时 mutate prompt（不写回 node.prompt）
            let runtime_prompt = match action {
                HealAction::RetrySame => original_prompt.clone(),
                HealAction::RetryMutate => mutate_prompt(&original_prompt, attempt as u32),
                HealAction::RetryRestrictTools => format!(
                    "{}\n\n[tools restricted: 不准用 web_search / shell，只用 read_file / write_file]",
                    original_prompt
                ),
                HealAction::Escalate => {
                    node.prompt = original_prompt; // 还原
                    return Ok(ExecOutcome::NeedsHuman { last_error });
                }
                HealAction::GiveUp => {
                    node.prompt = original_prompt;
                    return Ok(ExecOutcome::Failed { last_error });
                }
            };
            // 用 runtime_prompt 跑一次（不污染 node.prompt）
            let run = execute_node_with_prompt(
                chain,
                &runtime_prompt,
                &node.name,
                node.expected_output.as_deref(),
                upstream_outputs,
                opts,
            )
            .await?;
            if run.status == PlanNodeStatus::Done {
                node.runs.push(run.clone());
                node.status = PlanNodeStatus::Done;
                node.prompt = original_prompt;
                return Ok(ExecOutcome::Done(run.output.unwrap_or_default()));
            }
            last_error = run.error.clone().unwrap_or_default();
            node.runs.push(run);
            node.retry_count += 1;
            node.status = PlanNodeStatus::Failed;
        } else {
            let run = execute_node(chain, node, upstream_outputs, opts).await?;
            if run.status == PlanNodeStatus::Done {
                node.runs.push(run.clone());
                node.status = PlanNodeStatus::Done;
                return Ok(ExecOutcome::Done(run.output.unwrap_or_default()));
            }
            last_error = run.error.clone().unwrap_or_default();
            node.runs.push(run);
            node.retry_count += 1;
            node.status = PlanNodeStatus::Failed;
        }
    }
    // 用尽 max_retries
    node.prompt = original_prompt;
    let action = decide_heal(node, &last_error);
    Ok(match action {
        HealAction::Escalate | HealAction::GiveUp => ExecOutcome::NeedsHuman { last_error },
        _ => ExecOutcome::Failed { last_error },
    })
}

/// 跑一次（带临时 prompt，不改 node）
async fn execute_node_with_prompt(
    chain: &Arc<FallbackChain>,
    prompt: &str,
    name: &str,
    expected_output: Option<&str>,
    upstream_outputs: &[String],
    opts: &ExecOptions,
) -> Result<NodeRun> {
    use crate::llm::message::{Message, Role};
    use crate::llm::provider::CompletionRequest;
    let started = chrono::Utc::now().timestamp();
    let mut user_msg = format!("# 任务\n{}\n", prompt);
    if let Some(exp) = expected_output {
        user_msg.push_str(&format!("\n# 期望产出\n{exp}\n"));
    }
    if !upstream_outputs.is_empty() {
        user_msg.push_str("\n# 上游节点的输出（按顺序）\n");
        for (i, o) in upstream_outputs.iter().enumerate() {
            user_msg.push_str(&format!("\n## 上游 #{} 输出\n{}\n", i + 1, o));
        }
    }
    let req = CompletionRequest {
        messages: vec![
            Message { role: Role::System, content: format!("{NODE_SYSTEM}\n# 当前节点: {name}"), tool_call_id: None, tool_calls: vec![], name: None },
            Message { role: Role::User, content: user_msg, tool_call_id: None, tool_calls: vec![], name: None },
        ],
        tools: vec![],
        temperature: Some(opts.temperature),
        max_tokens: Some(opts.max_tokens),
        force_non_stream: true,
    };
    let run = match chain.chat_with_fallback(&opts.alias, req).await {
        Ok((_used, resp)) => NodeRun {
            started_at: started,
            finished_at: Some(chrono::Utc::now().timestamp()),
            status: PlanNodeStatus::Done,
            output: Some(resp.content),
            error: None,
            heal_strategy: None,
        },
        Err(e) => NodeRun {
            started_at: started,
            finished_at: Some(chrono::Utc::now().timestamp()),
            status: PlanNodeStatus::Failed,
            output: None,
            error: Some(format!("{e:#}")),
            heal_strategy: None,
        },
    };
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_user_prompt_includes_upstream() {
        let n = PlanNode {
            id: "x".into(),
            name: "x".into(),
            prompt: "汇总数据".into(),
            expected_output: Some("json".into()),
            max_retries: 2,
            retry_count: 0,
            status: PlanNodeStatus::Ready,
            runs: vec![],
        };
        let s = build_user_prompt(&n, &[String::from("first"), String::from("second")]);
        assert!(s.contains("汇总数据"));
        assert!(s.contains("期望产出"));
        assert!(s.contains("上游 #1"));
        assert!(s.contains("first"));
        assert!(s.contains("second"));
    }
}
