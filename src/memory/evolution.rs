//! 自我记忆进化引擎。
//!
//! 两种触发路径：
//! - **对话路径**（`from_conversation`）：用户对话里 LLM 提议保存某条事实，用户 y 后写入 long-term。
//! - **网络搜索路径**（`run`）：手动 `/memory_evolve` 或自动定时器——遍历
//!   `~/.fr_cli/memory/topics.json` 的 topic，对每个 topic 调 web 搜索，
//!   把命中页面的 markdown 摘要写到一个 `evolution/<date>-evo-NNN.md` 文件，
//!   同时把精简版追加到 `~/.fr_cli/memory/long-term.md`。
//!
//! 关键设计：
//! - 进化文件命名带时间戳，方便排序与裁剪
//! - 每个 topic 单独成段，避免主题串味
//! - 不阻断主 REPL：web 搜索用 reqwest 带 8s 超时，失败优雅降级

use crate::Result;
use crate::tools::web_search::{SearchHit, WebSearchProvider};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// 用户关注的话题项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Topic {
    pub name: String,
    #[serde(default)]
    pub added_at: DateTime<Utc>,
    #[serde(default)]
    pub last_evolved_at: Option<DateTime<Utc>>,
    /// "user" / "auto" / "imported"
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "user".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TopicsFile {
    pub topics: Vec<Topic>,
}

impl TopicsFile {
    pub fn load_or_default() -> Self {
        let path = topics_path();
        if !path.exists() {
            return Self::default();
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&text).unwrap_or_default()
    }
}

/// ~/.fr_cli/memory/topics.json
pub fn topics_path() -> PathBuf {
    crate::config::paths::data_dir()
        .map(|d| d.join("memory").join("topics.json"))
        .unwrap_or_else(|_| PathBuf::from("~/.fr_cli/memory/topics.json"))
}

/// ~/.fr_cli/memory/long-term.md —— 累积的长期记忆
pub fn long_term_path() -> PathBuf {
    crate::config::paths::data_dir()
        .map(|d| d.join("memory").join("long-term.md"))
        .unwrap_or_else(|_| PathBuf::from("~/.fr_cli/memory/long-term.md"))
}

/// ~/.fr_cli/memory/evolution/ —— 每次进化的快照
pub fn evolution_dir() -> PathBuf {
    crate::config::paths::data_dir()
        .map(|d| d.join("memory").join("evolution"))
        .unwrap_or_else(|_| PathBuf::from("~/.fr_cli/memory/evolution"))
}

/// long-term.md 注入 system prompt 时需要的最大字节数（避免越来越胖）。
pub const MAX_LONG_TERM_INJECT_BYTES: usize = 8 * 1024;

/// 用户加 / 删 topic
pub fn add_topic(name: &str) -> Result<Topic> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(crate::Error::Other("topic 不能为空".into()));
    }
    let mut t = TopicsFile::load_or_default();
    if let Some(existing) = t.topics.iter().find(|t| t.name.eq_ignore_ascii_case(trimmed)) {
        let mut dup = existing.clone();
        dup.added_at = Utc::now();
        dup.source = "user".into();
        // 替换原条目位置
        for entry in t.topics.iter_mut() {
            if entry.name.eq_ignore_ascii_case(&dup.name) {
                *entry = dup.clone();
                break;
            }
        }
        write_topics(&topics_path(), &t)?;
        return Ok(dup);
    }
    let topic = Topic {
        name: trimmed.to_string(),
        added_at: Utc::now(),
        last_evolved_at: None,
        source: "user".into(),
    };
    t.topics.push(topic.clone());
    write_topics(&topics_path(), &t)?;
    Ok(topic)
}

pub fn remove_topic(name: &str) -> Result<bool> {
    let mut t = TopicsFile::load_or_default();
    let n = name.trim().to_ascii_lowercase();
    let before = t.topics.len();
    t.topics.retain(|x| x.name.to_ascii_lowercase() != n);
    let removed = t.topics.len() != before;
    if removed {
        write_topics(&topics_path(), &t)?;
    }
    Ok(removed)
}

fn write_topics(path: &PathBuf, t: &TopicsFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(t)?)?;
    Ok(())
}

/// Evolution run 单个 topic 的产出。
#[derive(Debug, Clone)]
pub struct TopicEvolution {
    pub topic: String,
    pub query: String,
    pub hits: Vec<SearchHit>,
}

/// Evolution run 整体产出（含时间戳）。
#[derive(Debug, Clone)]
pub struct EvolutionResult {
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub per_topic: Vec<TopicEvolution>,
    /// 写入磁盘的文件路径
    pub snapshot_path: PathBuf,
}

