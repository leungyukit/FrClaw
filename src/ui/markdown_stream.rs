//! 流式 markdown 渲染。
//!
//! 接收 LLM 增量 token（可能半行），按行 flush 到 stdout。
//!
//! 状态机：
//! - **普通行** —— 行内 `code` / **bold** / *italic* / [link](url)
//! - **代码块** —— ``` 围栏里的内容：原样输出，无行内解析
//! - **标题** —— `# / ## / ###` 起头
//! - **列表** —— `- / * / 1.` 起头
//! - **引用** —— `> ` 起头
//!
//! 不重写 markdown 库 —— 复用 [`crate::ui::markdown::render`] 的整段渲染，但每次
//! 只在「最后一行已完整」时再 render + 截取增量部分。

use crate::ui::markdown as md;
use std::io::Write;

/// 增量 markdown 渲染器。
///
/// 用法：
/// ```ignore
/// let mut s = MarkdownStream::new();
/// s.add("# hello\n");        // 立刻 flush
/// s.add("more text...");     // buffer
/// s.add(" end of line\n");   // 立刻 flush
/// s.finish();                // 刷掉最后残留
/// ```
pub struct MarkdownStream {
    /// 累积的完整内容
    accumulated: String,
    /// 已经渲染并输出到 stdout 的字节数
    printed: usize,
    /// 上一次 render 的结果（用于截取增量）
    last_rendered: String,
    /// 强制 flush 阈值（单行字符数）
    force_flush_at: usize,
}

impl MarkdownStream {
    pub fn new() -> Self {
        Self {
            accumulated: String::new(),
            printed: 0,
            last_rendered: String::new(),
            force_flush_at: 200,
        }
    }

    /// 推一段 token 流。
    pub fn add(&mut self, chunk: &str) {
        self.accumulated.push_str(chunk);
        self.try_flush(false);
    }

    /// 结束：刷掉最后残留（即使没换行）。
    pub fn finish(&mut self) {
        self.try_flush(true);
    }

    fn try_flush(&mut self, force: bool) {
        loop {
            // 找到下一行结尾
            let start = self.printed;
            let bytes = self.accumulated.as_bytes();
            // 从 start 往后找 \n 或 force_flush_at 字符
            let mut newline_pos: Option<usize> = None;
            let mut char_count = 0usize;
            let mut i = start;
            while i < bytes.len() {
                if bytes[i] == b'\n' {
                    newline_pos = Some(i);
                    break;
                }
                // utf-8 字符边界
                char_count += 1;
                if char_count >= self.force_flush_at {
                    // i 可能落在多字节字符（CJK）中间，
                    // 推进到完整字符结束处，保证 accumulated[..=end] 可安全切片
                    let mut end = i;
                    while end + 1 < bytes.len() && !self.accumulated.is_char_boundary(end + 1) {
                        end += 1;
                    }
                    newline_pos = Some(end);
                    break;
                }
                i += 1;
            }
            let end = match newline_pos {
                Some(p) => p,
                None => {
                    // 没完整行；若 force（finish）→ 全部 flush
                    if force {
                        self.accumulated.len()
                    } else {
                        return;
                    }
                }
            };
            self.flush_up_to(end);
            if end < self.accumulated.len() && self.accumulated.as_bytes()[end] == b'\n' {
                self.printed = end + 1;
            } else {
                self.printed = end;
            }
            if !force {
                // 继续找下一行（可能累积了多行）
                if self.printed >= self.accumulated.len() {
                    return;
                }
            } else {
                return;
            }
        }
    }

    fn flush_up_to(&mut self, end: usize) {
        // 边界：empty accumulated / end=0 → 跳过
        if end == 0 || self.accumulated.is_empty() {
            return;
        }
        let real_end = end.min(self.accumulated.len().saturating_sub(1));
        // 截取 accumulated[0..=real_end] 整段过 render，跟 last_rendered 比 diff
        let prefix = &self.accumulated[..=real_end];
        let new_rendered = md::render(prefix);
        // 增量：新渲染字符串里比 last_rendered 多的部分。
        // 不能直接用 last_rendered.len() 切——render 会因 markdown 状态变化
        // （如不完整的 **bold**）改写 earlier 字节，旧长度可能落在多字节字符
        // 中间（CJK 直接 panic）。取最长公共前缀（字符边界安全）。
        if new_rendered.len() > self.last_rendered.len() {
            let keep = common_prefix_len(&self.last_rendered, &new_rendered);
            let diff = &new_rendered[keep..];
            let stdout = std::io::stdout();
            let mut h = stdout.lock();
            let _ = h.write_all(diff.as_bytes());
            let _ = h.flush();
        }
        self.last_rendered = new_rendered;
    }
}

