//! Round 14 ─ Heartbeat 工具（LLM 可见）。
//!
//! - `heartbeat_status`  —— 自检当前 policy / state / 上次报告
//! - `heartbeat_now`     —— 立即触发一次 Heartbeat 跑（异步，非阻塞）
//! - `heartbeat_set`     —— 修改 policy（开/关、间隔、directives）
//!
//! 设计：SOUL 段是「权威配置」，但允许 LLM 临时通过 `heartbeat_set` 调整 policy，
//! 下次 reload SOUL 会自动合并。

use crate::llm::message::ToolDefinition;
use crate::tools::soul::SoulContext;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// 工具集上下文：依赖 SoulContext（因为 policy 来自 SOUL）。
#[derive(Clone)]
pub struct HeartbeatToolContext {
    pub state: Arc<Mutex<crate::heartbeat::state::HeartbeatState>>,
    pub soul: Arc<SoulContext>,
    /// 当前 policy 快照（缓存，由 /heartbeat reload 同步）
    pub policy: Arc<Mutex<crate::heartbeat::policy::HeartbeatPolicy>>,
    /// 注入 main run_now 用
    pub app_ctx_factory: Arc<Mutex<Option<Arc<crate::repl::context::AppContext>>>>,
}

impl std::fmt::Debug for HeartbeatToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeartbeatToolContext")
            .field("state", &"<HeartbeatState>")
            .field("soul", &self.soul)
            .field("policy", &"<HeartbeatPolicy>")
            .field("app_ctx_factory_bound", &self.app_ctx_factory.lock().ok().map(|g| g.is_some()))
            .finish()
    }
}

impl HeartbeatToolContext {
    pub fn new(soul: Arc<SoulContext>) -> Self {
        Self {
            state: Arc::new(Mutex::new(crate::heartbeat::state::HeartbeatState::load())),
            soul,
            policy: Arc::new(Mutex::new(
                crate::heartbeat::policy::HeartbeatPolicy::default_disabled(),
            )),
            app_ctx_factory: Arc::new(Mutex::new(None)),
        }
    }

    /// 真正从磁盘读 SOUL.md 刷新 policy。
    /// cwd 由 SoulContext 决定；如果 SOUL.md 写入但 soul.content 还是老的，必须调这个。
    pub fn reload_policy(&self) {
        // 1) 先把 soul.content 从磁盘 reload
        let cwd = self.soul.cwd.lock().unwrap().clone();
        *self.soul.content.lock().unwrap() = crate::soul::SoulContent::load(&cwd);
        // 2) 重新 parse policy
        let soul = self.soul.content.lock().unwrap().clone();
        let pol = crate::heartbeat::policy::HeartbeatPolicy::from_soul(&soul);
        *self.policy.lock().unwrap() = pol;
    }
}

pub fn heartbeat_status_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "heartbeat_status",
        "查看 Heartbeat 状态：当前 policy、enabled、上次跑时间、history。",
        json!({
            "type": "object",
            "properties": {
                "verbose": { "type": "boolean", "description": "是否返回完整 history" }
            }
        }),
    )
}

pub fn heartbeat_now_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "heartbeat_now",
        "立即手动触发一次 Heartbeat 跑（agent 自检 + 调工具 + 写报告）。",
        json!({
            "type": "object",
            "properties": {}
        }),
    )
}

pub fn heartbeat_set_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "heartbeat_set",
        "调整 Heartbeat policy：enabled / interval_minutes / 追加 directive。",
        json!({
            "type": "object",
            "properties": {
                "enabled": { "type": "boolean", "description": "true/false 切换总开关" },
                "interval_minutes": { "type": "integer", "description": "间隔分钟数（≥1）" },
                "add_directive": { "type": "string", "description": "追加一条 directive" },
                "clear_directives": { "type": "boolean", "description": "清空所有 directives" }
            }
        }),
    )
}

/// 实际工具执行
pub fn tool_heartbeat_status(ctx: &HeartbeatToolContext, args: &Value) -> Result<Value> {
    let verbose = args.get("verbose").and_then(|v| v.as_bool()).unwrap_or(false);
    let pol = ctx.policy.lock().unwrap().clone();
    let st = ctx.state.lock().unwrap().clone();
    let mut out = json!({
        "enabled": st.enabled,
        "policy": {
            "enabled": pol.enabled,
            "interval_minutes": pol.interval_minutes,
            "directives_count": pol.directives.len(),
            "quiet_hours": pol.quiet_hours,
        },
        "last_run_at": st.last_run_at,
        "next_run_at": st.next_run_at,
        "last_report": st.last_report,
        "history_count": st.history.len(),
    });
    if verbose {
        let hist: Vec<Value> = st
            .history
            .iter()
            .take(20)
            .map(|r| {
                json!({
                    "ran_at": r.ran_at,
                    "status": r.status,
                    "summary": r.summary,
                    "tools_called": r.tools_called,
                })
            })
            .collect();
        out["recent_runs"] = json!(hist);
    }
    Ok(out)
}

