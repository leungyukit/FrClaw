//! trigger 匹配：substring / regex / `@alias`。
//!
//! 匹配优先级：
//! 1. `@<name>` 形式 → 精确匹配 skill.name
//! 2. trigger 是 regex（用 `regex::Regex::new` 编译成功）→ regex 匹配
//! 3. 否则大小写不敏感子串匹配

use crate::skills::Skill;
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// 精确 `@alias` 匹配
    Alias,
    /// regex 匹配
    Regex,
    /// 子串匹配（大小写不敏感）
    Substring,
}

/// 命中模式判定。
pub fn classify_trigger(trigger: &str) -> MatchMode {
    if let Some(rest) = trigger.strip_prefix('@') {
        if !rest.is_empty() && !rest.contains(' ') {
            return MatchMode::Alias;
        }
    }
    if contains_regex_meta(trigger) && Regex::new(trigger).is_ok() {
        MatchMode::Regex
    } else {
        MatchMode::Substring
    }
}

/// trigger 是否包含 regex 元字符（不算空格等无害字符）。
fn contains_regex_meta(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '\\' | '.'))
}

/// 在一组 skill 里找所有触发命中的 skill。
///
/// `query` —— 用户输入文本
pub fn match_triggers<'a>(query: &str, skills: &'a [Skill]) -> Vec<&'a Skill> {
    let query_lc = query.to_lowercase();
    let query_stripped = query.trim();

    // 提取 query 中所有 `@alias` token（连续非空白、@ 开头）
    let mut query_aliases: Vec<String> = Vec::new();
    for token in query.split_whitespace() {
        if let Some(rest) = token.strip_prefix('@') {
            if !rest.is_empty() {
                query_aliases.push(rest.to_lowercase());
            }
        }
    }

    let mut hits = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // 1) alias 命中
    for skill in skills {
        for t in &skill.frontmatter.triggers {
            if let Some(alias) = t.strip_prefix('@') {
                let alias_lc = alias.to_lowercase();
                if query_aliases.iter().any(|q| q == &alias_lc) {
                    if seen.insert(skill.frontmatter.name.clone()) {
                        hits.push(skill);
                    }
                }
            }
        }
    }

    // 2) regex / substring 命中
    for skill in skills {
        if seen.contains(&skill.frontmatter.name) {
            continue;
        }
        for t in &skill.frontmatter.triggers {
            let mode = classify_trigger(t);
            let hit = match mode {
                MatchMode::Alias => false, // 已扫过
                MatchMode::Regex => Regex::new(t)
                    .ok()
                    .map(|r| r.is_match(query) || r.is_match(query_stripped))
                    .unwrap_or(false),
                MatchMode::Substring => query_lc.contains(&t.to_lowercase()),
            };
            if hit {
                seen.insert(skill.frontmatter.name.clone());
                hits.push(skill);
                break;
            }
        }
    }

    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::loader::{Skill, SkillFrontmatter, SkillSource};

    fn dummy_skill(name: &str, triggers: Vec<&str>) -> Skill {
        Skill {
            frontmatter: SkillFrontmatter {
                name: name.into(),
                description: "d".into(),
                triggers: triggers.into_iter().map(|s| s.into()).collect(),
                allowed_tools: vec![],
                max_steps: 5,
                source: "user".into(),
                tags: vec![],
            },
            body: "".into(),
            source_path: format!("/x/{name}/SKILL.md"),
            kind: SkillSource::User,
        }
    }

    #[test]
    fn classify_basic() {
        assert_eq!(classify_trigger("@foo"), MatchMode::Alias);
        assert_eq!(classify_trigger("foo|bar"), MatchMode::Regex);
        assert_eq!(classify_trigger("plain text"), MatchMode::Substring);
    }

    #[test]
    fn matches_by_alias() {
        let skills = vec![dummy_skill("rust-analyzer", vec!["@rust-analyzer", "@ru"])];
        let hits = match_triggers("@rust-analyzer please help", &skills);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].frontmatter.name, "rust-analyzer");
    }

    #[test]
    fn matches_by_substring() {
        let skills = vec![dummy_skill("reviewer", vec!["code review", "review"])];
        let hits = match_triggers("please do a code review for me", &skills);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn matches_by_regex() {
        let skills = vec![dummy_skill("weather", vec!["weather|forecast"])];
        let hits = match_triggers("what's the weather today?", &skills);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn no_false_positive() {
        let skills = vec![dummy_skill("reviewer", vec!["code review"])];
        let hits = match_triggers("hello world", &skills);
        assert_eq!(hits.len(), 0);
    }
}
