//! 内置工具集 + ToolDefinition 元信息 + 授权执行。
//!
//! - `registry()` 列出 LLM 可见的工具描述（name/description/json_schema）
//! - `dispatch_with_confirm` 真正调用时强制走授权层（autonomous=false 时 prompt y/N）
//! - Sub-agent 委派 (`spawn_agent` / `task_output`) 与 Plan mode (`enter_plan_mode` /
//!   `exit_plan_mode`) 也作为 LLM 可见的特殊工具注册进来
//! - 自我记忆进化 (`memorize` / `recall` / `web_search`) ——
//!   对话路径 + 公网搜索路径共享长期记忆层

pub mod channels_tool;
pub mod heartbeat;
pub mod mcp_resources;
pub mod memorize;
pub mod multi_edit;
pub mod permission;
pub mod plan;
pub mod rag;
pub mod registry;
pub mod soul;
pub mod subagent;
pub mod web_search;
pub mod worktree;

pub use heartbeat::HeartbeatToolContext;

pub use permission::{PermissionGate, PermissionLevel};
pub use plan::PlanModeState;
pub use registry::{builtin_tool_definitions, ToolRegistry};
pub use subagent::SubAgentRegistry;
pub use web_search::{SearchHit, WebSearchProvider};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

pub type ToolResult = Result<Value>;

/// 不带授权的纯 dispatch —— 适合系统内路径（rule 已经同意）调用。
pub async fn dispatch(name: &str, args: &Value) -> ToolResult {
    registry::dispatch(name, args).await
}

/// 读文件。
pub fn read_file(args: &Value) -> ToolResult {
    read_file_with_sandbox(args, None)
}

/// 带沙箱检查的读文件。
pub fn read_file_with_sandbox(
    args: &Value,
    policy: Option<&crate::sandbox::policy::SandboxPolicy>,
) -> ToolResult {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("read_file: missing `path`"))?;
    let p = Path::new(path);
    if let Some(pol) = policy {
        let v = crate::sandbox::check::check_path(pol, p, crate::sandbox::check::PathOp::Read);
        if let crate::sandbox::check::SandboxVerdict::Deny { reason } = v {
            return Ok(json!({ "error": reason, "blocked_by": "sandbox" }));
        }
    }
    if !p.exists() {
        return Ok(json!({ "error": format!("file not found: {path}") }));
    }
    let content = std::fs::read_to_string(p)
        .with_context(|| format!("read_file: 读取 `{path}` 失败"))?;
    Ok(json!({
        "path": path,
        "length": content.len(),
        "content": content,
    }))
}

/// 写文件（覆盖）。
pub fn write_file(args: &Value) -> ToolResult {
    write_file_with_sandbox(args, None)
}

/// 带沙箱检查的写文件。
pub fn write_file_with_sandbox(
    args: &Value,
    policy: Option<&crate::sandbox::policy::SandboxPolicy>,
) -> ToolResult {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("write_file: missing `path`"))?;
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("write_file: missing `content`"))?;

    let p = Path::new(path);
    if let Some(pol) = policy {
        let v = crate::sandbox::check::check_path(pol, p, crate::sandbox::check::PathOp::Write);
        if let crate::sandbox::check::SandboxVerdict::Deny { reason } = v {
            return Ok(json!({ "error": reason, "blocked_by": "sandbox" }));
        }
    }

    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(p, content).with_context(|| format!("write_file: 写入 `{path}` 失败"))?;
    Ok(json!({
        "path": path,
        "wrote_bytes": content.len(),
        "ok": true,
    }))
}

pub fn list_dir(args: &Value) -> ToolResult {
    list_dir_with_sandbox(args, None)
}

pub fn list_dir_with_sandbox(
    args: &Value,
    policy: Option<&crate::sandbox::policy::SandboxPolicy>,
) -> ToolResult {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("list_dir: missing `path`"))?;
    let p = Path::new(path);
    if let Some(pol) = policy {
        let v = crate::sandbox::check::check_path(pol, p, crate::sandbox::check::PathOp::Read);
        if let crate::sandbox::check::SandboxVerdict::Deny { reason } = v {
            return Ok(json!({ "error": reason, "blocked_by": "sandbox" }));
        }
    }
    let entries = std::fs::read_dir(p)
        .with_context(|| format!("list_dir: 读取目录 `{path}` 失败"))?;
    let mut items = Vec::new();
    for entry in entries {
        let entry = entry?;
        let n = entry.file_name().to_string_lossy().to_string();
        let kind = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            "dir"
        } else {
            "file"
        };
        items.push(json!({ "name": n, "kind": kind }));
    }
    Ok(json!({ "path": path, "entries": items }))
}

