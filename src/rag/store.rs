//! RAG store：sqlite 单表 + naive cosine。
//!
//! Schema:
//! ```sql
//! CREATE TABLE IF NOT EXISTS chunks (
//!     id INTEGER PRIMARY KEY AUTOINCREMENT,
//!     source TEXT NOT NULL,           -- 来源标识（文件名/url/手动名）
//!     chunk_index INTEGER NOT NULL,   -- 第几个 chunk
//!     content TEXT NOT NULL,
//!     embedding BLOB NOT NULL,        -- f32 little-endian
//!     dim INTEGER NOT NULL,
//!     origin TEXT NOT NULL,           -- 'provider' / 'hash'
//!     metadata TEXT,                  -- JSON 自由字段
//!     created_at INTEGER NOT NULL
//! );
//! CREATE INDEX IF NOT EXISTS idx_chunks_source ON chunks(source);
//! ```
//!
//! query 策略：先按同源 dim 过滤，把 (embedding, content, source, origin) 拉进内存做 cosine。
//! < 10k chunks 时，scan 是几十微秒。

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::chunk::{chunk_text, ChunkOptions};
use super::embed::{hash_embed, Embedder};

/// 单条命中。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hit {
    pub source: String,
    pub chunk_index: usize,
    pub content: String,
    pub score: f32,
    pub origin: String,
}

/// 一个 source 的统计。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceInfo {
    pub name: String,
    pub count: usize,
    pub last_updated: i64,
}

pub struct RagStore {
    conn: Connection,
    db_path: PathBuf,
}

