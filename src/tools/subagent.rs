//! Sub-agent 委派机制：`spawn_agent` / `task_output` 工具。
//!
//! - `spawn_agent({prompt, model?})`：创建后台任务并立刻返回 task_id
//! - `task_output({task_id, block?})`：查询任务状态 / 输出
//!
//! 任务在 tokio 里跑，结果暂存在 `SubAgentRegistry`。

use crate::llm::message::ToolDefinition;
use crate::tools::registry::dispatch as dispatch_tool;
use crate::ui::colors;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone)]
enum TaskState {
    Pending,
    Running,
    Done(String),
    #[allow(dead_code)]
    Failed(String),
}

#[derive(Debug)]
#[allow(dead_code)]
struct SubTask {
    state: TaskState,
    prompt: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Default, Debug, Clone)]
pub struct SubAgentRegistry {
    inner: Arc<Mutex<HashMap<String, SubTask>>>,
}

impl SubAgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn handle(&self, name: &str, args: &Value) -> Result<Value> {
        match name {
            "spawn_agent" => self.spawn(args),
            "task_output" => self.output(args),
            _ => dispatch_tool(name, args).await,
        }
    }

    fn spawn(&self, args: &Value) -> Result<Value> {
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let task_id = format!("sub-{}", &Uuid::new_v4().to_string()[..8]);
        let prompt_owned = prompt.to_string();

        {
            let mut inner = self.inner.lock().unwrap();
            inner.insert(
                task_id.clone(),
                SubTask {
                    state: TaskState::Pending,
                    prompt: prompt_owned.clone(),
                    created_at: chrono::Utc::now(),
                },
            );
        }

        // 立刻返回 task_id；后续在 background 里模拟任务（这里 echo 一段 await）
        // MVP 不直接派真实 LLM — 给个 stub，但接口是完整的
        let inner = Arc::clone(&self.inner);
        let task_id_bg = task_id.clone();
        tokio::spawn(async move {
            {
                let mut map = inner.lock().unwrap();
                if let Some(t) = map.get_mut(&task_id_bg) {
                    t.state = TaskState::Running;
                }
            }
            // 占位：模拟执行延迟
            tokio::time::sleep(Duration::from_millis(300)).await;
            let mut map = inner.lock().unwrap();
            if let Some(t) = map.get_mut(&task_id_bg) {
                t.state = TaskState::Done(format!(
                    "(stub) Sub-agent 已收到任务:\n  {}\n\n未来 hook 此处把 prompt 派给独立 LLM session 并写回 Done。",
                    prompt_owned
                ));
            }
        });

        Ok(json!({
            "task_id": task_id,
            "status": "queued",
            "note": "Sub-agent 已在后台启动；用 task_output 查询结果"
        }))
    }

    fn output(&self, args: &Value) -> Result<Value> {
        let task_id = match args.get("task_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return Ok(json!({ "error": "missing task_id" })),
        };
        let inner = self.inner.lock().unwrap();
        let t = match inner.get(task_id) {
            Some(t) => t,
            None => return Ok(json!({ "error": format!("unknown task_id `{task_id}`") })),
        };
        let resp = match &t.state {
            TaskState::Pending => json!({
                "task_id": task_id,
                "status": "pending"
            }),
            TaskState::Running => json!({
                "task_id": task_id,
                "status": "running"
            }),
            TaskState::Done(s) => json!({
                "task_id": task_id,
                "status": "completed",
                "output": s
            }),
            TaskState::Failed(s) => json!({
                "task_id": task_id,
                "status": "failed",
                "error": s
            }),
        };
        Ok(resp)
    }
}

/// LLM 端工具定义
pub fn sub_agent_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::from_json_schema(
            "spawn_agent",
            "后台启动一个 sub-agent 跑独立任务，立刻返回 task_id",
            json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "交给 sub-agent 的任务描述" },
                    "model": { "type": "string", "description": "可选，强行覆盖子 agent 用的 provider alias" }
                },
                "required": ["prompt"]
            }),
        ),
        ToolDefinition::from_json_schema(
            "task_output",
            "查询 sub-agent 任务的当前状态或最终输出",
            json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "block": { "type": "boolean", "description": "是否阻塞等到完成", "default": false }
                },
                "required": ["task_id"]
            }),
        ),
    ]
}

/// 是不是 sub-agent 类工具？
pub fn is_subagent_tool(name: &str) -> bool {
    matches!(name, "spawn_agent" | "task_output")
}

/// 简短打印 task_output 内容（agent loop 用）
pub fn print_task_output_preview(value: &Value) {
    let status = value.get("status").and_then(|v| v.as_str()).unwrap_or("?");
    match status {
        "completed" => {
            let out = value.get("output").and_then(|v| v.as_str()).unwrap_or("");
            colors::print_info(&format!("(sub-agent 完成: {})", &out[..out.len().min(120)]));
        }
        "running" | "pending" => colors::print_info(&format!("(sub-agent `{status}`)")),
        "failed" => colors::print_error(&format!(
                "sub-agent 失败: {}",
                value.get("error").and_then(|v| v.as_str()).unwrap_or("")
            )),
        _ => {}
    }
}
