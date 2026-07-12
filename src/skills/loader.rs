//! SKILL.md 解析（YAML frontmatter + markdown body）。
//!
//! 文件 schema：
//! ```markdown
//! ---
//! name: rust-deep-analyzer
//! description: 深入分析 Rust 源码项目
//! triggers: ["分析 rust 项目", "rust code review", "@rust-analyzer"]
//! allowed-tools: [read_file, list_dir, shell]
//! max-steps: 12
//! ---
//!
//! # Rust Deep Analyzer
//!
//! ## Step 1
//! ...
//! ```
//!
//! 解析容错：
//! - 缺失 frontmatter（用 `---` 包裹）→ 整个文件当 body
//! - frontmatter 字段缺省值用 [`SkillFrontmatter::default`]
//! - YAML 解析失败 → 退化为「整个文件当 body, name=file_stem」

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillFrontmatter {
    /// skill 名（默认 = 父目录名）
    pub name: String,
    /// 一句话描述
    #[serde(default)]
    pub description: String,
    /// 触发词列表（substring / regex / `@alias` 形式）
    #[serde(default)]
    pub triggers: Vec<String>,
    /// 允许的工具名列表（白名单；空 = 全开）
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// 加载后允许的最大 agent step 数（默认 10）
    #[serde(default = "default_max_steps")]
    pub max_steps: usize,
    /// skill 来源（user / builtin / imported）
    #[serde(default = "default_source")]
    pub source: String,
    /// 可选标签
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_max_steps() -> usize {
    10
}

fn default_source() -> String {
    "user".into()
}

/// 一个加载后的 skill（含 frontmatter + body + 路径）。
#[derive(Debug, Clone)]
pub struct Skill {
    pub frontmatter: SkillFrontmatter,
    /// 纯 markdown body（不含 frontmatter）
    pub body: String,
    /// 来源路径（~/.fr_cli/skills/.../SKILL.md 或 builtin 内存路径）
    pub source_path: String,
    /// "user" / "builtin"
    pub kind: SkillSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    User,
    Builtin,
}

impl Skill {
    /// 注入到 system prompt 时使用的全文：frontmatter 摘要 + body
    pub fn to_system_block(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("# Skill: {}\n", self.frontmatter.name));
        if !self.frontmatter.description.is_empty() {
            s.push_str(&format!("> {}\n", self.frontmatter.description));
        }
        if !self.frontmatter.allowed_tools.is_empty() {
            s.push_str(&format!(
                "**允许工具**: {}\n",
                self.frontmatter.allowed_tools.join(", ")
            ));
        }
        if self.frontmatter.max_steps > 0 {
            s.push_str(&format!("**max-steps**: {}\n", self.frontmatter.max_steps));
        }
        s.push_str("\n");
        s.push_str(&self.body);
        s
    }

    /// 解析 frontmatter + body 拼出的「可用工具名」列表
    pub fn allowed_tool_set(&self) -> Option<Vec<String>> {
        if self.frontmatter.allowed_tools.is_empty() {
            None
        } else {
            Some(self.frontmatter.allowed_tools.clone())
        }
    }
}

