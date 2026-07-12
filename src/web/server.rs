//! axum server 启动入口。

use crate::repl::context::AppContext;
use crate::web::api::{get_mcp, get_rag, get_sandbox, get_skills, get_status, post_command, ApiState};
use crate::web::auth;
use crate::web::chat::post_chat;
use crate::web::static_files::{get_index, get_style};
use anyhow::Result;
use axum::routing::{get, post};
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;

pub struct WebConfig {
    pub host: String,
    pub port: u16,
    pub no_auth: bool,
    pub token: Option<String>,
}

pub async fn run_web_server(ctx: Arc<AppContext>, cfg: WebConfig) -> Result<()> {
    let token = match cfg.token {
        Some(t) => t,
        None => auth::load_or_create_token()?,
    };
    let state = ApiState {
        ctx: ctx.clone(),
        token: token.clone(),
        auth_enabled: !cfg.no_auth,
    };

    let app = Router::new()
        .route("/", get(get_index))
        .route("/static/style.css", get(get_style))
        .route("/api/status", get(get_status))
        .route("/api/skills", get(get_skills))
        .route("/api/sandbox", get(get_sandbox))
        .route("/api/mcp", get(get_mcp))
        .route("/api/rag", get(get_rag))
        .route("/api/chat", post(post_chat))
        .route("/api/command", post(post_command))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", cfg.host, cfg.port).parse()?;
    println!();
    println!("  ╔══════════════════════════════════════════════╗");
    println!("  ║         fr-claw Web 控制台 (Round 11)      ║");
    println!("  ╚══════════════════════════════════════════════╝");
    println!();
    println!("  监听地址:  http://{addr}");
    if !cfg.no_auth {
        println!("  Token:     {token}");
        println!("             (已存到 {})", auth::token_path().display());
        println!();
        println!("  打开浏览器:");
        println!("     http://{addr}/?token={token}");
    } else {
        println!("  ⚠ 鉴权已关闭（--no-auth）");
    }
    println!();
    println!("  Ctrl-C 退出");
    println!();

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    eprintln!();
    eprintln!("  (Ctrl-C 收到，正在关闭)");
}
