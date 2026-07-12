//! HeartbeatPolicy：从 SOUL 段解析。

use crate::soul::loader::SoulContent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HeartbeatPolicy {
    /// 总开关
    pub enabled: bool,
    /// 间隔（分钟）
    pub interval_minutes: u32,
    /// 单次 directive 列表（让 LLM 在 Heartbeat 时做的事）
    pub directives: Vec<String>,
    /// 提示：当前是否在工作时间（影响是否真的跑）
    pub quiet_hours: Option<String>,
}

impl HeartbeatPolicy {
    /// 从 SoulContent 解析（找 `## heartbeat` 段）。
    pub fn from_soul(soul: &SoulContent) -> Self {
        let body = extract_section(&soul.merged, "heartbeat");
        Self::parse(&body)
    }

    /// 直接 parse 文本
    pub fn parse(text: &str) -> Self {
        let mut policy = HeartbeatPolicy {
            enabled: true,
            interval_minutes: 60,
            directives: Vec::new(),
            quiet_hours: None,
        };
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("interval_minutes:") {
                if let Ok(n) = rest.trim().parse::<u32>() {
                    policy.interval_minutes = n.max(1);
                }
            } else if let Some(rest) = line.strip_prefix("enabled:") {
                if let Ok(b) = rest.trim().parse::<bool>() {
                    policy.enabled = b;
                }
            } else if let Some(rest) = line.strip_prefix("- ") {
                policy.directives.push(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("* ") {
                policy.directives.push(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("quiet_hours:") {
                policy.quiet_hours = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("directives:") {
                // "directives:" 后面跟缩进的列表项；忽略
                let _ = rest;
            }
        }
        policy
    }

    pub fn is_empty(&self) -> bool {
        self.directives.is_empty()
    }

    /// 默认 policy（用于无 SOUL heartbeat 段时）
    pub fn default_disabled() -> Self {
        Self { enabled: false, interval_minutes: 60, directives: vec![], quiet_hours: None }
    }
}

fn extract_section(text: &str, heading: &str) -> String {
    let mut in_section = false;
    let mut out = String::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("## ") {
            let h = trimmed.trim_start_matches("## ").trim();
            if in_section {
                break;
            }
            if h.eq_ignore_ascii_case(heading) {
                in_section = true;
                continue;
            }
        }
        if in_section {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let text = r#"## heartbeat
interval_minutes: 30
enabled: true
directives:
  - 检查 hermes 任务
  - 跑 /tasks list
"#;
        let p = HeartbeatPolicy::parse(text);
        assert_eq!(p.interval_minutes, 30);
        assert!(p.enabled);
        assert_eq!(p.directives.len(), 2);
        assert!(p.directives[0].contains("hermes"));
    }

    #[test]
    fn parse_minimal() {
        let text = "## heartbeat\ninterval_minutes: 60\n";
        let p = HeartbeatPolicy::parse(text);
        assert_eq!(p.interval_minutes, 60);
        assert!(p.directives.is_empty());
    }

    #[test]
    fn parse_no_section() {
        let p = HeartbeatPolicy::parse("");
        assert!(p.directives.is_empty());
        assert_eq!(p.interval_minutes, 60); // default
    }

    #[test]
    fn from_soul_extracts_heartbeat() {
        let mut soul = SoulContent::default();
        soul.merged = "intro\n## persona\ncalm\n\n## heartbeat\ninterval_minutes: 15\ndirectives:\n  - run check\n\n## values\nhonest\n".to_string();
        let p = HeartbeatPolicy::from_soul(&soul);
        assert_eq!(p.interval_minutes, 15);
        assert_eq!(p.directives.len(), 1);
        assert_eq!(p.directives[0], "run check");
    }

    #[test]
    fn default_disabled_is_off() {
        let p = HeartbeatPolicy::default_disabled();
        assert!(!p.enabled);
    }
}
