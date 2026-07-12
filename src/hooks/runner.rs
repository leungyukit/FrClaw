//! Hook runner：执行 shell 命令 hook，处理 stdout envelope 与 exit code。
//!
//! 设计要点：
//! - **同步执行**：hooks 通常是 < 1s 的轻量命令，用 `std::process::Command` 即可。
//! - **超时**：每个 hook 配置 `timeout_ms`，超时即返回 error_envelope。
//! - **退出码语义**：
//!     - `0` —— 通过
//!     - `2` —— 阻止（PreToolUse 不让工具跑 / UserPromptSubmit 不让 prompt 进 LLM）
//!     - 其它非 0 —— 错误（stderr 反馈给 LLM）
//! - **stdout envelope**：hook 命令 stdout 若输出合法 JSON，按 envelope 字段修改 args /
//!   阻止 / 追加上下文。空 stdout 或非 JSON 视为「无修改通过」。

use crate::hooks::config::{matching_entries, HookHandler, HooksFile};
use crate::hooks::events::{HookEvent, HookInput, HookOutput, HookStdoutEnvelope};
use anyhow::Result;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 执行单个 hook（同步 + 超时）。
pub fn run_hook(
    handler: &HookHandler,
    input: &HookInput,
) -> HookOutput {
    let started = Instant::now();
    let timeout = Duration::from_millis(handler.timeout_ms.max(50));

    let input_json = match serde_json::to_string(input) {
        Ok(s) => s,
        Err(e) => {
            return HookOutput::block(format!("HookInput 序列化失败: {e}"));
        }
    };

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(&handler.command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // 注入额外 env
    for (k, v) in &handler.env {
        cmd.env(k, v);
    }
    // 给 hook 也提供一些上下文 env（避免 hook 必须读 stdin）
    cmd.env("FR_HOOK_EVENT", input.event.label())
        .env("FR_HOOK_TIMESTAMP_MS", input.timestamp_ms.to_string());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return HookOutput {
                blocked: true,
                reason: Some(format!("spawn 失败: {e}")),
                elapsed_ms: started.elapsed().as_millis(),
                ..HookOutput::default()
            };
        }
    };

    // 写 stdin
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(input_json.as_bytes()) {
            return HookOutput {
                blocked: true,
                reason: Some(format!("写 stdin 失败: {e}")),
                elapsed_ms: started.elapsed().as_millis(),
                ..HookOutput::default()
            };
        }
    }

    // 等待：把 wait timeout 单独处理（用 try_wait 轮询）
    let output = wait_with_timeout(&mut child, timeout);
    let elapsed_ms = started.elapsed().as_millis();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            let _ = child.kill();
            return HookOutput {
                blocked: true,
                reason: Some(format!("hook 超时或执行失败: {e}")),
                modified_tool_name: None,
                modified_args: None,
                additional_context: None,
                modified_prompt: None,
                stderr_tail: None,
                elapsed_ms,
            };
        }
    };
    let _ = child.kill();

    // 解析 stdout
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code();
    let envelope = HookStdoutEnvelope::parse_from_stdout(&stdout);

    let stderr_tail: Option<String> = if !stderr.trim().is_empty() && !envelope.suppress_stderr {
        Some(stderr.chars().rev().take(400).collect::<String>().chars().rev().collect())
    } else {
        None
    };

    let mut out = HookOutput {
        blocked: false,
        reason: envelope.reason.clone(),
        modified_tool_name: envelope.modified_tool_name.clone(),
        modified_args: envelope.modified_args.clone(),
        additional_context: envelope.additional_context.clone(),
        modified_prompt: envelope.modified_prompt.clone(),
        stderr_tail,
        elapsed_ms,
    };

    match exit_code {
        Some(0) => {
            if !envelope.continue_ {
                // envelope.continue_=false 也算阻止
                out.blocked = true;
                if out.reason.is_none() {
                    out.reason = Some("hook 用 continue_=false 阻止".into());
                }
            }
        }
        Some(2) => {
            out.blocked = true;
            if out.reason.is_none() {
                out.reason = Some("hook exit code 2 阻止".into());
            }
        }
        Some(code) => {
            out.blocked = true;
            out.reason = Some(format!("hook exit code {code}"));
        }
        None => {
            out.blocked = true;
            out.reason = Some("hook 异常终止（无 exit code）".into());
        }
    }

    out
}

/// 同步等待子进程，但不超过 `timeout`。
///
/// timeout 到达时 kill 并返回错误；wait_with_output 阻塞，所以这里 spawn
/// 后用 try_wait 轮询。MVP 实现：每 50ms 一次。
fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    use std::time::Duration as D;
    let step = D::from_millis(50);
    let mut elapsed = D::from_millis(0);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child
                    .stdout
                    .take()
                    .map(|mut s| {
                        use std::io::Read;
                        let mut v = Vec::new();
                        let _ = s.read_to_end(&mut v);
                        v
                    })
                    .unwrap_or_default();
                let stderr = child
                    .stderr
                    .take()
                    .map(|mut s| {
                        use std::io::Read;
                        let mut v = Vec::new();
                        let _ = s.read_to_end(&mut v);
                        v
                    })
                    .unwrap_or_default();
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if elapsed >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("timeout after {}ms", timeout.as_millis()));
                }
                std::thread::sleep(step);
                elapsed += step;
            }
            Err(e) => return Err(format!("try_wait error: {e}")),
        }
    }
}

