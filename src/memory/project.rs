//! 项目记忆加载 —— 类似 Claude Code 的 CLAUDE.md 机制。
//!
//! 自动发现的优先级（最近一个 first-wins）：
//!   1. `.frcli.md`
//!   2. `AGENTS.md`
//!   3. `CLAUDE.md`
//!   4. `.github/AGENTS.md`
//!
//! 从 `cwd` 开始往上找直到 home dir（不再爬上 `~` 之外）。

use anyhow::Result;
use std::path::{Path, PathBuf};

const CANDIDATES: &[&str] = &[
    ".frcli.md",
    "AGENTS.md",
    "CLAUDE.md",
    ".github/AGENTS.md",
];

#[derive(Debug, Clone)]
pub struct ProjectMemory {
    /// 命中的文件路径
    pub source_path: PathBuf,
    /// 文件内容
    pub content: String,
}

/// 从 `start_dir`（一般 cwd）向上找最近的项目记忆文件。
/// 找到第一个候选就停。
pub fn discover(start_dir: &Path) -> Result<Option<ProjectMemory>> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let mut cur: Option<&Path> = Some(start_dir);
    while let Some(dir) = cur {
        if dir == home || dir == Path::new("/") {
            break;
        }
        for cand in CANDIDATES {
            let p = dir.join(cand);
            if p.exists() {
                let content = std::fs::read_to_string(&p).unwrap_or_default();
                if content.trim().is_empty() {
                    continue;
                }
                return Ok(Some(ProjectMemory {
                    source_path: p,
                    content,
                }));
            }
        }
        cur = dir.parent();
    }
    Ok(None)
}

/// 把项目记忆注入 system prompt 的尾部（换行分隔）。
pub fn inject_into_system(system: &str, mem: &ProjectMemory) -> String {
    let header = format!(
        "\n# 项目记忆（自动加载自 {}）\n以下是用户为此项目写的额外指令——务必遵守：\n\n",
        mem.source_path.display()
    );
    let mut s = String::with_capacity(system.len() + mem.content.len() + header.len());
    s.push_str(system);
    s.push_str(&header);
    s.push_str(&mem.content);
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_appends_block() {
        let sys = "you are assistant";
        let mem = ProjectMemory {
            source_path: PathBuf::from("/x/.frcli.md"),
            content: "Use Spanish for replies.".into(),
        };
        let out = inject_into_system(sys, &mem);
        assert!(out.contains("Use Spanish"));
        assert!(out.contains("项目记忆"));
    }
}
