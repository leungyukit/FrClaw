//! REPL 主循环。
//!
//! 简化版：每次循环
//! 1. 打印分隔线
//! 2. rustyline 读一行
//! 3. `/cmd` 走命令路由，其它走 LLM 对话
//! 4. 直到用户 `/exit`

use crate::repl::command::{dispatch, CmdOutcome, handle_user_input};
use crate::repl::context::AppContext;
use crate::ui::banner;
use anyhow::Result;
use rustyline::config::{Builder as RlBuilder, ColorMode, CompletionType};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::path::PathBuf;

pub struct ReplConfig {
    pub history_file: PathBuf,
}

impl Default for ReplConfig {
    fn default() -> Self {
        let mut path = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push(".fr_cli");
        let _ = std::fs::create_dir_all(&path);
        path.push("repl_history.txt");
        Self { history_file: path }
    }
}

pub async fn run(ctx: std::sync::Arc<AppContext>, cfg: ReplConfig) -> Result<()> {
    println!();
    banner::print_banner();

    {
        let chain = ctx.chain.read().unwrap();
        if let Some(alias) = chain.primary_alias() {
            let p = chain.primary().expect("alias implies provider");
            banner::print_greeting(alias, p.model());
        }
        if chain.providers().is_empty() {
            eprintln!("⚠️  当前没有可用 provider — 请先编辑 ~/.fr_cli/models.yaml 添加 provider，");
            eprintln!("    或启动时指定 --model <alias>。示例见 README.md。");
        } else {
            println!(
                "  输入 /help 看可用命令。直接输入文字 = 与 AI 对话。\n  Ctrl-D 或 /exit 退出。"
            );
        }
        println!();
    }

    let cfg_rl = RlBuilder::new()
        .auto_add_history(true)
        .color_mode(ColorMode::Enabled)
        .completion_type(CompletionType::List)
        .build();

    let mut rl = match DefaultEditor::with_config(cfg_rl) {
        Ok(rl) => rl,
        Err(e) => anyhow::bail!("初始化 rustyline 失败: {e}"),
    };
    if cfg.history_file.exists() {
        let _ = rl.load_history(&cfg.history_file);
    }

    loop {
        banner::print_separator();

        let prompt = build_prompt(&ctx);
        let line = match rl.readline(&prompt) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                println!("(Ctrl-C — 输入 /exit 退出)");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(e) => {
                eprintln!("读取错误: {e:?}");
                continue;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('/') {
            match dispatch(trimmed, &ctx).await? {
                CmdOutcome::Continue => {}
                CmdOutcome::Exit => break,
            }
        } else {
            // ⚠️ Round 4 ─ 总是进 handle_user_input（保留 hook 链触发）；
            // FR_NO_LLM 时 handle_user_input 内部跳过真实 LLM。
            if let Err(e) = handle_user_input(trimmed, &ctx).await {
                crate::ui::colors::print_error(&format!("对话失败: {e:#}"));
            }
        }

        let _ = rl.save_history(&cfg.history_file);
    }

    banner::print_bye();
    Ok(())
}

fn build_prompt(ctx: &AppContext) -> String {
    let alias = ctx.session.lock().unwrap().provider_alias.clone();
    format!("fr [{alias}]> ")
}
