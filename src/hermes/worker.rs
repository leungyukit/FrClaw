//! Worker：执行 task + 调度。
//!
//! - `run_task`：执行单个 task 一次（同步）
//! - `tick`：扫 due tasks，触发执行
//!
//! 这层不依赖具体的执行环境（不传 AppContext）；由 registry 包一层。

use super::store::TaskStoreHandle;
use super::task::{Task, TaskKind, TaskStatus};
use crate::hermes::cron::CronExpr;
use anyhow::Result;
use serde_json::json;
use std::process::Stdio;
use tokio::io::AsyncReadExt;

pub struct Worker {
    pub store: TaskStoreHandle,
    pub timeout_secs: u64,
}

impl Worker {
    pub fn new(store: TaskStoreHandle) -> Self {
        Self { store, timeout_secs: 60 }
    }

    /// 后台 tick：找 due tasks 并触发执行。
    pub async fn tick(&self) -> Result<usize> {
        let due = self.store.due_tasks()?;
        let n = due.len();
        for task in due {
            let store = self.store.clone();
            // 用 spawn_blocking 跑 task（task 可能调 LLM / shell，要等）
            tokio::task::spawn(async move {
                if let Err(e) = run_task(store.clone(), task).await {
                    eprintln!("  [hermes worker] task run error: {e:#}");
                }
            });
        }
        Ok(n)
    }
}

/// 执行一个 task（async 版本）。
///
/// 算法：
/// 1. start_run 拿到 run_id
/// 2. 按 kind 分发：
///    - shell：spawn `sh -c <args>`，收 stdout/stderr
///    - prompt / rag_query / web_search：暂存到 output（无 LLM 上下文时不真跑）
/// 3. finish_run 记录 output + 状态
/// 4. cron 任务：根据 cron_expr 算 next_run_at
pub async fn run_task(store: TaskStoreHandle, task: Task) -> Result<()> {
    let run_id = store.start_run(task.id)?;
    let result = dispatch(&task).await;
    let (status, output, error) = match result {
        Ok(out) => (TaskStatus::Completed, Some(out), None),
        Err(e) => (TaskStatus::Failed, None, Some(format!("{e:#}"))),
    };
    let next_run_at = if let Some(expr) = &task.cron_expr {
        // cron 任务：算下一次
        match CronExpr::new(expr) {
            Ok(c) => c.next_after(chrono::Utc::now().timestamp()),
            Err(_) => None,
        }
    } else {
        None // 一次性任务不再排
    };
    store.finish_run(run_id, task.id, status, output.as_deref(), error.as_deref(), next_run_at)?;
    Ok(())
}

async fn dispatch(task: &Task) -> Result<String> {
    match task.kind {
        TaskKind::Shell => run_shell(&task.args).await,
        TaskKind::Prompt => Ok(format!(
            "[prompt] 任务被创建但 worker 模式暂无 LLM 上下文；args: {}",
            task.args
        )),
        TaskKind::RagQuery => Ok(format!(
            "[rag_query] 无 RAG 上下文；args: {}",
            task.args
        )),
        TaskKind::WebSearch => Ok(format!(
            "[web_search] 无 search 上下文；args: {}",
            task.args
        )),
    }
}

async fn run_shell(args: &str) -> Result<String> {
    use tokio::process::Command;
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let (out_tx, out_rx) = tokio::sync::oneshot::channel();
    let (err_tx, err_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf).await;
        let _ = out_tx.send(buf);
    });
    tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        let _ = err_tx.send(buf);
    });
    let out_bytes = out_rx.await.unwrap_or_default();
    let err_bytes = err_rx.await.unwrap_or_default();
    let status = child.wait().await?;
    let stdout = String::from_utf8_lossy(&out_bytes).to_string();
    let stderr = String::from_utf8_lossy(&err_bytes).to_string();
    // 截断 4KB
    let preview = format!(
        "{}{}",
        stdout.chars().take(2048).collect::<String>(),
        if !stderr.is_empty() {
            format!("\n[stderr]\n{}", stderr.chars().take(1024).collect::<String>())
        } else {
            String::new()
        }
    );
    if !status.success() {
        return Err(anyhow::anyhow!(
            "shell 退出 {:?}: {}",
            status.code(),
            preview
        ));
    }
    Ok(preview)
}

#[allow(dead_code)]
fn _unused_dummy() -> serde_json::Value {
    json!({})
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::task::TaskKind;
    use std::sync::Arc;
    use super::super::store::TaskStore;

    #[tokio::test]
    async fn shell_task_runs() {
        let store = Arc::new(TaskStore::open_in_memory().unwrap());
        let id = store.add("echo-task", TaskKind::Shell, "echo hello", None, "manual").unwrap();
        store.approve(id, "user:test").unwrap();
        let t = store.get(id).unwrap().unwrap();
        run_task(store.clone(), t).await.unwrap();
        let t = store.get(id).unwrap().unwrap();
        assert_eq!(t.status, TaskStatus::Completed);
        assert!(t.last_result.unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn shell_task_fails_captured() {
        let store = Arc::new(TaskStore::open_in_memory().unwrap());
        let id = store.add("fail", TaskKind::Shell, "exit 1", None, "manual").unwrap();
        store.approve(id, "user:test").unwrap();
        let t = store.get(id).unwrap().unwrap();
        run_task(store.clone(), t).await.unwrap();
        let t = store.get(id).unwrap().unwrap();
        assert_eq!(t.status, TaskStatus::Failed);
        assert!(t.error.is_some());
    }
}