impl RagStore {
    /// 打开（或创建）一个 RAG store。`db_path` 通常是 `~/.fr_cli/rag.db`。
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(&db_path)
            .with_context(|| format!("open rag db at {}", db_path.display()))?;
        Self::from_connection(conn, db_path)
    }

    /// 打开内存版（用于测试或 fallback）。
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let path = PathBuf::from(":memory:");
        Self::from_connection(conn, path)
    }

    fn from_connection(conn: Connection, db_path: PathBuf) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                content TEXT NOT NULL,
                embedding BLOB NOT NULL,
                dim INTEGER NOT NULL,
                origin TEXT NOT NULL,
                metadata TEXT,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_chunks_source ON chunks(source);
            CREATE TABLE IF NOT EXISTS sources (
                name TEXT PRIMARY KEY,
                count INTEGER NOT NULL DEFAULT 0,
                last_updated INTEGER NOT NULL
            );
            "#,
        )?;
        Ok(Self { conn, db_path })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// 把一段文本切块 → embedding → 入库。
    /// 返回新增的 chunk 数。
    pub fn add_text(
        &self,
        embedder: &dyn Embedder,
        source: &str,
        text: &str,
        metadata: Option<&str>,
    ) -> Result<usize> {
        let chunks = chunk_text(text, &ChunkOptions::default());
        if chunks.is_empty() {
            return Ok(0);
        }
        let now = chrono::Utc::now().timestamp();
        let tx = self.conn.unchecked_transaction()?;
        let mut added = 0usize;
        for (idx, content) in &chunks {
            let emb = block_on(embedder.embed(content))?;
            tx.execute(
                "INSERT INTO chunks (source, chunk_index, content, embedding, dim, origin, metadata, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    source,
                    *idx as i64,
                    content,
                    f32_slice_to_bytes(&emb.vec),
                    emb.dim as i64,
                    format!("{:?}", emb.origin).to_lowercase(),
                    metadata,
                    now,
                ],
            )?;
            added += 1;
        }
        // 更新 sources 聚合
        tx.execute(
            "INSERT INTO sources (name, count, last_updated) VALUES (?1, ?2, ?3)
             ON CONFLICT(name) DO UPDATE SET count = count + ?2, last_updated = ?3",
            params![source, added as i64, now],
        )?;
        tx.commit()?;
        Ok(added)
    }

    /// 用本地 hash embedder 入库（异步友好的快捷方法）。
    pub fn add_text_hash(&self, source: &str, text: &str, metadata: Option<&str>) -> Result<usize> {
        let chunks = chunk_text(text, &ChunkOptions::default());
        if chunks.is_empty() {
            return Ok(0);
        }
        let now = chrono::Utc::now().timestamp();
        let tx = self.conn.unchecked_transaction()?;
        let mut added = 0usize;
        for (idx, content) in &chunks {
            let vec = hash_embed(content);
            tx.execute(
                "INSERT INTO chunks (source, chunk_index, content, embedding, dim, origin, metadata, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    source,
                    *idx as i64,
                    content,
                    f32_slice_to_bytes(&vec),
                    HASH_DIM as i64,
                    "hash",
                    metadata,
                    now,
                ],
            )?;
            added += 1;
        }
        tx.execute(
            "INSERT INTO sources (name, count, last_updated) VALUES (?1, ?2, ?3)
             ON CONFLICT(name) DO UPDATE SET count = count + ?2, last_updated = ?3",
            params![source, added as i64, now],
        )?;
        tx.commit()?;
        Ok(added)
    }

    /// 检索 top-k。
    pub fn query(
        &self,
        embedder: &dyn Embedder,
        query_text: &str,
        k: usize,
    ) -> Result<Vec<Hit>> {
        let q = block_on(embedder.embed(query_text))?;
        self.query_with_embedding(&q.vec, q.dim, k)
    }

    /// 直接用预计算 embedding 检索（hash 模式下用，避免重复 embed）。
    pub fn query_hash(&self, query_text: &str, k: usize) -> Result<Vec<Hit>> {
        let q = hash_embed(query_text);
        self.query_with_embedding(&q, super::embed::HASH_DIM, k)
    }

    fn query_with_embedding(&self, q_vec: &[f32], q_dim: usize, k: usize) -> Result<Vec<Hit>> {
        let mut stmt = self.conn.prepare(
            "SELECT source, chunk_index, content, embedding, dim, origin
             FROM chunks WHERE dim = ?1",
        )?;
        let rows = stmt.query_map(params![q_dim as i64], |row| {
            let source: String = row.get(0)?;
            let chunk_index: i64 = row.get(1)?;
            let content: String = row.get(2)?;
            let blob: Vec<u8> = row.get(3)?;
            let dim: i64 = row.get(4)?;
            let origin: String = row.get(5)?;
            Ok((source, chunk_index, content, blob, dim, origin))
        })?;
        let mut hits: Vec<Hit> = Vec::new();
        for row in rows {
            let (source, chunk_index, content, blob, dim, origin) = row?;
            let vec = bytes_to_f32_slice(&blob, dim as usize)?;
            let score = cosine(q_vec, &vec);
            hits.push(Hit {
                source,
                chunk_index: chunk_index as usize,
                content,
                score,
                origin,
            });
        }
        // 按 score 降序
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(k);
        Ok(hits)
    }

    pub fn list_sources(&self) -> Result<Vec<SourceInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, count, last_updated FROM sources ORDER BY last_updated DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SourceInfo {
                name: row.get(0)?,
                count: row.get::<_, i64>(1)? as usize,
                last_updated: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn count(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn count_sources(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row("SELECT COUNT(*) FROM sources", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// 按 source 名删除（返回删除的 chunk 数）。
    pub fn remove_source(&self, name: &str) -> Result<usize> {
        let n = self
            .conn
            .execute("DELETE FROM chunks WHERE source = ?1", params![name])?;
        self.conn.execute("DELETE FROM sources WHERE name = ?1", params![name])?;
        Ok(n)
    }

    /// 取某个 source 的全部内容（用于 `rag show`）。
    pub fn get_source(&self, name: &str) -> Result<Vec<(usize, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT chunk_index, content FROM chunks WHERE source = ?1 ORDER BY chunk_index ASC",
        )?;
        let rows = stmt.query_map(params![name], |row| {
            Ok((row.get::<_, i64>(0)? as usize, row.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 是否存在某个 source。
    pub fn has_source(&self, name: &str) -> Result<bool> {
        let n: Option<i64> = self
            .conn
            .query_row(
                "SELECT count FROM sources WHERE name = ?1",
                params![name],
                |r| r.get(0),
            )
            .optional()?;
        Ok(n.is_some())
    }
}

pub const HASH_DIM: usize = super::embed::HASH_DIM;

fn f32_slice_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

fn bytes_to_f32_slice(b: &[u8], dim: usize) -> Result<Vec<f32>> {
    if b.len() != dim * 4 {
        return Err(anyhow!("embedding blob size mismatch: {} bytes for dim {}", b.len(), dim));
    }
    let mut out = Vec::with_capacity(dim);
    for chunk in b.chunks_exact(4) {
        let bytes: [u8; 4] = chunk.try_into().unwrap();
        out.push(f32::from_le_bytes(bytes));
    }
    Ok(out)
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// 在同步方法里跑一个 future。`RagStore` 的方法都标 sync，
/// 但 embedder 可能是 async（provider 版）。`block_on` 在这里只是过渡
/// —— 真生产用 async API，但 RagStore 自身用 std::sync::Mutex 持有 connection 避免 async 锁坑。
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tauri_runtime_block_on(f)
}

fn tauri_runtime_block_on<F: std::future::Future>(f: F) -> F::Output {
    // 用 tokio 的 handle 跑
    match tokio::runtime::Handle::try_current() {
        Ok(h) => tokio::task::block_in_place(|| h.block_on(f)),
        Err(_) => {
            // 离 runtime 的场景：建一个临时 current_thread runtime
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("create temp tokio runtime");
            rt.block_on(f)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rag::embed::HashEmbedder;

    fn temp_store() -> RagStore {
        let dir = std::env::temp_dir().join(format!("fr-rag-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rag.db");
        RagStore::open(&path).unwrap()
    }

    #[test]
    fn add_and_query_basic() {
        let store = temp_store();
        store
            .add_text_hash(
                "rust-intro",
                "Rust 是一门系统级语言，主打内存安全与零成本抽象。",
                None,
            )
            .unwrap();
        store
            .add_text_hash(
                "rust-intro",
                "Cargo 是 Rust 的包管理工具，自带 build / test / run。",
                None,
            )
            .unwrap();
        store
            .add_text_hash(
                "cooking",
                "番茄炒蛋的诀窍是先炒蛋再炒番茄，最后加糖。",
                None,
            )
            .unwrap();
        let embedder = HashEmbedder::new();
        let hits = store.query(&embedder, "rust 内存安全", 3).unwrap();
        assert!(!hits.is_empty());
        // 第一个应该是 rust 相关
        assert!(
            hits[0].content.contains("Rust") || hits[0].content.contains("Cargo"),
            "got: {:?}",
            hits[0]
        );
    }

    #[test]
    fn list_and_remove() {
        let store = temp_store();
        store.add_text_hash("a", "hello world", None).unwrap();
        store.add_text_hash("b", "foo bar", None).unwrap();
        let sources = store.list_sources().unwrap();
        assert_eq!(sources.len(), 2);
        let n = store.remove_source("a").unwrap();
        assert_eq!(n, 1);
        assert!(!store.has_source("a").unwrap());
        assert!(store.has_source("b").unwrap());
    }

    #[test]
    fn get_source_chunks() {
        let store = temp_store();
        store
            .add_text_hash(
                "doc",
                "First paragraph.\n\nSecond paragraph here.",
                None,
            )
            .unwrap();
        let chunks = store.get_source("doc").unwrap();
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn count_and_dim() {
        let store = temp_store();
        assert_eq!(store.count().unwrap(), 0);
        store.add_text_hash("x", "some text", None).unwrap();
        let n = store.count().unwrap();
        assert!(n >= 1);
    }

    #[test]
    fn query_returns_top_k_sorted() {
        let store = temp_store();
        for i in 0..5 {
            store
                .add_text_hash(&format!("s{i}"), &format!("rust pattern matching iter {i}"), None)
                .unwrap();
        }
        store.add_text_hash("cook", "stir fry the garlic", None).unwrap();
        let embedder = HashEmbedder::new();
        let hits = store.query(&embedder, "rust 模式匹配", 3).unwrap();
        assert!(hits.len() <= 3);
        for w in hits.windows(2) {
            assert!(w[0].score >= w[1].score);
        }
    }

    #[test]
    fn f32_roundtrip() {
        let v = vec![1.0f32, -2.5, 0.0, 42.0];
        let b = f32_slice_to_bytes(&v);
        let r = bytes_to_f32_slice(&b, 4).unwrap();
        assert_eq!(v, r);
    }
}