/// 解析 SKILL.md 文件内容。
pub fn parse_skill_md(content: &str, source_path: String, kind: SkillSource) -> Result<Skill> {
    // 找 frontmatter：开头是 `\n---\n` 之后到下一个 `\n---\n` 或 `---\n`
    let trimmed = content.trim_start_matches('\u{feff}'); // 去除 BOM
    if let Some(rest) = strip_frontmatter(trimmed) {
        let (yaml, body) = rest;
        let mut fm: SkillFrontmatter = serde_yaml::from_str(yaml)
            .map_err(|e| anyhow!("SKILL.md frontmatter YAML 解析失败: {e}"))?;
        if fm.name.is_empty() {
            // 从 source_path 父目录名取
            if let Some(parent) = Path::new(&source_path).parent() {
                if let Some(name) = parent.file_name().and_then(|s| s.to_str()) {
                    fm.name = name.to_string();
                }
            }
        }
        if fm.name.is_empty() {
            return Err(anyhow!("SKILL.md 必须有 name 字段或位于带名的目录"));
        }
        Ok(Skill {
            frontmatter: fm,
            body: body.to_string(),
            source_path,
            kind,
        })
    } else {
        // 没有 frontmatter —— 整个文件当 body，name 用父目录名
        let mut fm = SkillFrontmatter::default();
        fm.source = match kind {
            SkillSource::User => "user".into(),
            SkillSource::Builtin => "builtin".into(),
        };
        if let Some(parent) = Path::new(&source_path).parent() {
            if let Some(name) = parent.file_name().and_then(|s| s.to_str()) {
                fm.name = name.to_string();
            }
        }
        if fm.name.is_empty() {
            return Err(anyhow!("无 frontmatter 的 SKILL.md 必须位于带名的目录"));
        }
        Ok(Skill {
            frontmatter: fm,
            body: content.to_string(),
            source_path,
            kind,
        })
    }
}

/// 解析 `---` 包裹的 YAML frontmatter。返回 (yaml, body) 或 None。
fn strip_frontmatter(content: &str) -> Option<(&str, &str)> {
    if !content.starts_with("---") {
        return None;
    }
    // 起始 `---` 后是换行
    let after_first = content[3..].strip_prefix('\n').or_else(|| content[3..].strip_prefix("\r\n"))?;
    // 找下一个 `\n---` 或 `\r\n---`（含末尾 EOF）
    let mut idx = 0;
    let bytes = after_first.as_bytes();
    while idx < bytes.len() {
        if bytes[idx] == b'\n' {
            // 后续看是否是 ---
            let rest = &after_first[idx + 1..];
            if rest.starts_with("---") {
                let yaml = &after_first[..idx];
                // body 跳过 `---` 后的换行
                let body_start = idx + 1 + 3; // 跳过 \n---
                let body = if body_start < after_first.len() {
                    let mut b = &after_first[body_start..];
                    if b.starts_with('\n') {
                        b = &b[1..];
                    } else if b.starts_with("\r\n") {
                        b = &b[2..];
                    }
                    b
                } else {
                    ""
                };
                return Some((yaml, body));
            }
        }
        idx += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_with_frontmatter() {
        let content = "---\nname: test\ndescription: desc\ntriggers: [a, b]\n---\n\n# Body\n\ntext";
        let s = parse_skill_md(content, "/x/test/SKILL.md".into(), SkillSource::User).unwrap();
        assert_eq!(s.frontmatter.name, "test");
        assert_eq!(s.frontmatter.description, "desc");
        assert_eq!(s.frontmatter.triggers, vec!["a", "b"]);
        assert!(s.body.contains("# Body"));
    }

    #[test]
    fn parse_without_frontmatter() {
        let content = "just body, no frontmatter\n";
        let s = parse_skill_md(content, "/x/myskill/SKILL.md".into(), SkillSource::User).unwrap();
        assert_eq!(s.frontmatter.name, "myskill");
        assert_eq!(s.body, content);
    }

    #[test]
    fn name_fallback_to_parent_dir() {
        let content = "---\nname: custom\n---\nbody";
        let s = parse_skill_md(content, "/x/parent_dir/SKILL.md".into(), SkillSource::User).unwrap();
        assert_eq!(s.frontmatter.name, "custom");
    }

    #[test]
    fn to_system_block_contains_metadata() {
        let s = Skill {
            frontmatter: SkillFrontmatter {
                name: "test".into(),
                description: "d".into(),
                triggers: vec![],
                allowed_tools: vec!["shell".into()],
                max_steps: 5,
                source: "user".into(),
                tags: vec![],
            },
            body: "## Step 1\ndo X".into(),
            source_path: "/x".into(),
            kind: SkillSource::User,
        };
        let block = s.to_system_block();
        assert!(block.contains("Skill: test"));
        assert!(block.contains("shell"));
        assert!(block.contains("Step 1"));
    }
}