/// 默认超时 30s 的 shell。
pub fn shell(args: &Value) -> ToolResult {
    shell_with_sandbox(args, None)
}

/// 带沙箱检查的 shell（macOS 可选 sandbox-exec 包裹）。
pub fn shell_with_sandbox(
    args: &Value,
    policy: Option<&crate::sandbox::policy::SandboxPolicy>,
) -> ToolResult {
    let cmd = args
        .get("cmd")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("shell: missing `cmd`"))?;
    let cwd = args.get("cwd").and_then(|v| v.as_str());

    // 沙箱黑名单检查
    if let Some(pol) = policy {
        let v = crate::sandbox::check::check_shell(pol, cmd);
        if let crate::sandbox::check::SandboxVerdict::Deny { reason } = v {
            return Ok(json!({
                "cmd": cmd,
                "error": reason,
                "blocked_by": "sandbox",
                "ok": false,
            }));
        }
    }

    // macOS sandbox-exec 增强
    #[cfg(target_os = "macos")]
    let wrapped = policy.and_then(|p| crate::sandbox::macos::wrap_command(p, cmd));
    #[cfg(not(target_os = "macos"))]
    let wrapped: Option<(String, Vec<String>)> = None;
    let wrapped_is_some = wrapped.is_some();

    let timeout_ms = policy.map(|p| p.timeout_ms).unwrap_or(30_000);
    let max_stdout = policy.map(|p| p.max_stdout_bytes).unwrap_or(1024 * 1024);

    let mut command = if let Some((program, sb_args)) = wrapped {
        let mut c = std::process::Command::new(program);
        c.args(sb_args);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    };
    if let Some(c) = cwd {
        command.current_dir(c);
    }

    let output = run_with_timeout(&mut command, timeout_ms, max_stdout);

    match output {
        Ok((stdout, stderr, code, ok)) => Ok(json!({
            "cmd": cmd,
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": code,
            "ok": ok,
            "sandbox_wrapped": wrapped_is_some,
        })),
        Err(e) => Ok(json!({
            "cmd": cmd,
            "error": format!("执行失败: {e}"),
            "ok": false,
        })),
    }
}

/// 带超时的 shell 执行。
fn run_with_timeout(
    cmd: &mut std::process::Command,
    timeout_ms: u64,
    max_stdout: usize,
) -> std::io::Result<(String, String, Option<i32>, bool)> {
    use std::io::Read;
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let mut stdout_pipe = child.stdout.take().unwrap();
    let mut stderr_pipe = child.stderr.take().unwrap();
    let (tx_out, rx_out) = mpsc::channel::<Vec<u8>>();
    let (tx_err, rx_err) = mpsc::channel::<Vec<u8>>();

    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        let _ = tx_out.send(buf);
    });
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        let _ = tx_err.send(buf);
    });

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out_bytes = rx_out.recv().unwrap_or_default();
                let mut err_bytes = rx_err.recv().unwrap_or_default();
                // 截断
                if out_bytes.len() > max_stdout {
                    out_bytes.truncate(max_stdout);
                    out_bytes.extend_from_slice(b"... (truncated)");
                }
                if err_bytes.len() > max_stdout {
                    err_bytes.truncate(max_stdout);
                    err_bytes.extend_from_slice(b"... (truncated)");
                }
                let stdout = String::from_utf8_lossy(&out_bytes).to_string();
                let stderr = String::from_utf8_lossy(&err_bytes).to_string();
                return Ok((stdout, stderr, status.code(), status.success()));
            }
            Ok(None) => {
                if start.elapsed() > Duration::from_millis(timeout_ms) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok((
                        String::new(),
                        format!("timeout: 命令超过 {}ms 被杀", timeout_ms),
                        Some(-1),
                        false,
                    ));
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(e),
        }
    }
}
