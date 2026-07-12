//! Task 数据模型。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    /// shell 命令
    Shell,
    /// LLM prompt
    Prompt,
    /// RAG 检索
    RagQuery,
    /// 公网搜索
    WebSearch,
}

impl TaskKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskKind::Shell => "shell",
            TaskKind::Prompt => "prompt",
            TaskKind::RagQuery => "rag_query",
            TaskKind::WebSearch => "web_search",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "shell" => Some(TaskKind::Shell),
            "prompt" => Some(TaskKind::Prompt),
            "rag_query" => Some(TaskKind::RagQuery),
            "web_search" => Some(TaskKind::WebSearch),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    /// 等待人工审批
    Pending,
    /// 已审批，等执行
    Approved,
    /// 正在执行
    Running,
    /// 成功完成
    Completed,
    /// 执行失败
    Failed,
    /// 被拒绝
    Rejected,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Approved => "approved",
            TaskStatus::Running => "running",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Rejected => "rejected",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(TaskStatus::Pending),
            "approved" => Some(TaskStatus::Approved),
            "running" => Some(TaskStatus::Running),
            "completed" => Some(TaskStatus::Completed),
            "failed" => Some(TaskStatus::Failed),
            "rejected" => Some(TaskStatus::Rejected),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub name: String,
    pub kind: TaskKind,
    /// JSON-encoded args（kind 决定 schema）
    pub args: String,
    pub status: TaskStatus,
    /// 5-field cron 表达式（None = 一次性任务）
    pub cron_expr: Option<String>,
    pub last_run_at: Option<i64>,
    pub next_run_at: Option<i64>,
    pub last_result: Option<String>,
    pub error: Option<String>,
    pub run_count: i64,
    pub created_at: i64,
    pub approved_at: Option<i64>,
    pub approved_by: Option<String>,
    /// "auto"（cron 触发，无需审批）/ "manual"（要审批）
    pub approval_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRun {
    pub id: i64,
    pub task_id: i64,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: TaskStatus,
    pub output: Option<String>,
    pub error: Option<String>,
}
