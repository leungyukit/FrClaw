//! Skills 子模块：
//!
//! - [`loader`] —— SKILL.md 解析（YAML frontmatter + markdown body）
//! - [`registry`] —— 扫描 `~/.fr_cli/skills/*/SKILL.md` + 内置 builtin-skills
//! - [`matcher`] —— trigger 匹配（substring / regex / `@alias`）
//!
//! 全局入口 [`SkillRegistry::discover`]：
//! ```ignore
//! let reg = SkillRegistry::discover()?;
//! for skill in reg.matching_triggers("rust code review") {
//!     // 注入到 system prompt
//! }
//! ```

pub mod loader;
pub mod matcher;
pub mod registry;

pub use loader::{parse_skill_md, Skill, SkillFrontmatter};
pub use matcher::{match_triggers, MatchMode};
pub use registry::SkillRegistry;
