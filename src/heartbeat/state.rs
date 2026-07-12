//! Heartbeat 状态持久化：`~/.fr_cli/heartbeat_state.json`。
//!
//! 字段：
//! - enabled: bool
//! - last_run_at: Option<i64>
//! - next_run_at: Option<i64>
//! - last_report: Option<String>（最后一段 LLM 输出 / status）
//! - history: Vec<RunRecord>（最近 50 次运行）

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunRecord {
    pub ran_at: i64,
    pub status: String,         // "completed" / "failed" / "skipped"
    pub summary: String,
    pub tools_called: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HeartbeatState {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub last_run_at: Option<i64>,
    #[serde(default)]
    pub next_run_at: Option<i64>,
    #[serde(default)]
    pub last_report: Option<String>,
    #[serde(default)]
    pub history: Vec<RunRecord>,
}

pub fn state_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".fr_cli").join("heartbeat_state.json"))
        .unwrap_or_else(|| PathBuf::from("./heartbeat_state.json"))
}

impl HeartbeatState {
    pub fn load() -> Self {
        let p = state_path();
        if !p.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&p) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let p = state_path();
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&p, content)
            .with_context(|| format!("写 heartbeat state 失败: {}", p.display()))?;
        Ok(())
    }

    pub fn record_run(&mut self, rec: RunRecord) {
        self.last_run_at = Some(rec.ran_at);
        self.last_report = Some(rec.summary.clone());
        self.history.insert(0, rec);
        if self.history.len() > 50 {
            self.history.truncate(50);
        }
    }

    pub fn is_due(&self, _interval_minutes: u32) -> bool {
        if !self.enabled {
            return false;
        }
        let now = chrono::Utc::now().timestamp();
        match self.next_run_at {
            Some(next) => now >= next,
            None => true, // 从未跑过
        }
    }

    pub fn schedule_next(&mut self, interval_minutes: u32) {
        let now = chrono::Utc::now().timestamp();
        self.next_run_at = Some(now + (interval_minutes as i64) * 60);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled() {
        let s = HeartbeatState::default();
        assert!(!s.enabled);
    }

    #[test]
    fn is_due_never_run_returns_true_when_enabled() {
        let mut s = HeartbeatState::default();
        s.enabled = true;
        assert!(s.is_due(60));
    }

    #[test]
    fn is_due_disabled_returns_false() {
        let s = HeartbeatState::default();  // enabled=false by default
        assert!(!s.is_due(60));
    }

    #[test]
    fn schedule_next_in_future() {
        let mut s = HeartbeatState::default();
        s.enabled = true;
        s.schedule_next(60);
        assert!(!s.is_due(60)); // 60 分钟后到期
    }

    #[test]
    fn record_run_truncates_history() {
        let mut s = HeartbeatState::default();
        for i in 0..60 {
            s.record_run(RunRecord {
                ran_at: i,
                status: "ok".to_string(),
                summary: format!("run {i}"),
                tools_called: vec![],
            });
        }
        assert_eq!(s.history.len(), 50);
    }

    #[test]
    fn save_and_reload() {
        let mut s = HeartbeatState::default();
        s.enabled = true;
        s.last_report = Some("hello".to_string());
        s.save().unwrap();
        let loaded = HeartbeatState::load();
        assert!(loaded.enabled);
        assert_eq!(loaded.last_report.as_deref(), Some("hello"));
    }
}
