//! 全局颜色控制。
//!
//! `owo-colors` 自动遵守 `NO_COLOR` env 与 stdout TTY 状态；
//! 这里再用一个进程级开关，让运行期也能强制开关（比如管道场景）。

use owo_colors::OwoColorize;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};

static COLOR_DISABLED: AtomicBool = AtomicBool::new(false);

pub fn set_disabled(disabled: bool) {
    COLOR_DISABLED.store(disabled, Ordering::Relaxed);
}

/// 当前是否禁用颜色。
pub fn disabled() -> bool {
    if COLOR_DISABLED.load(Ordering::Relaxed) {
        return true;
    }
    if std::env::var_os("NO_COLOR").is_some() {
        return true;
    }
    !std::io::stdout().is_terminal()
}

/// 打印一段文本并按 role 染色。
pub fn print_role(role: &str, text: &str) {
    if disabled() {
        println!("[{role}] {text}");
        return;
    }
    let colored = match role {
        "user" => text.bright_blue().to_string(),
        "assistant" => text.green().to_string(),
        "system" => text.dimmed().to_string(),
        "tool" => text.yellow().to_string(),
        "error" => text.bright_red().to_string(),
        _ => text.to_string(),
    };
    println!("{colored}");
}

/// 打印 AI 助手回复（绿色）。
pub fn print_assistant(text: &str) {
    if disabled() {
        println!("{text}");
        return;
    }
    println!("{}", text.green());
}

/// 打印错误。
pub fn print_error(text: &str) {
    print_role("error", text);
}

/// 打印提示信息（青色）。
pub fn print_info(text: &str) {
    if disabled() {
        println!("[info] {text}");
        return;
    }
    println!("{}", text.cyan());
}