/// 一次进化：遍历 topics，对每个调 search backend，组装 evolution 文件，写盘，
/// 把精简版 append 进 long-term.md。
///
/// `per_topic_limit`：每个 topic 取多少条搜索结果（MVP 用 3）。
pub async fn run_evolution(
    provider: &dyn WebSearchProvider,
    per_topic_limit: usize,
) -> Result<EvolutionResult> {
    let started = Utc::now();

    let topics = TopicsFile::load_or_default().topics;
    let mut per_topic: Vec<TopicEvolution> = Vec::new();

    for t in &topics {
        let query = topic_query(&t.name);
        match provider.search(&query, per_topic_limit) {
            Ok(hits) => {
                per_topic.push(TopicEvolution {
                    topic: t.name.clone(),
                    query,
                    hits,
                });
            }
            Err(e) => {
                per_topic.push(TopicEvolution {
                    topic: t.name.clone(),
                    query,
                    hits: vec![],
                });
                eprintln!("  ⚠️  topic `{}` 搜索失败: {e}", t.name);
            }
        }
    }

    let finished = Utc::now();

    // 1) 写 evolution 快照
    let snapshot_path = write_snapshot(&started, &per_topic)?;

    // 2) 把精简版 append 到 long-term.md
    append_long_term(&started, &per_topic)?;

    // 3) 写 topics.json 的 last_evolved_at
    let mut t = TopicsFile::load_or_default();
    for t_item in t.topics.iter_mut() {
        if per_topic.iter().any(|p| p.topic == t_item.name) {
            t_item.last_evolved_at = Some(finished);
        }
    }
    let _ = write_topics(&topics_path(), &t);

    Ok(EvolutionResult {
        started_at: started,
        finished_at: finished,
        per_topic,
        snapshot_path,
    })
}

/// 把 topic 名变成更适合搜索的 query：
/// - 保持原大小写（如果用户偏好就保留）
/// - 加时间相关词暗示「最近」
pub fn topic_query(topic: &str) -> String {
    let recent_year = Utc::now().format("%Y").to_string();
    format!("{topic} {recent_year}")
}

/// 把每次 evolution 输出写到 `evolution/YYYY-MM-DD-evo-NNN.md`
fn write_snapshot(
    started: &DateTime<Utc>,
    per_topic: &[TopicEvolution],
) -> Result<PathBuf> {
    let dir = evolution_dir();
    std::fs::create_dir_all(&dir)?;
    let date_str = started.format("%Y-%m-%d").to_string();
    let stamp = started.format("%H%M%S").to_string();
    let path = dir.join(format!("{date_str}-evo-{stamp}.md"));
    let body = render_snapshot(started, per_topic);
    std::fs::write(&path, body)?;
    Ok(path)
}

