//! Streaming markdown filter — character-level state machine that strips
//! unsupported markdown syntax on-the-fly for `WeChat` output.
//!
//! Preserves: code fences, inline code, tables, horizontal rules, bold,
//! blockquotes, leading indent, italic/bold-italic wrapping non-CJK content.
//!
//! Strips markers but keeps content: italic/bold-italic wrapping CJK, H5/H6 headings.
//!
//! Removes entirely: images `![alt](url)`.

#![allow(clippy::module_name_repetitions)]

#[derive(Debug, Clone, PartialEq, Eq)]
enum InlineType {
    Image,
    Bold3,
    Italic,
    UBold3,
    UItalic,
}

#[derive(Debug, Clone)]
struct InlineState {
    typ: InlineType,
    acc: String,
}

/// A character-level streaming markdown filter that processes text incrementally.
///
/// Outputs as much filtered text as possible on each `feed()` call, only
/// holding back the minimum characters needed for pattern disambiguation.
#[derive(Debug, Clone)]
pub struct StreamingMarkdownFilter {
    buf: String,
    fence: bool,
    sol: bool,
    inl: Option<InlineState>,
}

impl Default for StreamingMarkdownFilter {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(
    clippy::too_many_lines,
    clippy::range_plus_one,
    clippy::manual_range_contains,
    clippy::naive_bytecount,
    clippy::manual_strip,
    clippy::let_and_return,
    clippy::doc_markdown,
    clippy::items_after_statements,
    clippy::cast_possible_truncation
)]
impl StreamingMarkdownFilter {
    /// Create a new filter in its initial state.
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            fence: false,
            sol: true,
            inl: None,
        }
    }

    /// Feed a chunk of text into the filter, returning any output that can be emitted.
    pub fn feed(&mut self, delta: &str) -> String {
        self.buf.push_str(delta);
        self.pump(false)
    }

    /// Flush remaining buffered content, emitting everything.
    pub fn flush(&mut self) -> String {
        self.pump(true)
    }

    fn pump(&mut self, eof: bool) -> String {
        let mut out = String::new();
        loop {
            if self.buf.is_empty() {
                break;
            }
            let s_len = self.buf.len();
            let s_sol = self.sol;
            let s_fence = self.fence;
            let s_inl_is_some = self.inl.is_some();

            if self.fence {
                out.push_str(&self.pump_fence(eof));
            } else if self.inl.is_some() {
                out.push_str(&self.pump_inline(eof));
            } else if self.sol {
                out.push_str(&self.pump_sol(eof));
            } else {
                out.push_str(&self.pump_body(eof));
            }

            if self.buf.len() == s_len
                && self.sol == s_sol
                && self.fence == s_fence
                && self.inl.is_some() == s_inl_is_some
            {
                break;
            }
        }

        if eof {
            if let Some(inl) = self.inl.take() {
                let marker = match inl.typ {
                    InlineType::Image => "![",
                    InlineType::Bold3 => "***",
                    InlineType::Italic => "*",
                    InlineType::UBold3 => "___",
                    InlineType::UItalic => "_",
                };
                out.push_str(marker);
                out.push_str(&inl.acc);
            }
        }
        out
    }

    fn pump_fence(&mut self, eof: bool) -> String {
        if self.sol {
            if self.buf.len() < 3 && !eof {
                return String::new();
            }
            if self.buf.starts_with("```") {
                if let Some(nl) = self.buf[3..].find('\n') {
                    let nl = nl + 3;
                    self.fence = false;
                    let line = self.buf[..nl + 1].to_string();
                    self.buf = self.buf[nl + 1..].to_string();
                    self.sol = true;
                    return line;
                }
                if eof {
                    self.fence = false;
                    let line = std::mem::take(&mut self.buf);
                    return line;
                }
                return String::new();
            }
            self.sol = false;
        }
        if let Some(nl) = self.buf.find('\n') {
            let chunk = self.buf[..nl + 1].to_string();
            self.buf = self.buf[nl + 1..].to_string();
            self.sol = true;
            return chunk;
        }
        let chunk = std::mem::take(&mut self.buf);
        chunk
    }

    fn pump_sol(&mut self, eof: bool) -> String {
        let b = self.buf.clone();
        let bytes = b.as_bytes();

        if bytes[0] == b'\n' {
            self.buf = b[1..].to_string();
            return "\n".to_string();
        }

        if bytes[0] == b'`' {
            if b.len() < 3 && !eof {
                return String::new();
            }
            if b.starts_with("```") {
                if let Some(nl) = b[3..].find('\n') {
                    let nl = nl + 3;
                    self.fence = true;
                    let line = b[..nl + 1].to_string();
                    self.buf = b[nl + 1..].to_string();
                    self.sol = true;
                    return line;
                }
                if eof {
                    self.buf = String::new();
                    return b;
                }
                return String::new();
            }
            self.sol = false;
            return String::new();
        }

        if bytes[0] == b'>' {
            self.sol = false;
            return String::new();
        }

        if bytes[0] == b'#' {
            let mut n = 0;
            while n < bytes.len() && bytes[n] == b'#' {
                n += 1;
            }
            if n == b.len() && !eof {
                return String::new();
            }
            if n >= 5 && n <= 6 && n < b.len() && bytes[n] == b' ' {
                self.buf = b[n + 1..].to_string();
                self.sol = false;
                return String::new();
            }
            self.sol = false;
            return String::new();
        }

        if bytes[0] == b' ' || bytes[0] == b'\t' {
            let non_ws = b.find(|c: char| c != ' ' && c != '\t');
            if non_ws.is_none() && !eof {
                return String::new();
            }
            self.sol = false;
            return String::new();
        }

        if bytes[0] == b'-' || bytes[0] == b'*' || bytes[0] == b'_' {
            let ch = bytes[0];
            let mut j = 0;
            while j < bytes.len() && (bytes[j] == ch || bytes[j] == b' ') {
                j += 1;
            }
            if j == b.len() && !eof {
                return String::new();
            }
            if j == b.len() || bytes[j] == b'\n' {
                let count = bytes[..j].iter().filter(|&&x| x == ch).count();
                if count >= 3 {
                    if j < b.len() {
                        self.buf = b[j + 1..].to_string();
                        self.sol = true;
                        return b[..j + 1].to_string();
                    }
                    self.buf = String::new();
                    return b;
                }
            }
            self.sol = false;
            return String::new();
        }

        self.sol = false;
        String::new()
    }

    fn pump_body(&mut self, eof: bool) -> String {
        let mut out = String::new();
        let chars: Vec<char> = self.buf.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let c = chars[i];
            if c == '\n' {
                out.push_str(&chars[..i + 1].iter().collect::<String>());
                self.buf = chars[i + 1..].iter().collect();
                self.sol = true;
                return out;
            }
            if c == '!' && i + 1 < chars.len() && chars[i + 1] == '[' {
                out.push_str(&chars[..i].iter().collect::<String>());
                self.buf = chars[i + 2..].iter().collect();
                self.inl = Some(InlineState {
                    typ: InlineType::Image,
                    acc: String::new(),
                });
                return out;
            }
            if c == '~' {
                i += 1;
                continue;
            }
            if c == '*' {
                if i + 2 < chars.len() && chars[i + 1] == '*' && chars[i + 2] == '*' {
                    out.push_str(&chars[..i].iter().collect::<String>());
                    self.buf = chars[i + 3..].iter().collect();
                    self.inl = Some(InlineState {
                        typ: InlineType::Bold3,
                        acc: String::new(),
                    });
                    return out;
                }
                if i + 1 < chars.len() && chars[i + 1] == '*' {
                    i += 2;
                    continue;
                }
                if i + 1 < chars.len() && chars[i + 1] != ' ' && chars[i + 1] != '\n' {
                    out.push_str(&chars[..i].iter().collect::<String>());
                    self.buf = chars[i + 1..].iter().collect();
                    self.inl = Some(InlineState {
                        typ: InlineType::Italic,
                        acc: String::new(),
                    });
                    return out;
                }
                i += 1;
                continue;
            }
            if c == '_' {
                if i + 2 < chars.len() && chars[i + 1] == '_' && chars[i + 2] == '_' {
                    out.push_str(&chars[..i].iter().collect::<String>());
                    self.buf = chars[i + 3..].iter().collect();
                    self.inl = Some(InlineState {
                        typ: InlineType::UBold3,
                        acc: String::new(),
                    });
                    return out;
                }
                if i + 1 < chars.len() && chars[i + 1] == '_' {
                    i += 2;
                    continue;
                }
                if i + 1 < chars.len() && chars[i + 1] != ' ' && chars[i + 1] != '\n' {
                    out.push_str(&chars[..i].iter().collect::<String>());
                    self.buf = chars[i + 1..].iter().collect();
                    self.inl = Some(InlineState {
                        typ: InlineType::UItalic,
                        acc: String::new(),
                    });
                    return out;
                }
                i += 1;
                continue;
            }
            i += 1;
        }

        let mut hold = 0;
        if !eof {
            let s: String = chars.iter().collect();
            if s.ends_with("**") || s.ends_with("__") {
                hold = 2;
            } else if s.ends_with('*') || s.ends_with('_') || s.ends_with('!') {
                hold = 1;
            }
        }
        let emit_len = chars.len() - hold;
        out.push_str(&chars[..emit_len].iter().collect::<String>());
        self.buf = if hold > 0 {
            chars[chars.len() - hold..].iter().collect()
        } else {
            String::new()
        };
        out
    }

    fn pump_inline(&mut self, _eof: bool) -> String {
        let Some(inl) = self.inl.as_mut() else {
            return String::new();
        };
        inl.acc.push_str(&self.buf);
        self.buf = String::new();

        let typ = inl.typ.clone();
        let acc = inl.acc.clone();

        match typ {
            InlineType::Bold3 => {
                if let Some(idx) = acc.find("***") {
                    let content = &acc[..idx];
                    self.buf = acc[idx + 3..].to_string();
                    let result = if Self::contains_cjk(content) {
                        content.to_string()
                    } else {
                        format!("***{content}***")
                    };
                    self.inl = None;
                    return result;
                }
                String::new()
            }
            InlineType::UBold3 => {
                if let Some(idx) = acc.find("___") {
                    let content = &acc[..idx];
                    self.buf = acc[idx + 3..].to_string();
                    let result = if Self::contains_cjk(content) {
                        content.to_string()
                    } else {
                        format!("___{content}___")
                    };
                    self.inl = None;
                    return result;
                }
                String::new()
            }
            InlineType::Italic => {
                let chars: Vec<char> = acc.chars().collect();
                for j in 0..chars.len() {
                    if chars[j] == '\n' {
                        let before: String = chars[..j + 1].iter().collect();
                        let after: String = chars[j + 1..].iter().collect();
                        self.buf = after;
                        self.inl = None;
                        self.sol = true;
                        return format!("*{before}");
                    }
                    if chars[j] == '*' {
                        if j + 1 < chars.len() && chars[j + 1] == '*' {
                            continue;
                        }
                        let content: String = chars[..j].iter().collect();
                        self.buf = chars[j + 1..].iter().collect();
                        self.inl = None;
                        return if Self::contains_cjk(&content) {
                            content
                        } else {
                            format!("*{content}*")
                        };
                    }
                }
                String::new()
            }
            InlineType::UItalic => {
                let chars: Vec<char> = acc.chars().collect();
                for j in 0..chars.len() {
                    if chars[j] == '\n' {
                        let before: String = chars[..j + 1].iter().collect();
                        let after: String = chars[j + 1..].iter().collect();
                        self.buf = after;
                        self.inl = None;
                        self.sol = true;
                        return format!("_{before}");
                    }
                    if chars[j] == '_' {
                        if j + 1 < chars.len() && chars[j + 1] == '_' {
                            continue;
                        }
                        let content: String = chars[..j].iter().collect();
                        self.buf = chars[j + 1..].iter().collect();
                        self.inl = None;
                        return if Self::contains_cjk(&content) {
                            content
                        } else {
                            format!("_{content}_")
                        };
                    }
                }
                String::new()
            }
            InlineType::Image => {
                if let Some(cb) = acc.find(']') {
                    if cb + 1 >= acc.len() {
                        return String::new();
                    }
                    if acc.as_bytes()[cb + 1] != b'(' {
                        let r = format!("![{}", &acc[..cb + 1]);
                        self.buf = acc[cb + 1..].to_string();
                        self.inl = None;
                        return r;
                    }
                    if let Some(cp) = acc[cb + 2..].find(')') {
                        let cp = cp + cb + 2;
                        self.buf = acc[cp + 1..].to_string();
                        self.inl = None;
                        return String::new();
                    }
                }
                String::new()
            }
        }
    }

    fn contains_cjk(text: &str) -> bool {
        text.chars().any(|c| {
            ('\u{2E80}'..='\u{9FFF}').contains(&c)
                || ('\u{AC00}'..='\u{D7AF}').contains(&c)
                || ('\u{F900}'..='\u{FAFF}').contains(&c)
        })
    }
}

