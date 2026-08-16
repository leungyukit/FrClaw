//! Plan 持久化 ── 复用 hermes 的 sqlite（加 2 表）。

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::dag::PlanDag;

pub struct PlanStore {
    conn: Mutex<Connection>,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl PlanStore {
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(&db_path)
            .with_context(|| format!("open plans db at {}", db_path.display()))?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS plans (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                goal TEXT NOT NULL,
                dag_json TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_plans_status ON plans(status);
            "#,
        )?;
        Ok(Self { conn: Mutex::new(conn), db_path })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS plans (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                goal TEXT NOT NULL,
                dag_json TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            "#,
        )?;
        Ok(Self { conn: Mutex::new(conn), db_path: PathBuf::from(":memory:") })
    }

    /// 存 plan
    pub fn save(&self, dag: &PlanDag) -> Result<i64> {
        let json = serde_json::to_string(dag)?;
        let now = chrono::Utc::now().timestamp();
        let conn = self.conn.lock().unwrap();
        if dag.id == 0 {
            conn.execute(
                "INSERT INTO plans (name, goal, dag_json, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![dag.name, dag.goal, json, dag.status.as_str(), now],
            )?;
            Ok(conn.last_insert_rowid())
        } else {
            conn.execute(
                "UPDATE plans SET name=?1, goal=?2, dag_json=?3, status=?4, updated_at=?5 WHERE id=?6",
                params![dag.name, dag.goal, json, dag.status.as_str(), now, dag.id],
            )?;
            Ok(dag.id)
        }
    }

    pub fn get(&self, id: i64) -> Result<Option<PlanDag>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT dag_json FROM plans WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(r) = rows.next()? {
            let s: String = r.get(0)?;
            Ok(Some(serde_json::from_str(&s)?))
        } else {
            Ok(None)
        }
    }

    pub fn list(&self) -> Result<Vec<PlanDag>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT dag_json FROM plans ORDER BY created_at DESC LIMIT 100")?;
        let rows = stmt.query_map([], |r| {
            let s: String = r.get(0)?;
            Ok(serde_json::from_str::<PlanDag>(&s).unwrap_or_else(|_| PlanDag::new("?", "?")))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM plans WHERE id = ?1", params![id])?;
        Ok(())
    }
}

/// 全局 handle（Arc 共享）
pub type PlanStoreHandle = Arc<PlanStore>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::dag::{PlanNode, PlanNodeStatus};

    #[test]
    fn save_and_get() {
        let store = PlanStore::open_in_memory().unwrap();
        let mut dag = PlanDag::new("test", "test goal");
        dag.add_node(PlanNode {
            id: "n1".into(),
            name: "step 1".into(),
            prompt: "do something".into(),
            expected_output: None,
            max_retries: 2,
            retry_count: 0,
            status: PlanNodeStatus::Ready,
            runs: vec![],
        });
        let id = store.save(&dag).unwrap();
        let got = store.get(id).unwrap().unwrap();
        assert_eq!(got.name, "test");
        assert_eq!(got.nodes.len(), 1);
        assert_eq!(got.nodes[0].id, "n1");
    }
}
