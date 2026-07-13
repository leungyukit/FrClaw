//! Heartbeat runner：到点跑一次 mini agent loop。
//!
//! 流程：
//! 1. 加载 SOUL 的 heartbeat 段
//! 2. 加载 HeartbeatState
//! 3. 调 LLM（system prompt 包含 SOUL + 「你是 Heartbeat 跑」+ 状态）
//! 4. 收集 tool_calls（≤ 3 步），写回 RunRecord
//! 5. 写 report 到 long-term

use super::policy::HeartbeatPolicy;
use super::state::RunRecord;
use crate::repl::context::AppContext;
use crate::soul::loader::SoulContent;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;

pub struct HeartbeatRunner {
    pub ctx: Arc<AppContext>,
}

impl HeartbeatRunner {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    /// 跑一次。返回 RunRecord。
    pub async fn run_once(&self) -> Result<RunRecord> {
        let started = chrono::Utc::now().timestamp();

        // 1. 加载 SOUL + heartbeat 段
        let soul = self.ctx.soul.content.lock().unwrap().clone();
        let policy = HeartbeatPolicy::from_soul(&soul);
        if !policy.enabled {
            return Ok(RunRecord {
                ran_at: started,
                status: "skipped".to_string(),
                summary: "policy.enabled = false".to_string(),
                tools_called: vec![],
            });
        }

        // 2. 拼 system prompt
        let sys_prompt = build_system_prompt(&policy, &soul);

        // 调工具可能用到通道，先快照一份 ChannelManager，避免 RwLockReadGuard 跨 await
        let channels_snapshot = self.ctx.channels.read().unwrap().clone();

        // 3. 调 LLM：限制 1 个 step + 限制 tool calls
        let tools = crate::tools::ToolRegistry::definitions();
        // 先把 provider clone 出来，释放 chain 读锁，避免跨 .await 持有 RwLockReadGuard
        let provider = {
            let chain = self.ctx.chain.read().unwrap();
            match chain.primary() {
                Some(p) => p.clone(),
                None => {
                    return Ok(RunRecord {
                        ran_at: started,
                        status: "failed".to_string(),
                        summary: "no primary provider".to_string(),
                        tools_called: vec![],
                    });
                }
            }
        };

        let user_msg = build_user_prompt(&policy, &self.ctx);
        let req = crate::llm::provider::CompletionRequest {
            messages: vec![
                crate::llm::message::Message::system(sys_prompt),
                crate::llm::message::Message::user(user_msg),
            ],
            tools,
            temperature: None,
            max_tokens: Some(1024),
            force_non_stream: true,
        };
        let result = tokio::time::timeout(
            Duration::from_secs(45),
            provider.chat(req),
        )
        .await;

        let (summary, tools_called) = match result {
            Ok(Ok(resp)) => {
                let mut tools_called = vec![];
                for c in &resp.tool_calls {
                    tools_called.push(c.function.name.clone());
                    // 执行 tool（只读类 + RAG）
                    let r = crate::tools::registry::dispatch_with_ctx(
                        &c.function.name,
                        &c.function.arguments,
                        Some(&self.ctx.rag),
                        Some(&self.ctx.worktree),
                        Some(&self.ctx.sandbox),
                        Some(&self.ctx.soul),
                        Some(&self.ctx.heartbeat_tools),
                        Some(&self.ctx.mcp),
                        &channels_snapshot,
                    )
                    .await;
                    if let Err(e) = r {
                        eprintln!("  [heartbeat] tool {} 失败: {e}", c.function.name);
                    }
                }
                (resp.content, tools_called)
            }
            Ok(Err(e)) => {
                return Ok(RunRecord {
                    ran_at: started,
                    status: "failed".to_string(),
                    summary: format!("LLM error: {e}"),
                    tools_called: vec![],
                });
            }
            Err(_) => {
                return Ok(RunRecord {
                    ran_at: started,
                    status: "failed".to_string(),
                    summary: "LLM timeout".to_string(),
                    tools_called: vec![],
                });
            }
        };

        Ok(RunRecord {
            ran_at: started,
            status: "completed".to_string(),
            summary,
            tools_called,
        })
    }
}

fn build_system_prompt(policy: &HeartbeatPolicy, soul: &SoulContent) -> String {
    let mut s = String::from(
        "你是 fr-claw 的 Heartbeat 跑。\n\
         按 SOUL 的 heartbeat 段定义的 directive 行动；\n\
         调合适的工具（最多 3 个），然后用 1-3 句话总结：\n\
         - 现在状态怎样？\n\
         - 你做了什么？\n\
         - 有什么需要用户知道的？\n\n",
    );
    if !policy.directives.is_empty() {
        s.push_str("## Heartbeat directives\n");
        for d in &policy.directives {
            s.push_str(&format!("- {d}\n"));
        }
        s.push('\n');
    }
    if !soul.is_empty() {
        s.push_str(&format!("\n## 完整 SOUL（persona / voice / values）\n{}\n", soul.merged));
    }
    s
}

fn build_user_prompt(policy: &HeartbeatPolicy, ctx: &AppContext) -> String {
    let now = chrono::Utc::now().timestamp();
    let last = ctx
        .heartbeat
        .state
        .lock()
        .ok()
        .and_then(|s| s.last_run_at)
        .map(|ts| {
            let dt = chrono::DateTime::from_timestamp(ts, 0)
                .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "?".to_string());
            format!("上次跑: {dt}")
        })
        .unwrap_or_else(|| "首次跑".to_string());
    let last_report = ctx
        .heartbeat
        .state
        .lock()
        .ok()
        .and_then(|s| s.last_report.clone())
        .map(|r| format!("\n\n上次报告: {r}"))
        .unwrap_or_default();
    let n_tasks = ctx.hermes.list().map(|v| v.len()).unwrap_or(0);
    format!(
        "[heartbeat tick @ interval {} min]\n现在: {}\n{}\n待办任务数: {}{}\n",
        policy.interval_minutes,
        chrono::DateTime::from_timestamp(now, 0)
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default(),
        last,
        n_tasks,
        last_report,
    )
}

pub fn write_report_to_long_term(report: &str) {
    use std::io::Write;
    let p = crate::memory::evolution::long_term_path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        let _ = writeln!(f, "\n## Heartbeat @ {ts}\n{report}\n");
    }
}
