//! 内存中的对话会话。

use crate::config::models::ModelsFile;
use crate::config::settings::Settings;
use crate::llm::message::{Message, Role};
use crate::llm::prompts::default_system_prompt;
use crate::session::store;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub system_prompt: String,
    pub messages: Vec<Message>,
    /// 当前 provider alias（保存下来，下次加载时能恢复）。
    pub provider_alias: String,
}

impl ChatSession {
    pub fn new(name: impl Into<String>, _models: &ModelsFile, settings: &Settings, provider_alias: impl Into<String>) -> Self {
        let now = Utc::now();
        let system_prompt = default_system_prompt(&settings.lang);
        Self {
            name: name.into(),
            created_at: now,
            updated_at: now,
            system_prompt: system_prompt.clone(),
            messages: vec![Message::system(system_prompt)],
            provider_alias: provider_alias.into(),
        }
    }

    pub fn push_user(&mut self, content: impl Into<String>) {
        self.messages.push(Message::user(content));
        self.touch();
    }

    pub fn push_assistant(&mut self, content: impl Into<String>) {
        self.messages.push(Message::assistant(content));
        self.touch();
    }

    pub fn push_assistant_with_tools(&mut self, content: impl Into<String>, calls: Vec<crate::llm::message::ToolCall>) {
        let content = content.into();
        let mut m = Message::assistant(content.clone());
        m.tool_calls = calls;
        self.messages.push(m);
        self.touch();
    }

    pub fn push_tool_result(&mut self, tool_call_id: &str, name: &str, result: serde_json::Value) {
        let result_str = result.to_string();
        self.messages
            .push(Message::tool(tool_call_id, name, result_str));
        self.touch();
    }

    fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    /// 截取给 LLM 用的 messages：system + 最近 `history_window` * 2 条对话
    pub fn truncated_messages(&self, history_window: usize) -> Vec<Message> {
        let mut out: Vec<Message> = Vec::new();
        if let Some(sys) = self.messages.first() {
            out.push(sys.clone());
        }
        let rest = &self.messages[1..];
        let budget = history_window * 2;
        if rest.len() <= budget {
            out.extend(rest.iter().cloned());
        } else {
            let start = rest.len().saturating_sub(budget);
            out.extend(rest[start..].iter().cloned());
        }
        out
    }

    /// 存盘到 `~/.fr_cli/sessions/<name>.json`。
    pub fn save(&self) -> Result<()> {
        let path = store::session_path(&self.name)?;
        store::write_session(&path, self)
    }

    #[allow(unused_variables)]
    pub fn load(name: &str, models: &ModelsFile, settings: &Settings) -> Result<Self> {
        let path = store::session_path(name)?;
        if !path.exists() {
            anyhow::bail!("会话 `{}` 不存在", name);
        }
        store::read_session(&path)
    }

    pub fn list_all() -> Result<Vec<(String, DateTime<Utc>)>> {
        store::list_sessions()
    }
}

/// 列出 session 列表并格式化为简洁文本（用于 /list_sessions）。
pub fn format_session_list(sessions: &[(String, DateTime<Utc>)]) -> String {
    if sessions.is_empty() {
        return "  (无)".into();
    }
    let mut s = String::new();
    for (name, ts) in sessions {
        s.push_str(&format!(
            "  • {}  (updated {})\n",
            name,
            ts.format("%Y-%m-%d %H:%M:%S")
        ));
    }
    s
}

/// `Message::content` 渲染为单行（去掉换行）。用于 /see 命令预览。
pub fn safe_preview(content: &str, max: usize) -> String {
    let flat = content.replace('\n', " ");
    if flat.chars().count() <= max {
        flat
    } else {
        let mut s: String = flat.chars().take(max).collect();
        s.push('…');
        s
    }
}

#[allow(dead_code)]
fn _ensure_role_first_is_system(s: &ChatSession) {
    debug_assert!(matches!(
        s.messages.first().map(|m| m.role),
        Some(Role::System)
    ));
}

#[allow(dead_code)]
fn _check_path(p: &Path) -> std::io::Result<()> {
    p.metadata().map(|_| ()).map_err(Into::into)
}
