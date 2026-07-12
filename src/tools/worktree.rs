//! Git worktree 工具。
//!
//! 给 LLM 提供「在独立分支 / 目录里干活、不污染主 working tree」的能力。
//! 底层走 `git worktree` 子命令，避免引 libgit2 重依赖。
//!
//! 设计目标：
//! - 用 cwd 启动期自动探测 git root（一次）
//! - 不在当前 working tree 强制切换分支 —— 而是用 `worktree add <path> <branch>` 开新目录
//! - 失败信息透传 stderr（截断 1KB 防止爆栈）

use crate::llm::message::ToolDefinition;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

/// Worktree 上下文：持有 git root 缓存。
pub struct WorktreeContext {
    pub git_root: Arc<Mutex<Option<PathBuf>>>,
}

impl std::fmt::Debug for WorktreeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorktreeContext")
            .field("git_root", &self.git_root())
            .finish()
    }
}

impl WorktreeContext {
    pub fn new() -> Self {
        Self { git_root: Arc::new(Mutex::new(None)) }
    }

    /// 探测并缓存 git root（从 cwd 出发）。不在 git 仓库里 → 存 None。
    pub fn discover(cwd: &Path) -> Self {
        let ctx = Self::new();
        if let Some(root) = git_root(cwd) {
            *ctx.git_root.lock().unwrap() = Some(root);
        }
        ctx
    }

    pub fn git_root(&self) -> Option<PathBuf> {
        self.git_root.lock().unwrap().clone()
    }

    pub fn is_in_git(&self) -> bool {
        self.git_root().is_some()
    }

    /// 强制重新探测（`/worktree` 命令里可调）。
    pub fn refresh(&self, cwd: &Path) {
        *self.git_root.lock().unwrap() = git_root(cwd);
    }
}

/// 探测 git root：cwd 向上找 `.git` 目录。
fn git_root(start: &Path) -> Option<PathBuf> {
    let mut p = Some(start.to_path_buf());
    while let Some(cur) = p {
        if cur.join(".git").exists() {
            return Some(cur);
        }
        p = cur.parent().map(|x| x.to_path_buf());
    }
    None
}

/// 跑 git 子命令（同步）。在 worktree 目录下跑。
fn run_git(cwd: &Path, args: &[&str]) -> Result<(String, String, i32)> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("执行 git {} 失败", args.join(" ")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let code = out.status.code().unwrap_or(-1);
    Ok((stdout, stderr, code))
}

// ─── LLM Tool definitions ─────────────────────────────────────────

pub fn worktree_create_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "worktree_create",
        "在 git 仓库里创建一个新 worktree（独立目录 + 分支）。cwd 必须在 git 仓库内。",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "新 worktree 目录（相对 git root 或绝对路径）" },
                "branch": { "type": "string", "description": "新分支名（可选；不传则创建 detached HEAD）" },
                "from": { "type": "string", "description": "基于哪个分支/commit（默认 HEAD）" }
            },
            "required": ["path"]
        }),
    )
}

pub fn worktree_list_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "worktree_list",
        "列出 git 仓库里所有 worktree（包含主 worktree + 链接的）。",
        json!({ "type": "object", "properties": {} }),
    )
}

pub fn worktree_remove_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "worktree_remove",
        "删除一个 worktree。",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "要删除的 worktree 目录" },
                "force": { "type": "boolean", "description": "强删（忽略未提交改动）" }
            },
            "required": ["path"]
        }),
    )
}

pub fn worktree_status_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "worktree_status",
        "查看某个 worktree 的 git 状态（branch / clean/dirty / ahead-behind）。",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "worktree 目录；省略则用 git root" }
            }
        }),
    )
}

// ─── Dispatch impls ───────────────────────────────────────────────