/// Filter markdown from a complete text string, stripping unsupported syntax for `WeChat`.
pub fn filter_markdown(text: &str) -> String {
    let mut f = StreamingMarkdownFilter::new();
    let mut out = f.feed(text);
    out.push_str(&f.flush());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text() {
        assert_eq!(filter_markdown("hello world"), "hello world");
    }

    #[test]
    fn code_fence() {
        let input = "```rust\nfn main() {}\n```\n";
        assert_eq!(filter_markdown(input), input);
    }

    #[test]
    fn bold_preserved() {
        assert_eq!(filter_markdown("**bold**"), "**bold**");
    }

    #[test]
    fn image_stripping() {
        assert_eq!(
            filter_markdown("before ![alt](http://img.png) after"),
            "before  after"
        );
    }

    #[test]
    fn cjk_italic() {
        assert_eq!(filter_markdown("*你好*"), "你好");
    }

    #[test]
    fn non_cjk_italic() {
        assert_eq!(filter_markdown("*hello*"), "*hello*");
    }

    #[test]
    fn cjk_bold_italic() {
        assert_eq!(filter_markdown("***你好***"), "你好");
    }

    #[test]
    fn non_cjk_bold_italic() {
        assert_eq!(filter_markdown("***hello***"), "***hello***");
    }

    #[test]
    fn underscore_italic_cjk() {
        assert_eq!(filter_markdown("_你好_"), "你好");
    }

    #[test]
    fn underscore_bold_italic_cjk() {
        assert_eq!(filter_markdown("___你好___"), "你好");
    }

    #[test]
    fn non_cjk_underscore_italic() {
        assert_eq!(filter_markdown("_hello_"), "_hello_");
    }

    #[test]
    fn h5_heading() {
        assert_eq!(filter_markdown("##### Title"), "Title");
    }

    #[test]
    fn h6_heading() {
        assert_eq!(filter_markdown("###### Title"), "Title");
    }

    #[test]
    fn table_preserved() {
        let input = "| a | b |\n| - | - |\n| 1 | 2 |\n";
        assert_eq!(filter_markdown(input), input);
    }

    #[test]
    fn horizontal_rule() {
        assert_eq!(filter_markdown("---\n"), "---\n");
        assert_eq!(filter_markdown("***\n"), "***\n");
        assert_eq!(filter_markdown("___\n"), "___\n");
    }

    #[test]
    fn streaming_incremental() {
        let mut f = StreamingMarkdownFilter::new();
        let mut out = String::new();
        out.push_str(&f.feed("hel"));
        out.push_str(&f.feed("lo world"));
        out.push_str(&f.flush());
        assert_eq!(out, "hello world");
    }

    #[test]
    fn blockquote_preservation() {
        // The > marker is preserved (sol transitions to body, content passes through)
        let result = filter_markdown("> quote text");
        assert_eq!(result, "> quote text");
    }

    #[test]
    fn indent_preservation() {
        // Leading whitespace is preserved (sol transitions to body, content passes through)
        let result = filter_markdown("    indented");
        assert_eq!(result, "    indented");
    }
}
