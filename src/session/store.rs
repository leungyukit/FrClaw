//! 会话持久化到 `~/.fr_cli/sessions/`。

use crate::config::paths;
use crate::session::chat::ChatSession;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::PathBuf;

pub fn session_path(name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    Ok(paths::sessions_dir()?.join(format!("{name}.json")))
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("会话名不能为空");
    }
    if name.len() > 64 {
        anyhow::bail!("会话名过长 (>64 chars)");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        anyhow::bail!(
            "会话名只能包含字母/数字/-/_/. （避免路径穿越）"
        );
    }
    Ok(())
}

pub fn write_session(path: &std::path::Path, s: &ChatSession) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("创建会话目录失败")?;
    }
    let text = serde_json::to_string_pretty(s)?;
    std::fs::write(path, text).context("写会话文件失败")?;
    Ok(())
}

pub fn read_session(path: &std::path::Path) -> Result<ChatSession> {
    let text = std::fs::read_to_string(path).context("读会话文件失败")?;
    let s: ChatSession = serde_json::from_str(&text).context("解析会话文件失败")?;
    Ok(s)
}

pub fn list_sessions() -> Result<Vec<(String, DateTime<Utc>)>> {
    let dir = paths::sessions_dir()?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("json") {
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if let Ok(text) = std::fs::read_to_string(&p) {
                let updated_at: Option<DateTime<Utc>> = serde_json::from_str::<ChatSession>(&text)
                    .ok()
                    .map(|s| s.updated_at);
                if let Some(ts) = updated_at {
                    out.push((name, ts));
                }
            }
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(out)
}
