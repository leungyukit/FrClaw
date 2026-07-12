//! Task store：sqlite 持久化。
//!
//! 复用 `rusqlite`（rag crate 已经在依赖里）。

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::task::{Task, TaskKind, TaskRun, TaskStatus};

pub struct TaskStore {
    conn: Mutex<Connection>,
    db_path: PathBuf,
}

impl TaskStore {
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(&db_path)
            .with_context(|| format!("open tasks db at {}", db_path.display()))?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                args TEXT NOT NULL,
                status TEXT NOT NULL,
                cron_expr TEXT,
                last_run_at INTEGER,
                next_run_at INTEGER,
                last_result TEXT,
                error TEXT,
                run_count INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                approved_at INTEGER,
                approved_by TEXT,
                approval_mode TEXT NOT NULL DEFAULT 'manual'
            );
            CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
            CREATE INDEX IF NOT EXISTS idx_tasks_next_run ON tasks(next_run_at);
            CREATE TABLE IF NOT EXISTS task_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                started_at INTEGER NOT NULL,
                finished_at INTEGER,
                status TEXT NOT NULL,
                output TEXT,
                error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_task_runs_task ON task_runs(task_id);
            "#,
        )?;
        Ok(Self { conn: Mutex::new(conn), db_path })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db_path = PathBuf::from(":memory:");
        // 直接跑一次 schema（不调 init_schema —— 它已被删除）
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                args TEXT NOT NULL,
                status TEXT NOT NULL,
                cron_expr TEXT,
                last_run_at INTEGER,
                next_run_at INTEGER,
                last_result TEXT,
                error TEXT,
                run_count INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                approved_at INTEGER,
                approved_by TEXT,
                approval_mode TEXT NOT NULL DEFAULT 'manual'
            );
            CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
            CREATE TABLE IF NOT EXISTS task_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                started_at INTEGER NOT NULL,
                finished_at INTEGER,
                status TEXT NOT NULL,
                output TEXT,
                error TEXT
            );
            "#,
        )?;
        Ok(Self { conn: Mutex::new(conn), db_path })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn add(
        &self,
        name: &str,
        kind: TaskKind,
        args: &str,
        cron_expr: Option<&str>,
        approval_mode: &str,
    ) -> Result<i64> {
        let now = chrono::Utc::now().timestamp();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tasks (name, kind, args, status, cron_expr, next_run_at, created_at, approval_mode)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                name,
                kind.as_str(),
                args,
                TaskStatus::Pending.as_str(),
                cron_expr,
                now, // pending 任务立即可审 / 可执行
                now,
                approval_mode,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list(&self) -> Result<Vec<Task>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, args, status, cron_expr, last_run_at, next_run_at,
                    last_result, error, run_count, created_at, approved_at, approved_by, approval_mode
             FROM tasks ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([], row_to_task)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn get(&self, id: i64) -> Result<Option<Task>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, args, status, cron_expr, last_run_at, next_run_at,
                    last_result, error, run_count, created_at, approved_at, approved_by, approval_mode
             FROM tasks WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Ok(Some(r)) = rows.next() {
            Ok(Some(row_to_task(r)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_status(&self, id: i64, status: TaskStatus) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tasks SET status = ?1 WHERE id = ?2",
            params![status.as_str(), id],
        )?;
        Ok(())
    }

    pub fn approve(&self, id: i64, by: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tasks SET status = 'approved', approved_at = ?1, approved_by = ?2
             WHERE id = ?3 AND status = 'pending'",
            params![now, by, id],
        )?;
        Ok(())
    }

    pub fn reject(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tasks SET status = 'rejected' WHERE id = ?1 AND status = 'pending'",
            params![id],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
        conn.execute("DELETE FROM task_runs WHERE task_id = ?1", params![id])?;
        Ok(())
    }

    pub fn start_run(&self, task_id: i64) -> Result<i64> {
        let now = chrono::Utc::now().timestamp();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tasks SET status = 'running', last_run_at = ?1, run_count = run_count + 1
             WHERE id = ?2",
            params![now, task_id],
        )?;
        conn.execute(
            "INSERT INTO task_runs (task_id, started_at, status) VALUES (?1, ?2, 'running')",
            params![task_id, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn finish_run(
        &self,
        run_id: i64,
        task_id: i64,
        status: TaskStatus,
        output: Option<&str>,
        error: Option<&str>,
        next_run_at: Option<i64>,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE task_runs SET finished_at = ?1, status = ?2, output = ?3, error = ?4
             WHERE id = ?5",
            params![now, status.as_str(), output, error, run_id],
        )?;
        // 截断 result 到 2KB
        let result_trunc = output.map(|o| {
            if o.len() > 2048 {
                let s: String = o.chars().take(2048).collect();
                format!("{s}...")
            } else {
                o.to_string()
            }
        });
        conn.execute(
            "UPDATE tasks SET status = ?1, last_result = ?2, error = ?3, next_run_at = ?4
             WHERE id = ?5",
            params![status.as_str(), result_trunc, error, next_run_at, task_id],
        )?;
        Ok(())
    }

    /// 找 `next_run_at <= now AND status = 'approved'` 的任务。
    pub fn due_tasks(&self) -> Result<Vec<Task>> {
        let now = chrono::Utc::now().timestamp();
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, args, status, cron_expr, last_run_at, next_run_at,
                    last_result, error, run_count, created_at, approved_at, approved_by, approval_mode
             FROM tasks
             WHERE status = 'approved' AND next_run_at IS NOT NULL AND next_run_at <= ?1
             ORDER BY next_run_at ASC
             LIMIT 32",
        )?;
        let rows = stmt.query_map(params![now], row_to_task)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn runs_for(&self, task_id: i64) -> Result<Vec<TaskRun>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, started_at, finished_at, status, output, error
             FROM task_runs WHERE task_id = ?1 ORDER BY id DESC LIMIT 50",
        )?;
        let rows = stmt.query_map(params![task_id], |row| {
            Ok(TaskRun {
                id: row.get(0)?,
                task_id: row.get(1)?,
                started_at: row.get(2)?,
                finished_at: row.get(3)?,
                status: TaskStatus::parse(
                    &row.get::<_, String>(4)?,
                )
                .unwrap_or(TaskStatus::Failed),
                output: row.get(5)?,
                error: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: TaskKind::parse(&row.get::<_, String>(2)?).unwrap_or(TaskKind::Shell),
        args: row.get(3)?,
        status: TaskStatus::parse(&row.get::<_, String>(4)?).unwrap_or(TaskStatus::Pending),
        cron_expr: row.get(5)?,
        last_run_at: row.get(6)?,
        next_run_at: row.get(7)?,
        last_result: row.get(8)?,
        error: row.get(9)?,
        run_count: row.get(10)?,
        created_at: row.get(11)?,
        approved_at: row.get(12)?,
        approved_by: row.get(13)?,
        approval_mode: row.get(14)?,
    })
}

/// 共享的 TaskStore handle —— 给 HermesRegistry / commands 用。
pub type TaskStoreHandle = Arc<TaskStore>;

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> TaskStore {
        TaskStore::open_in_memory().unwrap()
    }

    #[test]
    fn add_and_list() {
        let s = store();
        let id = s.add("backup", TaskKind::Shell, "echo backup", None, "manual").unwrap();
        let list = s.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].name, "backup");
        assert_eq!(list[0].status, TaskStatus::Pending);
    }

    #[test]
    fn approve_changes_status() {
        let s = store();
        let id = s.add("x", TaskKind::Shell, "echo", None, "manual").unwrap();
        s.approve(id, "user:me").unwrap();
        let t = s.get(id).unwrap().unwrap();
        assert_eq!(t.status, TaskStatus::Approved);
        assert_eq!(t.approved_by.as_deref(), Some("user:me"));
    }

    #[test]
    fn reject() {
        let s = store();
        let id = s.add("x", TaskKind::Shell, "echo", None, "manual").unwrap();
        s.reject(id).unwrap();
        assert_eq!(s.get(id).unwrap().unwrap().status, TaskStatus::Rejected);
    }

    #[test]
    fn start_and_finish_run() {
        let s = store();
        let id = s.add("x", TaskKind::Shell, "echo", None, "manual").unwrap();
        s.approve(id, "auto").unwrap();
        let run_id = s.start_run(id).unwrap();
        s.finish_run(run_id, id, TaskStatus::Completed, Some("ok"), None, Some(999)).unwrap();
        let t = s.get(id).unwrap().unwrap();
        assert_eq!(t.status, TaskStatus::Completed);
        assert_eq!(t.run_count, 1);
        assert_eq!(t.next_run_at, Some(999));
        assert_eq!(t.last_result.as_deref(), Some("ok"));
    }

    #[test]
    fn due_tasks_filter() {
        let s = store();
        let now = chrono::Utc::now().timestamp();
        s.add("a", TaskKind::Shell, "x", None, "manual").unwrap();
        let b = s.add("b", TaskKind::Shell, "y", None, "auto").unwrap();
        s.approve(b, "auto").unwrap();
        // 直接把 b 的 next_run_at 调成过去（sql 直改）
        {
            let conn = s.conn.lock().unwrap();
            conn.execute(
                "UPDATE tasks SET next_run_at = ?1 WHERE id = ?2",
                rusqlite::params![now - 100, b],
            )
            .unwrap();
        }
        let due = s.due_tasks().unwrap();
        assert!(due.iter().any(|t| t.id == b), "due: {due:?}");
    }

    #[test]
    fn delete_cascades() {
        let s = store();
        let id = s.add("x", TaskKind::Shell, "x", None, "manual").unwrap();
        s.start_run(id).unwrap();
        s.delete(id).unwrap();
        let t = s.get(id).unwrap();
        assert!(t.is_none());
    }
}
