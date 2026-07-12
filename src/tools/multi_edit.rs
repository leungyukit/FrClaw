//! 原子多文件编辑 (`multi_edit`)。
//!
//! AI 提议 N 处编辑时，一次性应用；任一失败，全部回滚。
//!
//! 编辑 schema：
//! ```json
//! {
//!   "edits": [
//!     { "path": "/abs/a.rs", "old_text": "fn old()", "new_text": "fn new()" },
//!     { "path": "/abs/b.rs", "old_text": "x = 1",   "new_text": "x = 2" }
//!   ],
//!   "create_if_missing": false
//! }
//! ```
//!
//! 语义：
//! - 每个 `old_text` 必须在文件里**精确出现 1 次**（0 次 / N>1 次都报错）
//! - 同一文件的多个 edit 顺序应用（不允许重叠）
//! - 任何 edit 失败 → 还原**所有**已修改的文件到 pre-edit 状态
//! - 成功 → 不留 backup；失败 → 留 `.bak.fr_multi_edit` 供人工查

use crate::llm::message::ToolDefinition;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Edit {
    pub path: String,
    pub old_text: String,
    pub new_text: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EditOp {
    #[serde(default)]
    pub edits: Vec<Edit>,
    /// 旧文本找不到时，是否允许把整文件当成 old_text 覆盖（仅当文件不存在 / 0 字节）
    #[serde(default)]
    pub create_if_missing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EditResult {
    pub ok: bool,
    pub applied: usize,
    pub files_touched: usize,
    pub file_results: Vec<FileResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rolled_back: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileResult {
    pub path: String,
    pub edits_applied: usize,
    pub bytes_before: usize,
    pub bytes_after: usize,
}

const BAK_SUFFIX: &str = ".bak.fr_multi_edit";

/// 实际跑 multi_edit。
///
/// 算法（per-file snapshot + fail-fast + 全局回滚）：
/// 1. 解析所有 edit；按文件分组
/// 2. 对每个文件顺序处理：
///    a. snapshot → 写 .bak.fr_multi_edit
///    b. 应用这个文件的所有 edits
///    c. 写文件（任一失败 → break，进回滚）
/// 3. 如果任何文件失败 → 还原**所有**已 snapshot 的文件
/// 4. 全成功 → 删所有 .bak
pub fn run_multi_edit(op: &EditOp) -> EditResult {
    if op.edits.is_empty() {
        return EditResult {
            ok: false,
            applied: 0,
            files_touched: 0,
            file_results: vec![],
            error: Some("edits 数组为空".into()),
            rolled_back: vec![],
        };
    }

    // 1) 按文件分组（保持插入顺序）
    let mut groups: Vec<(String, Vec<&Edit>)> = Vec::new();
    for e in &op.edits {
        if let Some((_, list)) = groups.iter_mut().find(|(p, _)| p == &e.path) {
            list.push(e);
        } else {
            groups.push((e.path.clone(), vec![e]));
        }
    }

    let mut file_results: Vec<FileResult> = Vec::new();
    let mut snapshots: Vec<(String, Option<String>)> = Vec::new();
    let mut applied_count = 0usize;
    let mut last_error: Option<String> = None;
    let mut any_failed = false;

    for (path, edits) in &groups {
        // 2a) snapshot
        let original = match fs::read_to_string(path) {
            Ok(c) => Some(c),
            Err(_) if op.create_if_missing => None,
            Err(e) => {
                last_error = Some(format!("读 `{path}` 失败: {e}"));
                any_failed = true;
                break;
            }
        };
        let bak = path.clone() + BAK_SUFFIX;
        if let Some(ref c) = original {
            if let Err(e) = fs::write(&bak, c) {
                // 清理已写 backup
                for (p, _) in &snapshots {
                    let _ = fs::remove_file(p.clone() + BAK_SUFFIX);
                }
                return EditResult {
                    ok: false,
                    applied: 0,
                    files_touched: 0,
                    file_results,
                    error: Some(format!("写 backup `{bak}` 失败: {e}")),
                    rolled_back: vec![],
                };
            }
        }
        snapshots.push((path.clone(), original.clone()));

        // 2b) 应用这个文件的所有 edits
        let mut content = original.clone().unwrap_or_default();
        let bytes_before = content.len();
        let mut edits_applied = 0usize;
        let mut file_failed = false;
        for e in edits {
            let n = count_occurrences(&content, &e.old_text);
            if n == 0 {
                last_error = Some(format!(
                    "`{}` 找不到 `old_text`（{} 字符）",
                    e.path,
                    e.old_text.len()
                ));
                file_failed = true;
                break;
            }
            if n > 1 {
                last_error = Some(format!(
                    "`{}` 的 `old_text` 出现 {n} 次，请增加上下文消歧",
                    e.path
                ));
                file_failed = true;
                break;
            }
            content = content.replacen(&e.old_text, &e.new_text, 1);
            edits_applied += 1;
        }
        let bytes_after = content.len();

        if file_failed {
            any_failed = true;
            file_results.push(FileResult {
                path: path.clone(),
                edits_applied: 0,
                bytes_before,
                bytes_after: bytes_before,
            });
            // 本文件回滚（不写）—— 删 .bak
            let _ = fs::remove_file(&bak);
            snapshots.pop();
            break;
        }

        // 2c) 写文件
        if let Err(e) = write_atomic(Path::new(path), &content) {
            last_error = Some(format!("写 `{path}` 失败: {e}"));
            any_failed = true;
            file_results.push(FileResult {
                path: path.clone(),
                edits_applied: 0,
                bytes_before,
                bytes_after: bytes_before,
            });
            // 不删 .bak —— 留作手动恢复
            break;
        }
        applied_count += edits_applied;
        file_results.push(FileResult {
            path: path.clone(),
            edits_applied,
            bytes_before,
            bytes_after,
        });
    }

    if any_failed {
        // 全局回滚：把所有已 snapshot 的文件还原
        let mut rolled = Vec::new();
        for (path, original) in &snapshots {
            match original {
                Some(c) => {
                    if let Err(e) = write_atomic(Path::new(path), c) {
                        last_error = Some(format!(
                            "回滚 `{path}` 失败: {e}；请手动从 {}{} 恢复",
                            path, BAK_SUFFIX
                        ));
                    } else {
                        rolled.push(path.clone());
                    }
                }
                None => {
                    let _ = fs::remove_file(path);
                    rolled.push(path.clone());
                }
            }
            let _ = fs::remove_file(path.clone() + BAK_SUFFIX);
        }
        return EditResult {
            ok: false,
            applied: 0,
            files_touched: 0,
            file_results,
            error: last_error,
            rolled_back: rolled,
        };
    }

    // 4) 全成功 → 删所有 .bak
    for (path, _) in &snapshots {
        let _ = fs::remove_file(path.clone() + BAK_SUFFIX);
    }
    EditResult {
        ok: true,
        applied: applied_count,
        files_touched: groups.len(),
        file_results,
        error: None,
        rolled_back: vec![],
    }
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.matches(needle).count()
}

/// 写文件：先写临时文件再 rename，减少中途崩溃风险。
fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("fr")
    ));
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

// ─── LLM Tool definition ─────────────────────────────────────────

pub fn multi_edit_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "multi_edit",
        "原子地应用 N 处编辑到 M 个文件。任一失败则全部回滚。old_text 必须精确出现 1 次。",
        json!({
            "type": "object",
            "properties": {
                "edits": {
                    "type": "array",
                    "description": "编辑列表",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "文件绝对路径" },
                            "old_text": { "type": "string", "description": "要被替换的原文（精确 1 次）" },
                            "new_text": { "type": "string", "description": "新文本" }
                        },
                        "required": ["path", "old_text", "new_text"]
                    }
                },
                "create_if_missing": { "type": "boolean", "description": "文件不存在时是否新建" }
            },
            "required": ["edits"]
        }),
    )
}