pub fn tool_worktree_create(ctx: &WorktreeContext, args: &Value) -> Result<Value> {
    let root = match ctx.git_root() {
        Some(r) => r,
        None => {
            return Ok(json!({
                "error": "当前目录不在 git 仓库内（`/worktree` 看根路径）"
            }));
        }
    };
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return Ok(json!({ "error": "worktree_create: missing `path`" })),
    };
    let branch = args.get("branch").and_then(|v| v.as_str());
    let from = args.get("from").and_then(|v| v.as_str());

    // 解析 path：相对 → 相对 git root
    let target_path = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        root.join(path)
    };
    if target_path.exists() {
        return Ok(json!({
            "error": format!("路径已存在: {}", target_path.display()),
        }));
    }

    // 拼参数：worktree add [-b <branch>] [<from>] <path>
    // 或者 detached：worktree add --detach <path> [<from>]
    let mut git_args: Vec<String> = vec!["worktree".into(), "add".into()];
    if let Some(b) = branch {
        git_args.push("-b".into());
        git_args.push(b.to_string());
    } else {
        git_args.push("--detach".into());
    }
    if let Some(f) = from {
        git_args.push(f.to_string());
    }
    git_args.push(target_path.to_string_lossy().to_string());

    let args_ref: Vec<&str> = git_args.iter().map(String::as_str).collect();
    let (stdout, stderr, code) = run_git(&root, &args_ref)?;
    if code != 0 {
        return Ok(json!({
            "error": format!("git worktree add 失败 (exit {code})"),
            "stderr": truncate(&stderr, 1024),
            "stdout": truncate(&stdout, 1024),
        }));
    }
    Ok(json!({
        "ok": true,
        "path": target_path.to_string_lossy(),
        "branch": branch,
        "from": from,
        "stdout": truncate(&stdout, 512),
    }))
}

pub fn tool_worktree_list(ctx: &WorktreeContext, _args: &Value) -> Result<Value> {
    let root = match ctx.git_root() {
        Some(r) => r,
        None => {
            return Ok(json!({
                "error": "当前目录不在 git 仓库内",
                "worktrees": []
            }))
        }
    };
    let (stdout, stderr, code) = run_git(&root, &["worktree", "list", "--porcelain"])?;
    if code != 0 {
        return Ok(json!({
            "error": format!("git worktree list 失败 (exit {code})"),
            "stderr": truncate(&stderr, 1024),
        }));
    }
    // 解析 porcelain 格式
    let mut worktrees: Vec<Value> = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("worktree ") {
            if let Some((p, b)) = current.take() {
                worktrees.push(json!({ "path": p, "branch": b }));
            }
            current = Some((rest.to_string(), String::new()));
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            // 忽略；只看 branch
            let _ = rest;
        } else if let Some(rest) = line.strip_prefix("branch ") {
            if let Some(c) = current.as_mut() {
                c.1 = rest.to_string();
            }
        } else if line == "detached" {
            if let Some(c) = current.as_mut() {
                c.1 = "(detached HEAD)".to_string();
            }
        }
    }
    if let Some((p, b)) = current.take() {
        worktrees.push(json!({ "path": p, "branch": b }));
    }
    Ok(json!({
        "ok": true,
        "git_root": root.to_string_lossy(),
        "count": worktrees.len(),
        "worktrees": worktrees,
    }))
}

pub fn tool_worktree_remove(ctx: &WorktreeContext, args: &Value) -> Result<Value> {
    let root = match ctx.git_root() {
        Some(r) => r,
        None => return Ok(json!({ "error": "当前目录不在 git 仓库内" })),
    };
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return Ok(json!({ "error": "worktree_remove: missing `path`" })),
    };
    let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);

    let target = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        root.join(path)
    };

    let mut git_args: Vec<String> = vec!["worktree".into(), "remove".into()];
    if force {
        git_args.push("--force".into());
    }
    git_args.push(target.to_string_lossy().to_string());
    let args_ref: Vec<&str> = git_args.iter().map(String::as_str).collect();
    let (stdout, stderr, code) = run_git(&root, &args_ref)?;
    if code != 0 {
        return Ok(json!({
            "error": format!("git worktree remove 失败 (exit {code})"),
            "stderr": truncate(&stderr, 1024),
        }));
    }
    Ok(json!({
        "ok": true,
        "removed": target.to_string_lossy(),
        "stdout": truncate(&stdout, 512),
    }))
}

