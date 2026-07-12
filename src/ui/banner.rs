//! 启动 banner 与分隔线。

use crate::ui::colors;
use owo_colors::OwoColorize;

const BANNER: &str = r"
 ╔═══════════════════════════════════════════════════════════════════╗
 ║                                                                   ║
 ║   ███████╗██████╗  ██████╗██╗      █████╗     ██╗    ██╗          ║
 ║   ██╔════╝██╔══██╗██╔════╝██║     ██╔══██╗    ██║    ██║          ║
 ║   █████╗  ██████╔╝██║     ██║     ███████║    ██║ █╗ ██║          ║
 ║   ██╔══╝  ██╔══██╗██║     ██║     ██╔══██║    ██║███╗██║          ║
 ║   ██║     ██║  ██║╚██████╗███████╗██║  ██║    ╚███╔███╔╝          ║
 ║   ╚═╝     ╚═╝  ╚═╝ ╚═════╝╚══════╝╚═╝  ╚═╝     ╚══╝╚══╝           ║
 ║                                                                   ║
 ║       终端 AI 助手  /  AI 编程伙伴  ——  对标 OpenClaw 生态        ║
 ║                                                                   ║
 ╚═══════════════════════════════════════════════════════════════════╝
";

pub fn print_banner() {
    if colors::disabled() {
        println!("{BANNER}");
    } else {
        println!("{}", BANNER.cyan());
    }
}

pub fn print_separator() {
    let sep = "─".repeat(70);
    if colors::disabled() {
        println!("\n{sep}");
    } else {
        println!("\n{}", sep.dimmed());
    }
}

pub fn print_greeting(provider: &str, model: &str) {
    let line = format!("  Model: {provider} / {model}");
    if colors::disabled() {
        println!("{line}");
    } else {
        println!("{}", line.bright_green());
    }
}

pub fn print_bye() {
    if colors::disabled() {
        println!("再见！");
    } else {
        println!("{}", "再见！".bright_yellow());
    }
}