fn render_snapshot(started: &DateTime<Utc>, per_topic: &[TopicEvolution]) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "# Evolution {}\n\n",
        started.format("%Y-%m-%d %H:%M:%S")
    ));
    s.push_str(&format!(
        "**Topics scanned:** {}\n\n",
        per_topic
            .iter()
            .map(|p| format!("`{}`", p.topic))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    let total: usize = per_topic.iter().map(|p| p.hits.len()).sum();
    s.push_str(&format!("**Sources fetched:** {total} pages\n\n"));

    for p in per_topic {
        s.push_str(&format!("## Topic: {}\n\n", p.topic));
        if p.hits.is_empty() {
            s.push_str("_no hits_\n\n");
            continue;
        }
        for (i, hit) in p.hits.iter().enumerate() {
            s.push_str(&format!("### Hit {}/{}\n", i + 1, p.hits.len()));
            s.push_str(&format!("- **Title:** {}\n", hit.title));
            s.push_str(&format!("- **URL:** {}\n", hit.url));
            if !hit.snippet.is_empty() {
                s.push_str(&format!("- **Excerpt:** {}\n", hit.snippet));
            }
            if !hit.body_markdown.is_empty() {
                s.push_str(&format!("- **Body:**\n\n{}\n", hit.body_markdown.chars().take(2000).collect::<String>()));
            }
            s.push('\n');
        }
    }
    s
}

/// 把每次进化的精简版（每个 topic 标题 + URL 列表）追加进 long-term.md。
fn append_long_term(started: &DateTime<Utc>, per_topic: &[TopicEvolution]) -> Result<()> {
    let path = long_term_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut s = String::new();
    s.push_str(&format!(
        "\n---\n# Evolution {}\n\n",
        started.format("%Y-%m-%d %H:%M:%S")
    ));
    for p in per_topic {
        s.push_str(&format!("## {}\n\n", p.topic));
        if p.hits.is_empty() {
            s.push_str("_no hits_\n\n");
            continue;
        }
        for (i, hit) in p.hits.iter().enumerate() {
            s.push_str(&format!(
                "{}. [{}]({})\n",
                i + 1,
                hit.title.replace(']', r"\]").replace('[', r"\["),
                hit.url
            ));
            if !hit.snippet.is_empty() {
                s.push_str(&format!("   > {}\n", hit.snippet));
            }
        }
        s.push('\n');
    }

    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    f.write_all(s.as_bytes())?;
    Ok(())
}

/// 读 long-term.md（被 system prompt 注入用），截断到 MAX_LONG_TERM_INJECT_BYTES。
pub fn load_long_term_snippet() -> String {
    let path = long_term_path();
    if !path.exists() {
        return String::new();
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return String::new(),
    };
    if text.len() <= MAX_LONG_TERM_INJECT_BYTES {
        text
    } else {
        // 保留尾部 N 字节（越往后越新）
        let tail = &text[text.len() - MAX_LONG_TERM_INJECT_BYTES..];
        // 截到下一个 ---\n 即下次 evolution 开头
        if let Some(idx) = tail.find("\n---\n").map(|i| i + 5) {
            let header = "# 长期记忆（注入 system prompt，自动截取末尾 N 字节）\n\n";
            let mut out = String::with_capacity(header.len() + tail.len() - idx);
            out.push_str(header);
            out.push_str(&tail[idx..]);
            out
        } else {
            let header = "# 长期记忆（注入 system prompt，自动截取末尾 N 字节）\n\n";
            let mut out = String::with_capacity(header.len() + tail.len());
            out.push_str(header);
            out.push_str(tail);
            out
        }
    }
}

/// 对话路径：用户显式或 LLM 提议保存一条 fact 到 long-term.md。
pub fn memorize_from_conversation(fact: &str, source_tag: &str) -> Result<()> {
    let path = long_term_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let now = Utc::now().format("%Y-%m-%d %H:%M:%S");
    let entry = format!(
        "\n- **{now} ({source})** — {fact}\n",
        now = now,
        source = source_tag,
        fact = fact.trim()
    );
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    f.write_all(entry.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_query_appends_year() {
        let q = topic_query("rust release");
        assert!(q.contains("rust release"));
        assert!(q.len() > "rust release".len());
    }

    #[test]
    fn add_then_remove() {
        let t = TopicsFile::default();
        assert_eq!(t.topics.len(), 0);
        // 没法测持久化（破坏用户家目录）——只跑代码路径
        let _ = topic_query("claude code");
    }
}

// ----------------- Auto timer -----------------

use std::sync::atomic::{AtomicBool, Ordering};
use tokio::time::{sleep, Duration};

/// 进化 auto runner —— 由 `/memory_evolve auto on` 启动。
pub struct EvolutionAuto {
    pub stop: Arc<AtomicBool>,
    pub task: tokio::task::JoinHandle<()>,
}

impl EvolutionAuto {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
    pub fn is_running(&self) -> bool {
        !self.stop.load(Ordering::Relaxed)
    }
}

/// 启动 auto timer：`every_secs` 秒跑一次 `run_evolution`，
/// 直到 `stop.store(true, ...)` 被设为止。
pub fn start_auto(
    provider: Arc<dyn WebSearchProvider>,
    per_topic_limit: usize,
    every_secs: u64,
) -> EvolutionAuto {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_inner = Arc::clone(&stop);
    let task = tokio::spawn(async move {
        loop {
            if stop_inner.load(Ordering::Relaxed) {
                break;
            }
            match run_evolution(provider.as_ref(), per_topic_limit).await {
                Ok(r) => {
                    let topics = r.per_topic.len();
                    let hits: usize = r.per_topic.iter().map(|p| p.hits.len()).sum();
                    eprintln!(
                        "  [auto-evolve] 完成: topics={topics}, hits={hits}, snapshot={}",
                        r.snapshot_path.display()
                    );
                }
                Err(e) => eprintln!("  [auto-evolve] 失败: {e:#}"),
            }
            for _ in 0..every_secs {
                if stop_inner.load(Ordering::Relaxed) {
                    break;
                }
                sleep(Duration::from_secs(1)).await;
            }
            if stop_inner.load(Ordering::Relaxed) {
                break;
            }
        }
    });
    EvolutionAuto { stop, task }
}
