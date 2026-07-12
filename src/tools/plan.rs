//! Plan mode 状态机（`enter_plan_mode` / `exit_plan_mode`）。
//!
//! 当 LLM 调 `enter_plan_mode` 时，agent 暂停，把计划打印给用户；
//! 用户敲 y 审批进入 executing，敲 n 拒绝（让 LLM 调整方案）。
//! `exit_plan_mode(approved=true)` 后，plan 取消，循环继续。

use crate::tools;
use crate::tools::registry::dispatch as dispatch_tool;
use crate::ui::colors;
use anyhow::Result;
use serde_json::{json, Value};

#[derive(Default, Debug, Clone)]
pub struct PlanModeState {
    pub active: bool,
    pub steps: Vec<String>,
}

impl PlanModeState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Plan mode 工具的 dispatch —— 由 agent loop 调用。
///
/// `enter_plan_mode({steps: [...]})`：让用户确认计划
/// `exit_plan_mode()`：释放，agent 继续出最终答复
pub async fn handle_plan_tool(
    state: &mut PlanModeState,
    name: &str,
    args: &Value,
) -> Result<Value> {
    match name {
        "enter_plan_mode" => {
            let steps_v = args
                .get("steps")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let steps: Vec<String> = steps_v
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            state.active = true;
            state.steps = steps;
            render_plan(&state.steps, "📋 AI 进入计划模式");
            let approval = ask_approval();
            Ok(json!({
                "ok": approval,
                "mode": if approval { "approved_executing" } else { "rejected" },
                "user_feedback_prompt": if !approval { "用户未通过计划 — 请调整" } else { "" },
            }))
        }
        "exit_plan_mode" => {
            let approved = args
                .get("approved")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            state.active = false;
            let mode = if approved { "exited" } else { "cancelled" };
            Ok(json!({ "ok": true, "mode": mode }))
        }
        other => {
            // 不是 plan tool —— fall through 外面再走普通工具路径
            Ok(dispatch_tool(other, args).await?)
        }
    }
}

pub fn render_plan(steps: &[String], title: &str) {
    println!();
    colors::print_info(title);
    println!();
    if steps.is_empty() {
        println!("  (LLM 没有提供步骤描述)");
        return;
    }
    for (i, s) in steps.iter().enumerate() {
        println!("  {}. {}", i + 1, s);
    }
    println!();
}

pub fn ask_approval() -> bool {
    use std::io::IsTerminal;
    use std::io::Write as _;
    eprint!("  审批这个计划？(y=执行 / n=调整 / a=调整并附加理由): ");
    let _ = std::io::stderr().flush();
    if !std::io::stdin().is_terminal() {
        eprintln!("(非交互终端，默认拒绝)");
        return false;
    }
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    )
}

/// LLM 端的工具定义（传给模型）。
pub fn plan_tool_definitions() -> Vec<crate::llm::message::ToolDefinition> {
    vec![
        crate::llm::message::ToolDefinition::from_json_schema(
            "enter_plan_mode",
            "暂停并向用户展示一份执行计划（steps 数组，每项是字符串步骤描述）",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "steps": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "本次任务将按这些步骤执行"
                    }
                },
                "required": ["steps"]
            }),
        ),
        crate::llm::message::ToolDefinition::from_json_schema(
            "exit_plan_mode",
            "退出 plan mode（approved=true 表示同意执行 / false 表示取消）",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "approved": { "type": "boolean", "default": true }
                }
            }),
        ),
    ]
}

/// 推导：当一个工具不在 builtin 里且 name 以 `enter_plan_mode` / `exit_plan_mode` 开头时是 plan tool
pub fn is_plan_tool(name: &str) -> bool {
    matches!(name, "enter_plan_mode" | "exit_plan_mode")
}

/// 包装：把 plan 工具的 definitions 与 builtin 合并后送给 LLM。
pub fn all_tool_definitions() -> Vec<crate::llm::message::ToolDefinition> {
    let mut defs = tools::registry::ToolRegistry::definitions();
    defs.extend(plan_tool_definitions());
    defs.extend(crate::tools::subagent::sub_agent_definitions());
    defs
}
