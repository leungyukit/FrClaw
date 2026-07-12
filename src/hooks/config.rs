//! `~/.fr_cli/hooks.json` 配置加载与 matcher 匹配。
//!
//! Schema（与 Claude Code / OpenClaw 风格兼容）：
//! ```json
//! {
//!   "PreToolUse": [
//!     {
//!       "matcher": "shell|write_file",     // tool name 的 regex（"*" 全匹配）
//!       "hooks": [
//!         {
//!           "type": "shell",
//!           "command": "~/.fr_cli/hooks/audit.sh",
//!           "timeout_ms": 5000,
//!           "env": { "MY_ENV": "value" }
//!         }
//!       ]
//!     }
//!   ],
//!   "PostToolUse": [], "UserPromptSubmit": [], "SessionStart": []
//! }
//! ```

use crate::hooks::events::HookEvent;
use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn hooks_json_path() -> PathBuf {
    crate::config::paths::data_dir()
        .map(|d| d.join("hooks.json"))
        .unwrap_or_else(|_| PathBuf::from("~/.fr_cli/hooks.json"))
}

/// 完整配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksFile {
    #[serde(default, rename = "PreToolUse")]
    pub pre_tool_use: Vec<HookEntry>,
    #[serde(default, rename = "PostToolUse")]
    pub post_tool_use: Vec<HookEntry>,
    #[serde(default, rename = "UserPromptSubmit")]
    pub user_prompt_submit: Vec<HookEntry>,
    #[serde(default, rename = "SessionStart")]
    pub session_start: Vec<HookEntry>,
}

/// 一个 event 下的某一组 hooks（共享 matcher）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEntry {
    /// 工具名 matcher（regex）；`PreToolUse` / `PostToolUse` 用。
    /// 其它事件会忽略 matcher。
    #[serde(default = "default_matcher")]
    pub matcher: String,
    #[serde(default)]
    pub hooks: Vec<HookHandler>,
}

fn default_matcher() -> String {
    "*".into()
}

/// 单个 hook 命令。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookHandler {
    /// "shell"（MVP 只实现 shell）
    #[serde(default = "default_type")]
    pub r#type: String,
    /// shell 命令字符串
    pub command: String,
    /// 超时毫秒（默认 3000）
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

fn default_type() -> String {
    "shell".into()
}

fn default_timeout() -> u64 {
    3000
}

impl HooksFile {
    /// 从文件加载；缺失返回空配置。
    pub fn load_or_default() -> Self {
        let path = hooks_json_path();
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&path) {
            Ok(text) if text.trim().is_empty() => Self::default(),
            Ok(text) => match serde_json::from_str(&text) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!(
                        "⚠️  hooks.json 解析失败: {e} — 跳过 hooks 配置"
                    );
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// 拿到一个 event 的所有 HookEntry。
    pub fn entries_for(&self, event: HookEvent) -> &[HookEntry] {
        match event {
            HookEvent::PreToolUse => &self.pre_tool_use,
            HookEvent::PostToolUse => &self.post_tool_use,
            HookEvent::UserPromptSubmit => &self.user_prompt_submit,
            HookEvent::SessionStart => &self.session_start,
        }
    }
}

/// 写回 hooks.json。
pub fn write(cfg: &HooksFile) -> Result<()> {
    let path = hooks_json_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let text = serde_json::to_string_pretty(cfg)?;
    std::fs::write(&path, text)
        .with_context(|| format!("写 hooks.json 失败: {}", path.display()))?;
    Ok(())
}

/// matcher 是否命中 `name`。
///
/// - `"*"` 全匹配
/// - 其它按 regex 编译失败时回退到「包含」子串匹配
pub fn matcher_matches(matcher: &str, name: &str) -> bool {
    if matcher == "*" {
        return true;
    }
    match Regex::new(matcher) {
        Ok(re) => re.is_match(name),
        Err(_) => name.contains(matcher),
    }
}

/// 在某个 event 上，对某个 tool_name，找到所有匹配的 HookEntry。
pub fn matching_entries<'a>(
    cfg: &'a HooksFile,
    event: HookEvent,
    tool_name: Option<&str>,
) -> Vec<&'a HookEntry> {
    let entries = cfg.entries_for(event);
    let mut out = Vec::new();
    for e in entries {
        match tool_name {
            Some(name) => {
                if matcher_matches(&e.matcher, name) {
                    out.push(e);
                }
            }
            None => {
                if e.matcher == "*" {
                    out.push(e);
                }
            }
        }
    }
    out
}

/// 首次启动时，把样板 hooks.json 写到磁盘（仅在不存在时）。
pub fn ensure_sample() -> Result<bool> {
    let path = hooks_json_path();
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let sample = HooksFile {
        pre_tool_use: vec![HookEntry {
            matcher: "shell".into(),
            hooks: vec![HookHandler {
                r#type: "shell".into(),
                command: "echo \"  [PreToolUse:shell] 你正在跑 shell\" >&2".into(),
                timeout_ms: 1500,
                env: HashMap::new(),
            }],
        }],
        post_tool_use: vec![],
        user_prompt_submit: vec![],
        session_start: vec![HookEntry {
            matcher: "*".into(),
            hooks: vec![HookHandler {
                r#type: "shell".into(),
                command: "echo \"  [SessionStart] fr-claw 已就绪\" >&2".into(),
                timeout_ms: 1500,
                env: HashMap::new(),
            }],
        }],
    };
    write(&sample)?;
    Ok(true)
}

#[allow(dead_code)]
fn _check_path(p: &Path) -> std::io::Result<()> {
    p.metadata().map(|_| ()).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_matches_anything() {
        assert!(matcher_matches("*", "shell"));
        assert!(matcher_matches("*", "anything"));
    }

    #[test]
    fn alternation_regex() {
        assert!(matcher_matches("shell|write_file", "shell"));
        assert!(matcher_matches("shell|write_file", "write_file"));
        assert!(!matcher_matches("shell|write_file", "read_file"));
    }

    #[test]
    fn invalid_regex_falls_back_to_substring() {
        // 未闭合括号 `(unclosed` 编译失败 → fallback 走 contains
        assert!(matcher_matches("(unclosed", "(unclosed-tag"));
        assert!(!matcher_matches("(unclosed", "other"));
    }
}