/// 把一个 event 上的所有 hooks 都跑一遍，聚合输出：
/// - 任何一个 blocked 则最终 blocked，并把原因塞给 LLM
/// - 任何 modified_* 字段被最后一个非 None 的覆盖
/// - additional_context 全部追加（多个 hook 都想加）
pub fn dispatch_event_blocking(cfg: &HooksFile, event: HookEvent, input: &HookInput) -> HookOutput {
    let tool = input.tool_name();
    let entries = matching_entries(cfg, event, tool);
    let mut acc = HookOutput::proceed();
    acc.elapsed_ms = 0;

    for entry in entries {
        for handler in &entry.hooks {
            let out = run_hook(handler, input);
            acc.elapsed_ms += out.elapsed_ms;
            if out.blocked {
                acc.blocked = true;
                if let Some(r) = out.reason {
                    if acc.reason.is_none() {
                        acc.reason = Some(r);
                    } else {
                        let cur = acc.reason.as_deref().unwrap_or("").to_string();
                        acc.reason = Some(format!("{cur}\n{r}"));
                    }
                }
            }
            if out.modified_tool_name.is_some() {
                acc.modified_tool_name = out.modified_tool_name.clone();
            }
            if out.modified_args.is_some() {
                acc.modified_args = out.modified_args.clone();
            }
            if let Some(ctx) = &out.additional_context {
                if let Some(prev) = &acc.additional_context {
                    acc.additional_context = Some(format!("{prev}\n\n{ctx}"));
                } else {
                    acc.additional_context = Some(ctx.clone());
                }
            }
            if out.modified_prompt.is_some() {
                acc.modified_prompt = out.modified_prompt.clone();
            }
            if out.stderr_tail.is_some() && acc.stderr_tail.is_none() {
                acc.stderr_tail = out.stderr_tail.clone();
            }
            // 阻止后继续跑后续 hook 也 OK，但阻止标记会保留
        }
    }
    acc
}

/// async wrapper：当前 run_hook 是 sync（用 std::process + sleep），
/// 没必要 spawn_blocking，直接在当前线程跑。
pub async fn dispatch_event(cfg: &HooksFile, event: HookEvent, input: &HookInput) -> Result<HookOutput> {
    Ok(dispatch_event_blocking(cfg, event, input))
}

/// 给一个 hook 链加上 stdlib env（不影响 handler.env，只在 dispatcher 临时叠加）
#[allow(dead_code)]
pub fn with_env(mut env: HashMap<String, String>) -> HashMap<String, String> {
    env.entry("FR_CLI_VERSION".into())
        .or_insert(env!("CARGO_PKG_VERSION").into());
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::config::{HookEntry, HookHandler, HooksFile};
    use crate::hooks::events::HookPayload;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn empty_config_proceeds() {
        let cfg = HooksFile::default();
        let inp = HookInput::session_start("s", "/tmp", "zhipu");
        let out = dispatch_event_blocking(&cfg, HookEvent::SessionStart, &inp);
        assert!(!out.blocked);
    }

    #[test]
    fn exit_code_2_blocks() {
        let mut cfg = HooksFile::default();
        cfg.session_start.push(HookEntry {
            matcher: "*".into(),
            hooks: vec![HookHandler {
                r#type: "shell".into(),
                command: "exit 2".into(),
                timeout_ms: 1000,
                env: HashMap::new(),
            }],
        });
        let inp = HookInput::session_start("s", "/tmp", "zhipu");
        let out = dispatch_event_blocking(&cfg, HookEvent::SessionStart, &inp);
        assert!(out.blocked);
        assert!(out.reason.is_some());
    }

    #[test]
    fn stdout_envelope_modifies_args() {
        let mut cfg = HooksFile::default();
        cfg.pre_tool_use.push(HookEntry {
            matcher: "shell".into(),
            hooks: vec![HookHandler {
                r#type: "shell".into(),
                command: r#"echo '{"modified_args":{"cmd":"echo SAFE"}}'"#.into(),
                timeout_ms: 1000,
                env: HashMap::new(),
            }],
        });
        let inp = HookInput::pre_tool_use(
            "shell",
            serde_json::json!({"cmd": "rm -rf /"}),
            None,
        );
        let out = dispatch_event_blocking(&cfg, HookEvent::PreToolUse, &inp);
        assert!(!out.blocked);
        let new_args = out.modified_args.expect("args modified");
        assert_eq!(new_args["cmd"], "echo SAFE");
        // sanity check
        let _ = PathBuf::from("/tmp");
        let _ = &inp.payload;
        let _ = match inp.payload {
            HookPayload::PreToolUse { .. } => (),
            _ => panic!("payload type"),
        };
    }

    #[test]
    fn timeout_kills_blocking() {
        let mut cfg = HooksFile::default();
        cfg.pre_tool_use.push(HookEntry {
            matcher: "*".into(),
            hooks: vec![HookHandler {
                r#type: "shell".into(),
                command: "sleep 5".into(),
                timeout_ms: 200,
                env: HashMap::new(),
            }],
        });
        let inp = HookInput::pre_tool_use("shell", serde_json::json!({}), None);
        let out = dispatch_event_blocking(&cfg, HookEvent::PreToolUse, &inp);
        assert!(out.blocked);
        let reason = out.reason.clone().unwrap_or_default();
        assert!(reason.contains("超时") || reason.contains("timeout"));
    }
}
