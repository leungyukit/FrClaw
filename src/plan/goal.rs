//! Goal ── 用户给的高层目标。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    /// 自然语言目标
    pub description: String,
    /// 上下文（cwd / 当前状态 / 已知约束）
    #[serde(default)]
    pub context: Option<String>,
    /// 期望截止时间（None = 不急）
    #[serde(default)]
    pub deadline: Option<i64>,
    /// 成功判据（可选；用来评估 plan 是否达成）
    #[serde(default)]
    pub success_criteria: Vec<String>,
}

impl Goal {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            context: None,
            deadline: None,
            success_criteria: vec![],
        }
    }
    pub fn context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }
    pub fn success(mut self, criteria: Vec<String>) -> Self {
        self.success_criteria = criteria;
        self
    }
}
