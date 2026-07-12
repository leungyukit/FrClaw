//! RAG 个人知识库。
//!
//! 设计目标：
//! - **零扩展依赖**：用 `rusqlite` (bundled) 单表存 chunks + embedding BLOB
//! - **小规模 naive cosine**：< 10k chunks 时，scan 是微秒级；不引 sqlite-vec
//! - **embedding 双路**：先试 provider embeddings endpoint，失败 fallback 到本地 hashing TF
//! - **chunking 简单**：按段落/句子切，~500 字符 + 50 字符 overlap
//! - **Round 15b 升级**：混合检索（BM25 关键词 + vector cosine 加权融合）+ source 路由
//! - **跟 memorize 联动**：长期记忆双写到 RAG

pub mod chunk;
pub mod embed;
pub mod hybrid;
pub mod store;

pub use chunk::{chunk_text, ChunkOptions};
pub use embed::{hash_embed, Embedding, Embedder, ProviderEmbedder};
pub use hybrid::{bm25_score, hybrid_search, HybridHit, HybridOptions};
pub use store::{Hit, RagStore, SourceInfo};