/// heartbeat_now：把 AppContext clone 出来跑 run_now。**非阻塞**——spawn 到 tokio。
pub fn tool_heartbeat_now(ctx: &HeartbeatToolContext) -> Result<Value> {
    let app = ctx
        .app_ctx_factory
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| anyhow::anyhow!("heartbeat app context not bound (REPL only)"))?;
    let hb = app.heartbeat.clone();
    let app_for_run = app.clone();
    tokio::spawn(async move {
        match hb.run_now(app_for_run.clone()).await {
            Ok(rec) => eprintln!(
                "  [heartbeat] 手动跑完成 status={} tools={}",
                rec.status,
                rec.tools_called.len()
            ),
            Err(e) => eprintln!("  [heartbeat] 手动跑失败: {e:#}"),
        }
    });
    Ok(json!({
        "scheduled": true,
        "note": "Heartbeat 已 spawn 到后台 task；当前轮 LLM 不等它跑完",
    }))
}

pub fn tool_heartbeat_set(ctx: &HeartbeatToolContext, args: &Value) -> Result<Value> {
    let mut pol = ctx.policy.lock().unwrap();
    let mut changes = serde_json::Map::new();

    if let Some(b) = args.get("enabled").and_then(|v| v.as_bool()) {
        pol.enabled = b;
        changes.insert("enabled".into(), json!(b));
    }
    if let Some(n) = args.get("interval_minutes").and_then(|v| v.as_u64()) {
        let n = (n as u32).max(1);
        pol.interval_minutes = n;
        changes.insert("interval_minutes".into(), json!(n));
    }
    if args
        .get("clear_directives")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        pol.directives.clear();
        changes.insert("clear_directives".into(), json!(true));
    }
    if let Some(d) = args.get("add_directive").and_then(|v| v.as_str()) {
        if !d.trim().is_empty() {
            pol.directives.push(d.trim().to_string());
            changes.insert("added_directive".into(), json!(d));
        }
    }

    // 同步到 state.enabled
    {
        let mut st = ctx.state.lock().unwrap();
        st.enabled = pol.enabled;
        if pol.enabled && st.next_run_at.is_none() {
            st.schedule_next(pol.interval_minutes);
        }
        let _ = st.save();
    }

    Ok(json!({
        "ok": true,
        "applied": Value::Object(changes),
        "now": {
            "enabled": pol.enabled,
            "interval_minutes": pol.interval_minutes,
            "directives_count": pol.directives.len(),
        }
    }))
}

/// 把 SOUL 写入的 heartbeat 段也持久化回 SOUL.md
pub fn write_heartbeat_section_to_soul(soul_path: &PathBuf, pol: &crate::heartbeat::policy::HeartbeatPolicy) -> Result<()> {
    let body = format!(
        "\n## heartbeat\nenabled: {}\ninterval_minutes: {}\ndirectives:\n{}\n",
        pol.enabled,
        pol.interval_minutes,
        if pol.directives.is_empty() {
            "  (none)".to_string()
        } else {
            pol.directives
                .iter()
                .map(|d| format!("  - {d}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    );
    if !soul_path.exists() {
        std::fs::write(soul_path, format!("# SOUL — 持久身份\n{body}"))?;
        return Ok(());
    }
    let existing = std::fs::read_to_string(soul_path).unwrap_or_default();
    let updated = if existing.contains("## heartbeat") {
        replace_section(&existing, "## heartbeat", &body)
    } else {
        format!("{existing}\n{body}")
    };
    std::fs::write(soul_path, updated)?;
    Ok(())
}

fn replace_section(text: &str, heading: &str, new_body: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    let mut in_section = false;
    let mut found = false;
    for line in &lines {
        if line.trim_start().starts_with("## ") {
            let h = line.trim_start().trim_start_matches("## ").trim();
            if h.eq_ignore_ascii_case(heading.trim_start_matches("## ").trim()) {
                if !found {
                    out.push_str(new_body.trim_end());
                    out.push('\n');
                    found = true;
                }
                in_section = true;
                continue;
            }
            in_section = false;
        }
        if !in_section {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !found {
        out.push('\n');
        out.push_str(new_body);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_section_basic() {
        let s = "# SOUL\n\n## persona\ncalm\n\n## heartbeat\nold\n\n## values\nhonest\n";
        let new = "## heartbeat\ninterval_minutes: 5\n";
        let out = replace_section(s, "## heartbeat", new);
        assert!(out.contains("interval_minutes: 5"));
        assert!(!out.contains("\nold\n"));
        assert!(out.contains("## persona"));
        assert!(out.contains("## values"));
    }
}
