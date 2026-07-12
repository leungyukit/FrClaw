//! Skill 注册表：发现 `~/.fr_cli/skills/*/SKILL.md` + builtin skills。
//!
//! 启动期调用 [`SkillRegistry::discover`] 一次性加载，之后不重新扫描磁盘。
//! 实时刷新用 [`SkillRegistry::reload`]。

use crate::skills::loader::{parse_skill_md, Skill, SkillSource};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// 用户 skill 目录：`~/.fr_cli/skills/`
pub fn user_skills_dir() -> PathBuf {
    crate::config::paths::data_dir()
        .map(|d| d.join("skills"))
        .unwrap_or_else(|_| PathBuf::from("~/.fr_cli/skills"))
}

/// 内置 skill 目录（开发期随二进制发布的示范 skill）。
/// 通过 `FR_CLI_BUILTIN_SKILLS_DIR` env 覆盖；默认指向源码 `assets/skills/`。
pub fn builtin_skills_dir() -> PathBuf {
    if let Ok(p) = std::env::var("FR_CLI_BUILTIN_SKILLS_DIR") {
        return PathBuf::from(p);
    }
    // 源码内置目录（CARGO_MANIFEST_DIR 是 fr-claw/Cargo.toml 的目录）
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/skills")
}

#[derive(Default, Clone)]
pub struct SkillRegistry {
    inner: Arc<Mutex<Vec<Skill>>>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前注册的所有 skill（按 name 排序后的快照）。
    pub fn all(&self) -> Vec<Skill> {
        let mut v = self.inner.lock().unwrap().clone();
        v.sort_by(|a, b| a.frontmatter.name.cmp(&b.frontmatter.name));
        v
    }

    pub fn get(&self, name: &str) -> Option<Skill> {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.frontmatter.name == name)
            .cloned()
    }

    /// 触发匹配（在 [crate::skills::matcher::match_triggers] 上面一层包装）。
    pub fn match_query(&self, query: &str) -> Vec<Skill> {
        let v = self.all();
        let hits = crate::skills::matcher::match_triggers(query, &v);
        hits.into_iter().cloned().collect()
    }

    /// 全量发现：扫 builtin + 用户目录
    pub fn discover() -> Result<Self> {
        let mut skills = Vec::new();
        // 1) builtin
        let bp = builtin_skills_dir();
        if bp.exists() {
            load_dir(&bp, SkillSource::Builtin, &mut skills);
        }
        // 2) 用户
        let up = user_skills_dir();
        if up.exists() {
            load_dir(&up, SkillSource::User, &mut skills);
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(skills)),
        })
    }

    /// hot-reload
    pub fn reload(&self) -> Result<usize> {
        let new = Self::discover()?;
        let n = new.all().len();
        *self.inner.lock().unwrap() = new.inner.lock().unwrap().clone();
        Ok(n)
    }
}

fn load_dir(dir: &std::path::Path, kind: SkillSource, out: &mut Vec<Skill>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        // 取目录名（skill 名）
        let dir_name = p
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        if dir_name.as_deref().map(|s| s.starts_with('.')).unwrap_or(true) {
            continue;
        }
        let skill_md = p.join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }
        let content = match std::fs::read_to_string(&skill_md) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("  ⚠️  读 {} 失败: {e}", skill_md.display());
                continue;
            }
        };
        match parse_skill_md(&content, skill_md.display().to_string(), kind) {
            Ok(s) => out.push(s),
            Err(e) => {
                eprintln!(
                    "  ⚠️  解析 SKILL.md 失败 ({}): {e}",
                    skill_md.display()
                );
            }
        }
    }
}
