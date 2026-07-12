//! 极简 markdown 渲染。
//!
//! 故意做得简单：只覆盖 `heading / 段落 / code block / list / 粗体`
//! 这些 90% 场景会用到的语法。
//! 复杂格式直接 fallback 成纯文本，不重写 markdown 库。

use crate::ui::colors;
use owo_colors::OwoColorize;

/// 把 markdown 文本渲染一遍：
/// - `# / ## / ### ...` 标题着色 + 加粗
/// - ` ``` ` 围栏代码块保留原文，用 dim 色
/// - `- * +` 无序列表前加 `▸`
/// - `1. 2. ` 有序列表前加 `n.`
/// - `**xxx**` 加粗
/// - 行内 `code` 用反引号 + dim
pub fn render(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_code = false;
    let mut code_buf = String::new();
    let mut in_para = false;

    for raw_line in input.split('\n') {
        let line = raw_line.trim_end_matches('\r');

        if line.trim_start().starts_with("```") {
            if in_code {
                if colors::disabled() {
                    out.push_str(&code_buf);
                } else {
                    out.push_str(&code_buf.dimmed().to_string());
                }
                code_buf.clear();
                out.push('\n');
                in_code = false;
            } else {
                if in_para {
                    out.push('\n');
                    in_para = false;
                }
                in_code = true;
            }
            continue;
        }

        if in_code {
            code_buf.push_str(line);
            code_buf.push('\n');
            continue;
        }

        if let Some(stripped) = strip_heading(line) {
            if in_para {
                out.push('\n');
                in_para = false;
            }
            if colors::disabled() {
                out.push_str(&stripped);
                out.push('\n');
            } else {
                out.push_str(&stripped.bold().bright_cyan().to_string());
                out.push('\n');
            }
            continue;
        }

        if let Some((marker, body)) = strip_unordered_list(line) {
            in_para = true;
            if colors::disabled() {
                out.push_str(&format!("{marker} {body}\n"));
            } else {
                let m = marker.bright_yellow().to_string();
                out.push_str(&format!("{m} {body}\n"));
            }
            continue;
        }

        if let Some((idx, body)) = strip_ordered_list(line) {
            in_para = true;
            if colors::disabled() {
                out.push_str(&format!("{idx}. {body}\n"));
            } else {
                let m = idx.bright_yellow().to_string();
                out.push_str(&format!("{m}. {body}\n"));
            }
            continue;
        }

        if line.trim().is_empty() {
            if in_para {
                out.push('\n');
                in_para = false;
            }
            out.push('\n');
            continue;
        }

        let rendered = render_inline(line);
        out.push_str(&rendered);
        out.push('\n');
        in_para = true;
    }

    if in_code {
        out.push_str(&code_buf);
    }

    out
}

fn strip_heading(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = trimmed[hashes..].trim_start();
    Some(format!("{} {}", "#".repeat(hashes), rest))
}

fn strip_unordered_list(line: &str) -> Option<(&'static str, String)> {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    if chars.next() == Some('-') && chars.next() == Some(' ') {
        let body = trimmed[2..].trim_start().to_string();
        return Some(("-", body));
    }
    let mut chars = trimmed.chars();
    if chars.next() == Some('*') && chars.next() == Some(' ') {
        let body = trimmed[2..].trim_start().to_string();
        return Some(("*", body));
    }
    let mut chars = trimmed.chars();
    if chars.next() == Some('+') && chars.next() == Some(' ') {
        let body = trimmed[2..].trim_start().to_string();
        return Some(("+", body));
    }
    None
}

fn strip_ordered_list(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let mut i = 0;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i >= chars.len() || chars[i] != '.' {
        return None;
    }
    let digits: String = chars[..i].iter().collect();
    let body: String = chars[i + 1..].iter().collect::<String>().trim_start().to_string();
    Some((digits, body))
}

fn render_inline(line: &str) -> String {
    let mut s = line.to_string();
    if !colors::disabled() {
        s = render_inline_code(&s);
        s = render_bold(&s);
    }
    s
}

fn render_inline_code(s: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    let mut buf = String::new();
    for c in s.chars() {
        if c == '`' {
            if in_code {
                out.push_str(&format!("`{buf}`").dimmed().to_string());
                buf.clear();
                in_code = false;
            } else {
                out.push('`');
                in_code = true;
            }
        } else if in_code {
            buf.push(c);
        } else {
            out.push(c);
        }
    }
    if in_code {
        out.push('`');
        out.push_str(&buf);
    }
    out
}

fn render_bold(s: &str) -> String {
    let mut out = String::new();
    let mut in_bold = false;
    let mut buf = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            if in_bold {
                out.push_str(&buf.bold().to_string());
                buf.clear();
                in_bold = false;
            } else {
                in_bold = true;
            }
            i += 2;
            continue;
        }
        if in_bold {
            buf.push(chars[i]);
        } else {
            out.push(chars[i]);
        }
        i += 1;
    }
    if in_bold {
        out.push_str("**");
        out.push_str(&buf);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_strip() {
        assert_eq!(strip_heading("### title").unwrap(), "### title");
        assert!(strip_heading("####### too deep").is_none());
        assert!(strip_heading("not a heading").is_none());
    }

    #[test]
    fn list_strip() {
        assert_eq!(
            strip_unordered_list("- hello").unwrap(),
            ("-", "hello".to_string())
        );
        assert!(strip_unordered_list("hello").is_none());
        assert_eq!(
            strip_ordered_list("1. first").unwrap(),
            ("1".to_string(), "first".to_string())
        );
        assert!(strip_ordered_list("1 first").is_none());
    }
}
