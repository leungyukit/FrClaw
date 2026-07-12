//! fr-cli 主入口。
//!
//! 二进制名是 `fr`（见 Cargo.toml）。
//! 默认行为：进入交互式 REPL；`-c` + `-p/-f` 转 one-shot 模式。

use anyhow::Result;
use clap::Parser;
use fr_claw::cli::args::Args;
use fr_claw::cli::bootstrap;
use std::process::ExitCode;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = Args::parse();

    if std::env::var_os("NO_COLOR").is_some() {
        fr_claw::ui::colors::set_disabled(true);
    }

    let res: Result<i32> = if args.command || args.prompt.is_some() || args.file.is_some() {
        bootstrap::run_one_shot(&args).await
    } else {
        match bootstrap::run_repl(args, Default::default()).await {
            Ok(_) => Ok(0),
            Err(e) => Err(e),
        }
    };

    match res {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("fr: {e:#}");
            ExitCode::from(1)
        }
    }
}
