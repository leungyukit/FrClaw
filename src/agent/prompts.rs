//! Agent 思维模式对应的 prompt 模板。
//!
//! - Direct       —— 直接回答，不解释思考
//! - CoT          —— Chain of Thought，要求模型先列出思考过程再回答
//! - ToT          —— Tree of Thoughts，多分支探索后选最优
//! - ReAct        —— think / act / observe 循环（与 MasterAgent 主循环同义）
//! - Plan         —— 自动 enter_plan_mode，先出 plan 再执行

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingMode {
    Direct,
    CoT,
    ToT,
    ReAct,
    Plan,
}

impl ThinkingMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "direct" | "chat" => Some(ThinkingMode::Direct),
            "cot" | "think" => Some(ThinkingMode::CoT),
            "tot" | "tree" => Some(ThinkingMode::ToT),
            "react" | "agent" => Some(ThinkingMode::ReAct),
            "plan" => Some(ThinkingMode::Plan),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThinkingMode::Direct => "direct",
            ThinkingMode::CoT => "cot",
            ThinkingMode::ToT => "tot",
            ThinkingMode::ReAct => "react",
            ThinkingMode::Plan => "plan",
        }
    }

    pub fn suffix_directive(self) -> &'static str {
        match self {
            ThinkingMode::Direct => "",
            ThinkingMode::CoT => "\n# 思维模式：CoT（Chain-of-Thought）\n请先在内部列出推理步骤再回答；用 `### 分析` 与 `### 回答` 两段。\n",
            ThinkingMode::ToT => "\n# 思维模式：ToT（Tree-of-Thoughts）\n请在内部尝试 2-3 个候选方案，评估优劣后挑最好的输出。\n",
            ThinkingMode::ReAct => "\n# 思维模式：ReAct（Reasoning + Acting）\n回答用户问题时优先用 `shell` / `read_file` / `list_dir` 等工具获取真实数据，再基于证据给结论。\n",
            ThinkingMode::Plan => "\n# 思维模式：Plan\n如果面对的任务 ≥3 步，先调 `enter_plan_mode({steps:[...]})` 出计划，让用户审批后再 `exit_plan_mode`。\n",
        }
    }
}
