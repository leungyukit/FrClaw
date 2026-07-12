//! `web_search` 工具：LLM 端可见的搜索能力。
//!
//! - 默认 backend = DuckDuckGo HTML scraper（无需 key）
//! - 可选 backend = Brave（需要 `BRAVE_API_KEY` env）
//!
//! 同步实现（用 `reqwest::blocking`）——这样能嵌入到当前 sync tool dispatcher 里，
//! 不用单独起 tokio runtime。

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub body_markdown: String,
}

pub trait WebSearchProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn search(&self, query: &str, k: usize) -> Result<Vec<SearchHit>>;
}

/// 工厂：env 有 `BRAVE_API_KEY` 用 Brave，否则 DuckDuckGo HTML。
pub fn default_provider() -> Box<dyn WebSearchProvider> {
    if let Ok(key) = std::env::var("BRAVE_API_KEY") {
        if !key.is_empty() {
            return Box::new(BraveSearch::new(key));
        }
    }
    Box::new(DuckDuckGoHtml::default())
}

// ----------------- DuckDuckGo HTML -----------------

#[derive(Default)]
pub struct DuckDuckGoHtml {
    client: Client,
}

impl WebSearchProvider for DuckDuckGoHtml {
    fn name(&self) -> &'static str {
        "duckduckgo-html"
    }

    fn search(&self, query: &str, k: usize) -> Result<Vec<SearchHit>> {
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            url_encode(query)
        );
        let resp = self
            .client
            .get(&url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                 AppleWebKit/605.1.15 (KHTML, like Gecko) \
                 Version/16.0 Safari/605.1.15",
            )
            .timeout(Duration::from_secs(10))
            .send()
            .context("DuckDuckGo send")?;
        let status = resp.status();
        if !status.is_success() {
            return Err(anyhow!("DuckDuckGo HTTP {}", status));
        }
        let html = resp.text().context("DuckDuckGo body")?;
        parse_duckduckgo_html(&html, k)
    }
}

fn parse_duckduckgo_html(html: &str, k: usize) -> Result<Vec<SearchHit>> {
    let mut hits = Vec::new();
    let link_re = Regex::new(
        r#"<a[^>]*class="result__a"[^>]*href="([^"]+)"[^>]*>([\s\S]*?)</a>"#,
    )?;
    let snippet_re = Regex::new(r#"<a[^>]*class="result__snippet"[^>]*>([\s\S]*?)</a>"#)?;

    let mut urls_and_titles: Vec<(String, String)> = Vec::new();
    for cap in link_re.captures_iter(html) {
        let url = html_unescape(&cap[1]);
        let title = html_strip(&cap[2]);
        urls_and_titles.push((url, title));
    }
    let snippets: Vec<String> = snippet_re
        .captures_iter(html)
        .map(|c| html_strip(&c[1]))
        .collect();

    for (i, (url, title)) in urls_and_titles.into_iter().enumerate() {
        if hits.len() >= k {
            break;
        }
        let snippet = snippets.get(i).cloned().unwrap_or_default();
        let final_url = extract_real_url(&url);
        hits.push(SearchHit {
            title,
            url: final_url,
            snippet,
            body_markdown: String::new(),
        });
    }
    Ok(hits)
}

fn extract_real_url(href: &str) -> String {
    if !href.contains("/l/?") && !href.contains("uddg=") {
        return href.to_string();
    }
    if let Some(idx) = href.find("uddg=") {
        let rest = &href[idx + 5..];
        let stop = rest.find('&').unwrap_or(rest.len());
        let encoded = &rest[..stop];
        return url_decode(encoded);
    }
    href.to_string()
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .flat_map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                vec![b as char]
            } else {
                format!("%{:02X}", b).chars().collect()
            }
        })
        .collect()
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(c) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00"),
                16,
            ) {
                out.push(c);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn html_strip(s: &str) -> String {
    let no_tags = Regex::new(r"<[^>]+>").unwrap().replace_all(s, "").to_string();
    html_unescape(&no_tags)
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

// ----------------- Brave Search API -----------------

pub struct BraveSearch {
    api_key: String,
    client: Client,
}

impl BraveSearch {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct BraveResp {
    #[serde(default)]
    web: Option<BraveWeb>,
}

#[derive(Debug, Deserialize)]
struct BraveWeb {
    #[serde(default)]
    results: Vec<BraveResult>,
}

#[derive(Debug, Deserialize)]
struct BraveResult {
    title: String,
    url: String,
    #[serde(default)]
    description: String,
}

impl WebSearchProvider for BraveSearch {
    fn name(&self) -> &'static str {
        "brave"
    }

    fn search(&self, query: &str, k: usize) -> Result<Vec<SearchHit>> {
        let resp = self
            .client
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("X-Subscription-Token", &self.api_key)
            .query(&[("q", query), ("count", &k.to_string())])
            .timeout(Duration::from_secs(10))
            .send()
            .context("brave send")?;
        let status = resp.status();
        if !status.is_success() {
            let txt = resp.text().unwrap_or_default();
            return Err(anyhow!("Brave HTTP {}: {}", status, txt));
        }
        let parsed: BraveResp = resp.json().context("brave parse")?;
        let results = parsed.web.map(|w| w.results).unwrap_or_default();
        Ok(results
            .into_iter()
            .take(k)
            .map(|r| SearchHit {
                title: r.title,
                url: r.url,
                snippet: r.description,
                body_markdown: String::new(),
            })
            .collect())
    }
}

/// LLM 端可见的 web_search 工具的 dispatch。
pub fn tool_web_search(args: &serde_json::Value) -> Result<serde_json::Value> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("web_search: missing `query`"))?;
    let k = args.get("k").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
    let k = k.min(10);
    let provider = default_provider();
    match provider.search(query, k) {
        Ok(hits) => Ok(serde_json::json!({
            "query": query,
            "backend": provider.name(),
            "hits": hits.into_iter().map(|h| serde_json::json!({
                "title": h.title,
                "url": h.url,
                "snippet": h.snippet,
            })).collect::<Vec<_>>()
        })),
        Err(e) => Ok(serde_json::json!({
            "error": format!("{e:#}"),
            "query": query,
            "backend": provider.name(),
        })),
    }
}

/// LLM 端工具定义
pub fn web_search_definition() -> crate::llm::message::ToolDefinition {
    use crate::llm::message::ToolDefinition;
    use serde_json::json;
    ToolDefinition::from_json_schema(
        "web_search",
        "对公网做关键词搜索。默认 backend=DuckDuckGo（无需 key）；\
         若 env 有 BRAVE_API_KEY 自动切到 Brave Search API。\
         返回 hits 数组，每条含 title / url / snippet。",
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "搜索关键词" },
                "k": { "type": "integer", "description": "返回条数（默认 3，max 10）", "default": 3 }
            },
            "required": ["query"]
        }),
    )
}