pub fn tool_multi_edit(args: &Value) -> Result<Value> {
    let op: EditOp = serde_json::from_value(args.clone())
        .map_err(|e| anyhow!("multi_edit: 解析 args 失败: {e}"))?;
    let result = run_multi_edit(&op);
    let v = serde_json::to_value(&result)
        .map_err(|e| anyhow!("multi_edit: 序列化结果失败: {e}"))?;
    Ok(v)
}

// ─── Helper: 加载 @file.json 作为 EditOp ─────────────────────────

/// 解析 multi_edit CLI 输入：要么原始 JSON，要么 `@/path/to/file.json`。
pub fn parse_edit_op_from_args(rest: &str) -> Result<EditOp> {
    let s = rest.trim();
    if let Some(path) = s.strip_prefix('@') {
        let content = fs::read_to_string(path)
            .map_err(|e| anyhow!("读 `{path}` 失败: {e}"))?;
        let op: EditOp = serde_json::from_str(&content)
            .map_err(|e| anyhow!("解析 `{path}` JSON 失败: {e}"))?;
        Ok(op)
    } else {
        let op: EditOp = serde_json::from_str(s)
            .map_err(|e| anyhow!("multi_edit: 解析 JSON 失败: {e}\n提示: 用 `@file.json` 从文件读"))?;
        Ok(op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(p: &Path, c: &str) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, c).unwrap();
    }

    #[test]
    fn empty_edits_errors() {
        let op = EditOp { edits: vec![], create_if_missing: false };
        let r = run_multi_edit(&op);
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("空"));
    }

    #[test]
    fn single_file_multiple_edits_apply() {
        let dir = tempfile::Builder::new().prefix("fr-multi-1").tempdir().unwrap();
        let p = dir.path().join("a.rs");
        write(&p, "fn foo() {}\nfn bar() {}\n");
        let op = EditOp {
            edits: vec![
                Edit { path: p.to_string_lossy().to_string(), old_text: "fn foo() {}".into(), new_text: "fn foo() -> i32 { 0 }".into() },
                Edit { path: p.to_string_lossy().to_string(), old_text: "fn bar() {}".into(), new_text: "fn bar() -> i32 { 1 }".into() },
            ],
            create_if_missing: false,
        };
        let r = run_multi_edit(&op);
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(r.applied, 2);
        let content = fs::read_to_string(&p).unwrap();
        assert!(content.contains("fn foo() -> i32 { 0 }"));
        assert!(content.contains("fn bar() -> i32 { 1 }"));
    }

    #[test]
    fn missing_old_text_fails() {
        let dir = tempfile::Builder::new().prefix("fr-multi-2").tempdir().unwrap();
        let p = dir.path().join("a.rs");
        write(&p, "fn foo() {}\n");
        let op = EditOp {
            edits: vec![
                Edit { path: p.to_string_lossy().to_string(), old_text: "fn bar() {}".into(), new_text: "x".into() },
            ],
            create_if_missing: false,
        };
        let r = run_multi_edit(&op);
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("找不到"));
        // 文件未改
        let content = fs::read_to_string(&p).unwrap();
        assert_eq!(content, "fn foo() {}\n");
    }

    #[test]
    fn ambiguous_old_text_fails() {
        let dir = tempfile::Builder::new().prefix("fr-multi-3").tempdir().unwrap();
        let p = dir.path().join("a.rs");
        write(&p, "let x = 1; let y = 1;\n");
        let op = EditOp {
            edits: vec![
                Edit { path: p.to_string_lossy().to_string(), old_text: "1".into(), new_text: "2".into() },
            ],
            create_if_missing: false,
        };
        let r = run_multi_edit(&op);
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("出现"));
    }

    #[test]
    fn second_edit_failure_rolls_back_first() {
        let dir = tempfile::Builder::new().prefix("fr-multi-4").tempdir().unwrap();
        let a = dir.path().join("a.rs");
        let b = dir.path().join("b.rs");
        write(&a, "alpha\n");
        write(&b, "beta\n");
        let op = EditOp {
            edits: vec![
                Edit { path: a.to_string_lossy().to_string(), old_text: "alpha".into(), new_text: "ALPHA".into() },
                Edit { path: b.to_string_lossy().to_string(), old_text: "gamma".into(), new_text: "BETA".into() },
            ],
            create_if_missing: false,
        };
        let r = run_multi_edit(&op);
        assert!(!r.ok);
        // a 应该回滚到 alpha
        assert_eq!(fs::read_to_string(&a).unwrap(), "alpha\n");
        assert_eq!(fs::read_to_string(&b).unwrap(), "beta\n");
        // 至少 a 出现在 rolled_back
        assert!(r.rolled_back.iter().any(|p| p.contains("a.rs")));
    }

    #[test]
    fn multi_file_apply() {
        let dir = tempfile::Builder::new().prefix("fr-multi-5").tempdir().unwrap();
        let a = dir.path().join("a.rs");
        let b = dir.path().join("sub/b.rs");
        write(&a, "A1\n");
        write(&b, "B1\n");
        let op = EditOp {
            edits: vec![
                Edit { path: a.to_string_lossy().to_string(), old_text: "A1".into(), new_text: "A2".into() },
                Edit { path: b.to_string_lossy().to_string(), old_text: "B1".into(), new_text: "B2".into() },
            ],
            create_if_missing: false,
        };
        let r = run_multi_edit(&op);
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(r.files_touched, 2);
        assert_eq!(fs::read_to_string(&a).unwrap(), "A2\n");
        assert_eq!(fs::read_to_string(&b).unwrap(), "B2\n");
    }

    #[test]
    fn parse_edit_op_from_inline_json() {
        let s = r#"{"edits":[{"path":"/x","old_text":"a","new_text":"b"}]}"#;
        let op = parse_edit_op_from_args(s).unwrap();
        assert_eq!(op.edits.len(), 1);
    }

    #[test]
    fn parse_edit_op_from_at_file() {
        let dir = tempfile::Builder::new().prefix("fr-multi-6").tempdir().unwrap();
        let f = dir.path().join("edits.json");
        fs::write(&f, r#"{"edits":[{"path":"/x","old_text":"a","new_text":"b"}]}"#).unwrap();
        let s = format!("@{}", f.to_string_lossy());
        let op = parse_edit_op_from_args(&s).unwrap();
        assert_eq!(op.edits.len(), 1);
    }

    #[test]
    fn backup_cleaned_on_success() {
        let dir = tempfile::Builder::new().prefix("fr-multi-7").tempdir().unwrap();
        let p = dir.path().join("a.rs");
        write(&p, "x\n");
        let op = EditOp {
            edits: vec![
                Edit { path: p.to_string_lossy().to_string(), old_text: "x".into(), new_text: "y".into() },
            ],
            create_if_missing: false,
        };
        let r = run_multi_edit(&op);
        assert!(r.ok);
        let bak = p.to_string_lossy().to_string() + BAK_SUFFIX;
        assert!(!std::path::Path::new(&bak).exists());
    }
}