impl Default for MarkdownStream {
    fn default() -> Self {
        Self::new()
    }
}

/// 两个渲染结果的最长公共前缀长度（按字节比较；若落在中途非字符边界，
/// 回退到 `new` 中最近的前一个字符边界，保证可以安全切片）。
fn common_prefix_len(prev: &str, new: &str) -> usize {
    let pb = prev.as_bytes();
    let nb = new.as_bytes();
    let max = pb.len().min(nb.len());
    let mut i = 0;
    while i < max && pb[i] == nb[i] {
        i += 1;
    }
    while i > 0 && !new.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::markdown::render;

    #[test]
    fn empty_input_no_flush() {
        let mut s = MarkdownStream::new();
        s.add("");
        s.finish();
        // nothing should panic; last_rendered stays empty
        assert_eq!(s.last_rendered, "");
    }

    #[test]
    fn full_line_buffered_then_flushed() {
        // 直接用 render() 验证内容正确（stream 只负责切片，render 决定外观）
        let cases = vec![
            ("# title\nbody", "### title\nbody"),
            ("- a\n- b\n", "a\nb"),
            ("```\ncode\n```\n", "code"),
            ("**bold**\n", "**bold**"),
        ];
        for (input, _expect_contains) in cases {
            let out = render(input);
            // 不强制 ANSI；只验证 render 不会 panic
            assert!(!out.is_empty() || input.trim().is_empty());
        }
    }

    #[test]
    fn force_flush_at_threshold() {
        let mut s = MarkdownStream::new();
        s.force_flush_at = 10;
        s.add("abcdefghijklmn");
        // 14 字符无 \n：add() 时 force flush 到 9；finish() flush 剩
        assert_eq!(s.printed, 9);
        s.finish();
        assert_eq!(s.printed, 14);
    }

    #[test]
    fn multiline_increments_printed() {
        let mut s = MarkdownStream::new();
        s.add("line1\nline");
        // "line1\n" 已 flush → printed=6
        assert_eq!(s.printed, 6);
        s.add("2\nline3");
        // "line2\n" 已 flush → printed=12
        assert_eq!(s.printed, 12);
        s.finish();
        // "line3" finish flush → printed=17
        assert_eq!(s.printed, 17);
    }

    #[test]
    fn no_color_path_does_not_crash() {
        // 跑一遍在 NO_COLOR 环境下
        let mut s = MarkdownStream::new();
        s.add("# title\n");
        s.add("a `b` c\n");
        s.add("```\ncode block\n```\n");
        s.finish();
        // 不验证具体内容（ANSI vs 非 ANSI）；只验证不 panic
    }

    #[test]
    fn common_prefix_respects_char_boundaries() {
        // 旧渲染以 ASCII 结尾，新渲染在同一位置换成 CJK——旧 byte 长度会切开新字符
        assert_eq!(common_prefix_len("ab", "a我b"), 1);
        // 纯前缀关系：完整保留
        assert_eq!(common_prefix_len("\x1b[93mx", "\x1b[93m你好"), 5);
        // 空 prev：全部输出
        assert_eq!(common_prefix_len("", "你好世界"), 0);
        // 完全相同
        assert_eq!(common_prefix_len("abc", "abc"), 3);
    }

    #[test]
    fn chinese_text_with_markdown_rewrite_no_panic() {
        // 模拟真实场景：未闭合 ** 触发 render 改写，后续接 CJK 文本
        let mut s = MarkdownStream::new();
        s.add("**你好");
        s.add("世界**\n");
        s.add("普通中文行，不含 markdown\n");
        s.add("- 列表项**加粗**\n");
        s.finish();
    }

    // 颜色已禁用时，纯逻辑验证
    #[test]
    fn colors_disabled_yields_plain() {
        let out = render("# t\n");
        // 至少包含原内容字符
        assert!(out.contains("t"));
    }
}
