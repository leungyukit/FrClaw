//! Self-Heal：失败 → 重试 → 换工具 → 升级 plan mode
//!
//! 策略分层（按 retry_count 升级）：
//!   0 → 1: 重试相同 prompt（LLM 偶发不稳）
//!   1 → 2: 把 prompt 简化 + 加 "you can ignore previous errors"
//!   2 → 3: 换工具子集（关掉 web_search / shell 等可能超时的）
//!   3+: 升级 needs_human，等人审批
//!
//! 真实生产应该让 LLM 评估错误类型选策略；MVP 用 heuristic。

use super::dag::{NodeRun, PlanNode, PlanNodeStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealAction {
    /// 原样重跑
    RetrySame,
    /// 改 prompt 重试
    RetryMutate,
    /// 换工具子集
    RetryRestrictTools,
    /// 升级到人审批
    Escalate,
    /// 真的搞不动了，永久失败
    GiveUp,
}

pub fn decide_heal(node: &PlanNode, last_error: &str) -> HealAction {
    let n = node.retry_count;
    let err_lower = last_error.to_lowercase();
    if n == 0 {
        return HealAction::RetrySame;
    }
    if n == 1 {
        return HealAction::RetryMutate;
    }
    if n == 2 {
        // 超时 / 限流 类错误 → 换工具子集重试；其他直接升级
        if err_lower.contains("timeout")
            || err_lower.contains("rate limit")
            || err_lower.contains("429")
            || err_lower.contains("503")
            || err_lower.contains("deadline")
        {
            return HealAction::RetryRestrictTools;
        }
        return HealAction::Escalate;
    }
    if n >= 3 {
        return HealAction::Escalate;
    }
    HealAction::GiveUp
}

/// 给 prompt 加一段 mutation（"忽略之前的错误，聚焦 X"）
pub fn mutate_prompt(orig: &str, retry: u32) -> String {
    let suffix = match retry {
        1 => "\n\n[retry hint: 之前失败了，请重读任务、改个角度重新做]",
        2 => "\n\n[retry hint: 之前失败 2 次。这次只做最核心的部分，跳过边角。]",
        _ => "\n\n[retry hint: 之前失败 N 次。聚焦最小可交付物。]",
    };
    format!("{orig}{suffix}")
}

pub fn record_run(node: &mut PlanNode, run: NodeRun) {
    let finished = run.finished_at.is_some();
    let success = run.status == PlanNodeStatus::Done;
    node.runs.push(run);
    if success {
        node.status = PlanNodeStatus::Done;
    } else if finished {
        // 失败时根据 heal 决定下一步（caller 已经调过 decide_heal）
        // 这里只更新 retry_count + 设 Failed 状态（caller 视需要升级）
        node.retry_count += 1;
        node.status = PlanNodeStatus::Failed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn n() -> PlanNode {
        PlanNode {
            id: "x".into(),
            name: "x".into(),
            prompt: "p".into(),
            expected_output: None,
            max_retries: 5,
            retry_count: 0,
            status: PlanNodeStatus::Running,
            runs: vec![],
        }
    }

    #[test]
    fn heal_progression() {
        let mut node = n();
        // 0: RetrySame
        assert_eq!(decide_heal(&node, "any"), HealAction::RetrySame);
        node.retry_count = 1;
        assert_eq!(decide_heal(&node, "any"), HealAction::RetryMutate);
        node.retry_count = 2;
        assert_eq!(decide_heal(&node, "TimeoutError"), HealAction::RetryRestrictTools);
        assert_eq!(decide_heal(&node, "random error"), HealAction::Escalate);
        node.retry_count = 3;
        assert_eq!(decide_heal(&node, "any"), HealAction::Escalate);
    }

    #[test]
    fn mutate_prompt_appends() {
        let m = mutate_prompt("do X", 1);
        assert!(m.starts_with("do X"));
        assert!(m.contains("retry hint"));
    }

    #[test]
    fn record_run_increments_retry_on_fail() {
        let mut node = n();
        record_run(
            &mut node,
            NodeRun {
                started_at: Utc::now().timestamp(),
                finished_at: Some(Utc::now().timestamp()),
                status: PlanNodeStatus::Failed,
                output: None,
                error: Some("boom".into()),
                heal_strategy: None,
            },
        );
        assert_eq!(node.retry_count, 1);
        assert_eq!(node.status, PlanNodeStatus::Failed);
    }

    #[test]
    fn record_run_done_no_increment() {
        let mut node = n();
        record_run(
            &mut node,
            NodeRun {
                started_at: Utc::now().timestamp(),
                finished_at: Some(Utc::now().timestamp()),
                status: PlanNodeStatus::Done,
                output: Some("ok".into()),
                error: None,
                heal_strategy: None,
            },
        );
        assert_eq!(node.retry_count, 0);
        assert_eq!(node.status, PlanNodeStatus::Done);
    }
}
