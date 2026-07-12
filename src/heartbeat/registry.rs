//! HeartbeatRegistry：后台 tick 协程 + AppContext 集成。

use super::runner::HeartbeatRunner;
use super::state::HeartbeatState;
use crate::repl::context::AppContext;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::interval;

#[derive(Clone)]
pub struct HeartbeatRegistry {
    pub state: Arc<Mutex<HeartbeatState>>,
}

impl HeartbeatRegistry {
    pub fn new() -> Self {
        Self { state: Arc::new(Mutex::new(HeartbeatState::load())) }
    }

    /// 启动后台 tick（每 60s 一次）。
    /// ctx 是 Arc<AppContext> 共享。
    pub fn start_background(&self, ctx: Arc<AppContext>) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();
        tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(60));
            // 第一次立即评估
            tick.tick().await;
            loop {
                tick.tick().await;
                if let Err(e) = tick_once(ctx.clone(), state.clone()).await {
                    eprintln!("  [heartbeat] tick error: {e:#}");
                }
            }
        })
    }

    /// 跑一次（手动触发）。
    pub async fn run_now(&self, ctx: Arc<AppContext>) -> anyhow::Result<super::state::RunRecord> {
        let runner = HeartbeatRunner::new(ctx.clone());
        let rec = runner.run_once().await?;
        self.state.lock().unwrap().record_run(rec.clone());
        let _ = self.state.lock().unwrap().save();
        super::runner::write_report_to_long_term(&rec.summary);
        Ok(rec)
    }
}

impl Default for HeartbeatRegistry {
    fn default() -> Self {
        Self::new()
    }
}

async fn tick_once(
    ctx: Arc<AppContext>,
    state: Arc<Mutex<HeartbeatState>>,
) -> anyhow::Result<()> {
    // 加载最新 policy
    let policy = {
        let soul = ctx.soul.content.lock().unwrap().clone();
        super::policy::HeartbeatPolicy::from_soul(&soul)
    };
    if !policy.enabled {
        return Ok(());
    }
    // 检查 due
    {
        let s = state.lock().unwrap();
        if !s.is_due(policy.interval_minutes) {
            return Ok(());
        }
    }
    // 跑
    let runner = HeartbeatRunner::new(ctx.clone());
    let rec = runner.run_once().await?;
    let mut s = state.lock().unwrap();
    s.record_run(rec.clone());
    s.schedule_next(policy.interval_minutes);
    let _ = s.save();
    super::runner::write_report_to_long_term(&rec.summary);
    eprintln!(
        "  [heartbeat] {} — {} tools",
        rec.status,
        rec.tools_called.len()
    );
    Ok(())
}

#[allow(dead_code)]
pub fn _unused_marker() {}
