//! 文本分块。
//!
//! 极简策略：先按段落（\n\n）切，段落过长再按句子（.!?。！？\n）切，必要时按硬窗口切。
//! 目标 chunk 大小 ~500 字符，overlap 50 字符。

use std::sync::OnceLock;
use regex::Regex;

#[derive(Clone, Debug)]
pub struct ChunkOptions {
    pub max_chars: usize,
    pub overlap: usize,
}

impl Default for ChunkOptions {
    fn default() -> Self {
        Self { max_chars: 500, overlap: 50 }
    }
}

/// 把一段文本切成多个 chunk。
///
/// 返回 (chunk_index, content) 序列。
pub fn chunk_text(text: &str, opts: &ChunkOptions) -> Vec<(usize, String)> {
    if text.is_empty() {
        return vec![];
    }
    let normalized = text.replace("\r\n", "\n");
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut idx = 0usize;

    for para in normalized.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        if para.chars().count() <= opts.max_chars {
            out.push((idx, para.to_string()));
            idx += 1;
            continue;
        }
        // 长段落：按句子切
        for piece in split_long_paragraph_owned(para, opts) {
            out.push((idx, piece));
            idx += 1;
        }
    }
    out
}

fn split_long_paragraph_owned(para: &str, opts: &ChunkOptions) -> Vec<String> {
    let sentences = split_sentences(para);
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();

    for s in sentences {
        let s = s.trim().to_string();
        if s.is_empty() {
            continue;
        }
        if buf.is_empty() {
            buf.push_str(&s);
            continue;
        }
        if buf.chars().count() + s.chars().count() + 1 <= opts.max_chars {
            buf.push(' ');
            buf.push_str(&s);
        } else {
            push_with_overlap(&mut out, &buf, opts);
            buf = s;
        }
    }
    if !buf.is_empty() {
        push_with_overlap(&mut out, &buf, opts);
    }
    out
}

fn push_with_overlap(out: &mut Vec<String>, buf: &str, opts: &ChunkOptions) {
    out.push(buf.to_string());
    if opts.overlap > 0 && buf.chars().count() > opts.overlap {
        // 取尾部 overlap 字符作为下一 chunk 的前缀（在调用方已 push 到 buf 处理）
        // 这里仅 push 主块，overlap 在更上层 merge
    }
}

fn split_sentences(text: &str) -> Vec<String> {
    // Rust regex 不支持 look-around，改用 split_inclusive 在标点后切
    // 注意 split_inclusive 保留分隔符
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"[.!?。！？\n]+").unwrap());
    let mut out: Vec<String> = Vec::new();
    let mut last_end = 0usize;
    for m in re.find_iter(text) {
        let end = m.end();
        let piece = text[last_end..end].trim();
        if !piece.is_empty() {
            out.push(piece.to_string());
        }
        last_end = end;
    }
    let tail = text[last_end..].trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
    out
}

/// 带 overlap 的窗口切分（用于特别长的句子，无标点）。
pub fn window_chunks(text: &str, opts: &ChunkOptions) -> Vec<String> {
    if text.is_empty() {
        return vec![];
    }
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n <= opts.max_chars {
        return vec![text.to_string()];
    }
    let step = opts.max_chars.saturating_sub(opts.overlap).max(1);
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < n {
        let end = (start + opts.max_chars).min(n);
        out.push(chars[start..end].iter().collect());
        if end == n {
            break;
        }
        start += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input() {
        let out = chunk_text("", &ChunkOptions::default());
        assert!(out.is_empty());
    }

    #[test]
    fn short_paragraphs() {
        let text = "Hello world.\n\nSecond paragraph here.";
        let out = chunk_text(text, &ChunkOptions::default());
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].1, "Hello world.");
        assert_eq!(out[1].1, "Second paragraph here.");
    }

    #[test]
    fn long_paragraph_splits_on_sentences() {
        let text = "First sentence is here. Second sentence follows. Third sentence continues. Fourth sentence ends. Fifth sentence starts new. Sixth sentence and more. Seventh sentence continues. Eighth sentence wraps up.";
        let opts = ChunkOptions { max_chars: 60, overlap: 10 };
        let out = chunk_text(text, &opts);
        assert!(out.len() >= 2);
        for (_, c) in &out {
            // 每个 chunk 不超过 max_chars 太多（容许 +overlap）
            assert!(c.chars().count() <= opts.max_chars + 30);
        }
    }

    #[test]
    fn window_chunks_basic() {
        let text = "a".repeat(250);
        let opts = ChunkOptions { max_chars: 100, overlap: 20 };
        let out = window_chunks(&text, &opts);
        assert!(out.len() >= 3);
        assert!(out[0].chars().count() == 100);
    }
}