pub fn tool_worktree_status(ctx: &WorktreeContext, args: &Value) -> Result<Value> {
    let root = match ctx.git_root() {
        Some(r) => r,
        None => return Ok(json!({ "error": "当前目录不在 git 仓库内" })),
    };
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| if Path::new(s).is_absolute() {
            PathBuf::from(s)
        } else {
            root.join(s)
        })
        .unwrap_or_else(|| root.clone());

    // branch
    let (branch_out, _, branch_code) = run_git(&path, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let branch = if branch_code == 0 {
        branch_out.trim().to_string()
    } else {
        "(detached)".to_string()
    };
    // porcelain status
    let (status_out, status_err, status_code) = run_git(&path, &["status", "--porcelain"])?;
    let clean = status_code == 0 && status_out.trim().is_empty();
    let dirty_files: Vec<String> = status_out
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l[3..].to_string())
        .collect();

    Ok(json!({
        "ok": true,
        "path": path.to_string_lossy(),
        "branch": branch,
        "clean": clean,
        "dirty_files": dirty_files,
        "dirty_count": dirty_files.len(),
        "stderr": if status_code != 0 { Some(truncate(&status_err, 256)) } else { None },
    }))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut t = s[..max].to_string();
        t.push_str("... (truncated)");
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn init_temp_git_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::Builder::new()
            .prefix("fr-worktree-test")
            .tempdir()
            .unwrap();
        let root = dir.path().to_path_buf();
        Command::new("git").args(["init", "-q"]).current_dir(&root).output().unwrap();
        Command::new("git").args(["config", "user.email", "test@x"]).current_dir(&root).output().unwrap();
        Command::new("git").args(["config", "user.name", "Test"]).current_dir(&root).output().unwrap();
        fs::write(root.join("a.txt"), "hello\n").unwrap();
        Command::new("git").args(["add", "."]).current_dir(&root).output().unwrap();
        Command::new("git").args(["commit", "-m", "init", "-q"]).current_dir(&root).output().unwrap();
        (dir, root)
    }

    fn init_worktree_ctx(root: &Path) -> WorktreeContext {
        let ctx = WorktreeContext::new();
        *ctx.git_root.lock().unwrap() = Some(root.to_path_buf());
        ctx
    }

    #[test]
    fn git_root_detects_dotgit() {
        let (dir, root) = init_temp_git_repo();
        assert_eq!(git_root(&root), Some(root.clone()));
        // 子目录也能找到
        let sub = root.join("sub");
        fs::create_dir(&sub).unwrap();
        assert_eq!(git_root(&sub), Some(root));
        dir.close().unwrap();
    }

    #[test]
    fn worktree_create_and_list_and_remove() {
        let (dir, root) = init_temp_git_repo();
        let ctx = init_worktree_ctx(&root);

        // create
        let new_path = root.join("wt-feature");
        let v = tool_worktree_create(
            &ctx,
            &json!({ "path": new_path.to_string_lossy(), "branch": "feature" }),
        )
        .unwrap();
        assert_eq!(v["ok"], json!(true), "create: {v}");
        assert!(new_path.exists());

        // list
        let v = tool_worktree_list(&ctx, &json!({})).unwrap();
        let wts = v["worktrees"].as_array().unwrap();
        assert_eq!(wts.len(), 2, "list should have main + new, got: {v}");

        // status of new worktree
        let v = tool_worktree_status(
            &ctx,
            &json!({ "path": new_path.to_string_lossy() }),
        )
        .unwrap();
        assert_eq!(v["branch"], json!("feature"));
        assert_eq!(v["clean"], json!(true));

        // remove
        let v = tool_worktree_remove(
            &ctx,
            &json!({ "path": new_path.to_string_lossy() }),
        )
        .unwrap();
        assert_eq!(v["ok"], json!(true), "remove: {v}");
        assert!(!new_path.exists());

        dir.close().unwrap();
    }

    #[test]
    fn worktree_status_detects_dirty() {
        let (dir, root) = init_temp_git_repo();
        let ctx = init_worktree_ctx(&root);
        // 改主 worktree
        fs::write(root.join("a.txt"), "modified\n").unwrap();
        let v = tool_worktree_status(&ctx, &json!({})).unwrap();
        assert_eq!(v["clean"], json!(false));
        assert!(v["dirty_count"].as_u64().unwrap() >= 1);
        dir.close().unwrap();
    }

    #[test]
    fn worktree_create_outside_git_errors() {
        let ctx = WorktreeContext::new();
        let v = tool_worktree_create(&ctx, &json!({ "path": "/tmp/x" })).unwrap();
        assert!(v["error"].is_string());
    }
}
