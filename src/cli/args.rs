//! 命令行参数。

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "fr", version, about = "FrClaw —— 终端 AI 助手 / AI 编程伙伴 (Rust 重写)")]
pub struct Args {
    /// 直接发送一条 prompt（非交互模式）
    #[arg(short = 'p', long = "prompt", value_name = "TEXT")]
    pub prompt: Option<String>,

    /// 从文件读 prompt
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    pub file: Option<String>,

    /// 不进入 REPL，输出后立即退出
    #[arg(short = 'c', long = "command")]
    pub command: bool,

    /// 打印启动 banner
    #[arg(long = "logo")]
    pub logo: bool,

    /// 切换工作目录
    #[arg(short = 'C', long = "cwd", value_name = "DIR")]
    pub cwd: Option<String>,

    /// 强制指定 provider alias（覆盖 default）
    #[arg(long = "model", value_name = "ALIAS")]
    pub model: Option<String>,

    #[command(subcommand)]
    pub subcommand: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// 启动 Web 控制台（HTTP + Bearer Token + SSE 推送）
    Web {
        /// 监听端口（默认 8765）
        #[arg(short = 'p', long = "port", value_name = "PORT", default_value_t = 8765)]
        port: u16,
        /// 监听地址（默认 127.0.0.1）
        #[arg(long = "host", value_name = "HOST", default_value = "127.0.0.1")]
        host: String,
        /// 关闭 Bearer Token 鉴权（不推荐）
        #[arg(long = "no-auth")]
        no_auth: bool,
        /// 用已存在的 token（不重新生成）
        #[arg(long = "token", value_name = "TOKEN")]
        token: Option<String>,
    },
}
