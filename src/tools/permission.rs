//! 授权层：4 阶授权 (`Y / A / F / N`)，autonomous 模式自动批准。
//!
//! - Y —— 这一次 yes
//! - A —— 这次 session 内所有的 same-tool 都允许
//! - F —— full auto（自治模式）
//! - N —— no（拒绝）
//!
//! read_file / list_dir 默认是安全的（read-only 工具，可省掉确认）。
//! write_file / shell 必须经过授权。

use anyhow::Result;
use serde_json::Value;
use std::collections::HashSet;
use std::io::{IsTerminal, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    /// 完全自主，工具调用不弹问。
    FullAuto,
    /// 询问用户决定。
    AskEachTime,
}

#[derive(Default, Debug, Clone)]
pub struct PermissionGate {
    full_auto: bool,
    /// session 内已经被一次性 / always 同意过的工具名
    approved: HashSet<String>,
}

impl PermissionGate {
    pub fn new(full_auto: bool) -> Self {
        Self {
            full_auto,
            approved: HashSet::new(),
        }
    }

    /// 拷贝（agent 用；不影响 session 原 state 的 mutation）
    pub fn clone_for_agent(&self) -> Self {
        Self {
            full_auto: self.full_auto,
            approved: self.approved.clone(),
        }
    }

    pub fn set_full_auto(&mut self, on: bool) {
        self.full_auto = on;
        if on {
            self.approved.clear();
        }
    }

    pub fn is_full_auto(&self) -> bool {
        self.full_auto
    }

    /// 判定一个工具调用是否需要征求用户同意。
    pub fn needs_confirmation(&self, name: &str) -> bool {
        if self.full_auto {
            return false;
        }
        // 只读工具直接放行
        match name {
            "read_file" | "list_dir" => return false,
            _ => {}
        }
        !self.approved.contains(name)
    }

    /// 询问一次。如果用户输入 A（always），把工具加入 approved。
    /// 返回是否被允许执行。
    pub fn confirm(&mut self, name: &str, args: &Value) -> bool {
        if !self.needs_confirmation(name) {
            return true;
        }
        let arg_preview = serde_json::to_string(args)
            .unwrap_or_default()
            .chars()
            .take(200)
            .collect::<String>();
        let arg_preview_one_line = arg_preview.replace('\n', " ");
        eprintln!();
        eprintln!(
            "  工具 `{name}` 请求授权  args: {arg_preview_one_line}{suffix}",
            suffix = if arg_preview.len() >= 200 { "…" } else { "" }
        );
        eprint!("  授权方式 (y=这一次 / a=always / f=full-auto / n=no): ");
        let _ = std::io::stderr().flush();

        if !std::io::stdin().is_terminal() {
            // 非 TTY：默认拒绝，避免数据批量跑出去
            eprintln!("(非交互终端，默认拒绝)");
            return false;
        }

        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() {
            return false;
        }
        let trimmed = line.trim();
        match trimmed {
            "y" | "Y" => true,
            "a" | "A" => {
                self.approved.insert(name.to_string());
                true
            }
            "f" | "F" => {
                self.full_auto = true;
                self.approved.clear();
                eprintln!("  ⚠️  全面自治模式已打开（session 内所有工具调用不弹问）");
                true
            }
            _ => false, // 包括 n/N/空
        }
    }
}

/// 便捷 shell 命令授权提示（仅 /shell 内部命令路径使用）。
pub fn shell_prompt(cmd: &str) -> bool {
    if std::env::var_os("FR_NO_CONFIRM").is_some() {
        return true;
    }
    eprintln!();
    eprintln!("  即将执行 shell: `{}`", cmd);
    eprint!("  y/N: ");
    let _ = std::io::stderr().flush();
    if !std::io::stdin().is_terminal() {
        eprintln!("(非交互终端，默认拒绝)");
        return false;
    }
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "a" | "yes"
    )
}

/// 把 `Result<Value>` 转为给 LLM 的 tool message 内容（带 envelope）。
pub fn result_envelope(r: Result<Value>) -> Value {
    match r {
        Ok(v) => v,
        Err(e) => serde_json::json!({ "error": format!("{e:#}") }),
    }
}
