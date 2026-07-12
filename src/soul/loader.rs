//! SOUL.md 加载器。
//!
//! 多源合并顺序（高优先级覆盖低优先级的同名 `## 段`）：
//! 1. `~/.fr_cli/soul.md`（全局）
//! 2. cwd 下 `SOUL.md`（项目级）
//! 3. cwd 下 `AGENTS.md`（兼容 OpenClaw / Claude Code）

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// 全局 SOUL.md 路径。
pub fn global_soul_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".fr_cli").join("soul.md"))
        .unwrap_or_else(|| PathBuf::from("./soul.md"))
}

/// 项目级 SOUL.md 候选（cwd 下的 SOUL.md / AGENTS.md）。
pub fn project_soul_paths(cwd: &Path) -> Vec<PathBuf> {
    vec![cwd.join("SOUL.md"), cwd.join("AGENTS.md")]
}

/// 一个 SOUL.md 来源（文件 + 角色）。
#[derive(Debug, Clone)]
pub struct SoulSource {
    pub path: PathBuf,
    /// "global" / "project"
    pub kind: String,
    pub content: String,
}

/// 加载并合并所有 SOUL.md。
///
/// 返回 `SoulContent` 包含：所有 source + 合并后的纯文本（按优先级拼）。
#[derive(Debug, Clone, Default)]
pub struct SoulContent {
    pub sources: Vec<SoulSource>,
    /// 合并后的 markdown 文本（注入 system prompt）
    pub merged: String,
}

impl SoulContent {
    /// 加载全局 + cwd 下所有 SOUL.md。
    pub fn load(cwd: &Path) -> Self {
        let mut sources = Vec::new();

        // 1. 全局
        let global = global_soul_path();
        if let Ok(s) = read_safe(&global) {
            sources.push(SoulSource {
                path: global,
                kind: "global".to_string(),
                content: s,
            });
        }
        // 2. 项目
        for p in project_soul_paths(cwd) {
            if let Ok(s) = read_safe(&p) {
                sources.push(SoulSource {
                    path: p,
                    kind: "project".to_string(),
                    content: s,
                });
            }
        }

        let merged = merge_markdown(&sources);
        Self { sources, merged }
    }

    /// 加载指定路径列表（测试用）。
    pub fn load_paths<I: IntoIterator<Item = PathBuf>>(cwd: &Path, extra: I) -> Self {
        let mut sources = Vec::new();
        for p in extra {
            if let Ok(s) = read_safe(&p) {
                let kind = if p.starts_with(cwd) { "project" } else { "global" }.to_string();
                sources.push(SoulSource { path: p, kind, content: s });
            }
        }
        let merged = merge_markdown(&sources);
        Self { sources, merged }
    }

    pub fn is_empty(&self) -> bool {
        self.merged.trim().is_empty()
    }

    /// 注入到 system prompt 末尾的格式化文本。
    pub fn to_system_block(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        let mut out = String::from("\n\n# SOUL（持久身份 / 价值观 / 准则）\n");
        out.push_str(&self.merged);
        out.push('\n');
        out
    }
}

fn read_safe(p: &Path) -> Result<String> {
    std::fs::read_to_string(p).with_context(|| format!("read {}", p.display()))
}

/// 合并多个 markdown：按 `## 段` 切分；高优先级 source 的同名段覆盖低优先级。
/// 没有 `##` 段的小文件直接拼到末尾（标 `[来源]`）。
fn merge_markdown(sources: &[SoulSource]) -> String {
    if sources.is_empty() {
        return String::new();
    }
    if sources.len() == 1 {
        return sources[0].content.clone();
    }

    // 每 source 按 `## 段` 切分。第一个 `##` 之前的是「无标题」intro。
    // 高优先级 = 索引大的（晚加载的）。
    let mut section_map: std::collections::BTreeMap<String, (usize, String)> =
        std::collections::BTreeMap::new();
    let mut preambles: Vec<String> = Vec::new();

    for (priority, src) in sources.iter().enumerate().rev() {
        let sections = split_by_h2(&src.content);
        for (i, (heading, body)) in sections.iter().enumerate() {
            let key = heading.clone();
            if i == 0 && heading.is_empty() {
                // 这是无标题 intro，单独保存（避免覆盖）
                preambles.push(format!("<!-- from {} -->\n{}", src.path.display(), body.trim_end()));
            } else {
                let entry = section_map.entry(key).or_insert((priority, String::new()));
                if priority >= entry.0 {
                    entry.0 = priority;
                    entry.1 = format!("{}\n", body.trim_end());
                }
            }
        }
    }

    let mut out = String::new();
    // preambles
    if !preambles.is_empty() {
        out.push_str(&preambles.join("\n\n"));
        out.push_str("\n\n");
    }
    // sections
    for (heading, (_prio, body)) in &section_map {
        if heading.is_empty() {
            out.push_str(body);
        } else {
            out.push_str(&format!("## {}\n{}\n", heading, body.trim_end()));
        }
        out.push('\n');
    }
    out
}

fn split_by_h2(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current_heading = String::new();
    let mut current_body = String::new();
    let mut found_any_h2 = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            // 切到新段
            if found_any_h2 || !current_body.is_empty() {
                out.push((current_heading.clone(), std::mem::take(&mut current_body)));
            }
            current_heading = rest.trim().to_string();
            found_any_h2 = true;
        } else {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    out.push((current_heading, current_body));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn empty_load() {
        let s = SoulContent::default();
        assert!(s.is_empty());
        assert_eq!(s.to_system_block(), "");
    }

    #[test]
    fn load_global() {
        let dir = tempfile::Builder::new().prefix("fr-soul-1").tempdir().unwrap();
        let p = dir.path().join("soul.md");
        fs::write(&p, "You are helpful.\n").unwrap();
        let s = SoulContent::load_paths(dir.path(), vec![p]);
        assert!(!s.is_empty());
        assert!(s.merged.contains("helpful"));
    }

    #[test]
    fn merge_h2_sections_higher_priority_wins() {
        let dir = tempfile::Builder::new().prefix("fr-soul-2").tempdir().unwrap();
        let global = dir.path().join("g.md");
        let project = dir.path().join("p.md");
        fs::write(&global, "intro\n## tone\ncalm\n## values\nhonest\n").unwrap();
        fs::write(&project, "## tone\nenergetic\n").unwrap();
        let s = SoulContent::load_paths(dir.path(), vec![global, project]);
        // project 覆盖 global 的 tone
        assert!(s.merged.contains("energetic"));
        assert!(!s.merged.contains("calm"));
        // values 仍在
        assert!(s.merged.contains("honest"));
    }

    #[test]
    fn split_by_h2_basic() {
        let text = "intro\n## a\nbody a\n## b\nbody b\n";
        let sections = split_by_h2(text);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].0, "");
        assert!(sections[0].1.contains("intro"));
        assert_eq!(sections[1].0, "a");
        assert!(sections[1].1.contains("body a"));
        assert_eq!(sections[2].0, "b");
    }

    #[test]
    fn to_system_block_format() {
        let mut s = SoulContent::default();
        s.merged = "You are a Rust expert.".to_string();
        let block = s.to_system_block();
        assert!(block.contains("# SOUL"));
        assert!(block.contains("Rust expert"));
    }

    #[test]
    fn global_soul_path_under_home() {
        let p = global_soul_path();
        assert!(p.to_string_lossy().contains(".fr_cli"));
    }
}
