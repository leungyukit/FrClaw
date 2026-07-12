//! Hooks 输入 / 输出数据模型。
//!
//! hook 命令通过 stdin 收一个 JSON（HookInput），通过 stdout / stderr / exit code
//! 表达 HookOutput。
//!
//! 各事件的「HookInput」字段：
//!
//! - `PreToolUse` —— { tool_name, tool_args }（stdout 若输出 JSON 也作为 modified_args）
//! - `PostToolUse` —— { tool_name, tool_args, tool_result, elapsed_ms }
//! - `UserPromptSubmit` —— { prompt }（stdout 若输出 JSON 可包含 additional_context
//!               字段拼到 messages，或 modified_prompt 替换原 prompt）
//! - `SessionStart` —— { session_id, cwd, models_alias }（无 stdout 协议）

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    UserPromptSubmit,
    SessionStart,
}

impl HookEvent {
    pub fn label(self) -> &'static str {
        match self {
            HookEvent::PreToolUse => "PreToolUse",
            HookEvent::PostToolUse => "PostToolUse",
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
            HookEvent::SessionStart => "SessionStart",
        }
    }
}

/// 传给 hook 的输入 JSON。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInput {
    pub event: HookEvent,
    pub timestamp_ms: i64,
    #[serde(flatten)]
    pub payload: HookPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookPayload {
    PreToolUse {
        tool_name: String,
        tool_args: Value,
        session_name: Option<String>,
    },
    PostToolUse {
        tool_name: String,
        tool_args: Value,
        tool_result: Value,
        elapsed_ms: u128,
        session_name: Option<String>,
    },
    UserPromptSubmit {
        prompt: String,
        session_name: Option<String>,
    },
    SessionStart {
        session_name: String,
        cwd: String,
        provider_alias: String,
    },
}

impl HookInput {
    pub fn pre_tool_use(
        tool_name: impl Into<String>,
        tool_args: Value,
        session_name: Option<String>,
    ) -> Self {
        let event = HookEvent::PreToolUse;
        Self {
            event,
            timestamp_ms: now_ms(),
            payload: HookPayload::PreToolUse {
                tool_name: tool_name.into(),
                tool_args,
                session_name,
            },
        }
    }

    pub fn post_tool_use(
        tool_name: impl Into<String>,
        tool_args: Value,
        tool_result: Value,
        elapsed_ms: u128,
        session_name: Option<String>,
    ) -> Self {
        let event = HookEvent::PostToolUse;
        Self {
            event,
            timestamp_ms: now_ms(),
            payload: HookPayload::PostToolUse {
                tool_name: tool_name.into(),
                tool_args,
                tool_result,
                elapsed_ms,
                session_name,
            },
        }
    }

    pub fn user_prompt_submit(prompt: impl Into<String>, session_name: Option<String>) -> Self {
        let event = HookEvent::UserPromptSubmit;
        Self {
            event,
            timestamp_ms: now_ms(),
            payload: HookPayload::UserPromptSubmit {
                prompt: prompt.into(),
                session_name,
            },
        }
    }

    pub fn session_start(
        session_name: impl Into<String>,
        cwd: impl Into<String>,
        provider_alias: impl Into<String>,
    ) -> Self {
        let event = HookEvent::SessionStart;
        Self {
            event,
            timestamp_ms: now_ms(),
            payload: HookPayload::SessionStart {
                session_name: session_name.into(),
                cwd: cwd.into(),
                provider_alias: provider_alias.into(),
            },
        }
    }

    pub fn tool_name(&self) -> Option<&str> {
        match &self.payload {
            HookPayload::PreToolUse { tool_name, .. } => Some(tool_name),
            HookPayload::PostToolUse { tool_name, .. } => Some(tool_name),
            _ => None,
        }
    }
}

/// 钩子执行的输出。
#[derive(Debug, Clone, Default)]
pub struct HookOutput {
    pub blocked: bool,
    pub reason: Option<String>,
    pub modified_tool_name: Option<String>,
    pub modified_args: Option<Value>,
    pub additional_context: Option<String>,
    pub modified_prompt: Option<String>,
    pub stderr_tail: Option<String>,
    pub elapsed_ms: u128,
}

impl HookOutput {
    pub fn proceed() -> Self {
        Self::default()
    }
    pub fn block(reason: impl Into<String>) -> Self {
        Self {
            blocked: true,
            reason: Some(reason.into()),
            ..Self::default()
        }
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 单个 hook 命令的 stdout JSON 协议（opt-in）。
///
/// `continue_` —— 缺省 = true（hook 不表态则放行；显式 false = 阻止，等价于 exit 2）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookStdoutEnvelope {
    #[serde(default = "default_continue")]
    pub continue_: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub modified_tool_name: Option<String>,
    #[serde(default)]
    pub modified_args: Option<Value>,
    #[serde(default)]
    pub additional_context: Option<String>,
    #[serde(default)]
    pub modified_prompt: Option<String>,
    #[serde(default)]
    pub suppress_stderr: bool,
}

fn default_continue() -> bool {
    true
}

impl HookStdoutEnvelope {
    pub fn parse_from_stdout(stdout: &str) -> Self {
        let trimmed = stdout.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            return Self::default_value();
        }
        serde_json::from_str(trimmed).unwrap_or_else(|_| Self::default_value())
    }

    pub fn default_value() -> Self {
        Self {
            continue_: true,
            reason: None,
            modified_tool_name: None,
            modified_args: None,
            additional_context: None,
            modified_prompt: None,
            suppress_stderr: false,
        }
    }
}
