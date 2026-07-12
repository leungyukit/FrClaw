//! SOUL 工具（LLM 可见 2 个）。
//!
//! - `read_soul()` —— 读合并后的 SOUL 内容（让 LLM 自检身份）
//! - `append_soul({text})` —— 追加一条到全局 soul.md
//!
//! 「让 LLM 自己改 soul」听起来危险，所以 append_soul 只追加、不修改。

use crate::llm::message::ToolDefinition;
use crate::soul::loader::{SoulContent, SoulSource};
use crate::soul::loader as soul_loader;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

pub struct SoulContext {
    pub content: Arc<Mutex<SoulContent>>,
    pub cwd: Arc<Mutex<std::path::PathBuf>>,
}

impl std::fmt::Debug for SoulContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SoulContext").finish()
    }
}

impl SoulContext {
    pub fn new(content: SoulContent, cwd: std::path::PathBuf) -> Self {
        Self {
            content: Arc::new(Mutex::new(content)),
            cwd: Arc::new(Mutex::new(cwd)),
        }
    }

    pub fn merged(&self) -> String {
        self.content.lock().unwrap().merged.clone()
    }

    pub fn sources(&self) -> Vec<SoulSource> {
        self.content.lock().unwrap().sources.clone()
    }

    pub fn append(&self, text: &str) -> Result<()> {
        let p = soul_loader::global_soul_path();
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)?;
        writeln!(f, "\n{}", text)?;
        // reload
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        *self.content.lock().unwrap() = SoulContent::load(&cwd);
        Ok(())
    }
}

// ─── Tool definitions ─────────────────────────────────────────────

pub fn read_soul_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "read_soul",
        "读取当前合并后的 SOUL.md 内容（persona / voice / values / 准则），用于自检身份。",
        json!({ "type": "object", "properties": {} }),
    )
}

pub fn append_soul_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "append_soul",
        "追加一条到全局 soul.md（仅追加，不修改原内容）。适合记录新学到的偏好/准则。",
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "要追加的文本" }
            },
            "required": ["text"]
        }),
    )
}

// ─── Dispatch ──────────────────────────────────────────────────────

pub fn tool_read_soul(ctx: &SoulContext, _args: &Value) -> Result<Value> {
    let s = ctx.content.lock().unwrap().clone();
    let sources: Vec<Value> = s
        .sources
        .iter()
        .map(|src| {
            json!({
                "path": src.path.to_string_lossy(),
                "kind": src.kind,
            })
        })
        .collect();
    Ok(json!({
        "sources": sources,
        "merged": s.merged,
        "empty": s.is_empty(),
    }))
}

pub fn tool_append_soul(ctx: &SoulContext, args: &Value) -> Result<Value> {
    let text = args
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("append_soul: missing `text`"))?;
    if text.trim().is_empty() {
        return Ok(json!({ "ok": false, "error": "text 不能为空" }));
    }
    let len = text.len();
    ctx.append(text)?;
    Ok(json!({
        "ok": true,
        "appended_bytes": len,
        "path": soul_loader::global_soul_path().to_string_lossy(),
    }))
}
