//! 语法着色与轻量诊断：**纯函数，零 IO**。
//!
//! 这里不做完整的语法分析，只做编辑器需要的最小集合：
//! - 按行给出 token 区间（关键字 / 字符串 / 注释 / 数字 / 标注 / 标签），供 UI 着色
//! - 检查括号与引号是否配对，供 UI 标红
//!
//! 不变量：
//! - 返回的 [`Token`] 区间**按字节偏移**且互不重叠、按起点升序。
//! - token 之外的区间是「普通文本」，不需要单独返回。
//! - 所有偏移都落在 `char` 边界上，UI 切片不会 panic。

use std::path::Path;

/// 识别到的语言。只用于选择着色规则与关键字表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Java,
    JavaScript,
    Python,
    Shell,
    Toml,
    Json,
    Xml,
    Markdown,
    /// 未识别：只做括号检查，不着色。
    Plain,
}

impl Language {
    /// 按文件扩展名推断语言。
    pub fn from_path(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "rs" => Self::Rust,
            "java" => Self::Java,
            "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" => Self::JavaScript,
            "py" => Self::Python,
            "sh" | "bash" | "zsh" => Self::Shell,
            "toml" => Self::Toml,
            "json" => Self::Json,
            "xml" | "html" | "htm" | "svg" | "xhtml" => Self::Xml,
            "md" | "markdown" => Self::Markdown,
            _ => Self::Plain,
        }
    }

    /// 展示名，用于编辑器标题栏。
    pub fn label(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Java => "Java",
            Self::JavaScript => "JavaScript",
            Self::Python => "Python",
            Self::Shell => "Shell",
            Self::Toml => "TOML",
            Self::Json => "JSON",
            Self::Xml => "XML",
            Self::Markdown => "Markdown",
            Self::Plain => "纯文本",
        }
    }

    /// 使用 `//` 行注释与 `/* */` 块注释。
    fn c_style_comments(self) -> bool {
        matches!(self, Self::Rust | Self::Java | Self::JavaScript)
    }

    /// 使用 `#` 行注释。
    fn hash_comments(self) -> bool {
        matches!(self, Self::Python | Self::Shell | Self::Toml)
    }

    /// 需要做括号配对检查的语言。
    pub fn uses_brackets(self) -> bool {
        matches!(
            self,
            Self::Rust | Self::Java | Self::JavaScript | Self::Python | Self::Json | Self::Xml
        )
    }

    /// 关键字表。
    fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
                "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
                "super", "trait", "true", "type", "unsafe", "use", "where", "while",
            ],
            Self::Java => &[
                "abstract",
                "assert",
                "boolean",
                "break",
                "byte",
                "case",
                "catch",
                "char",
                "class",
                "const",
                "continue",
                "default",
                "do",
                "double",
                "else",
                "enum",
                "extends",
                "false",
                "final",
                "finally",
                "float",
                "for",
                "if",
                "implements",
                "import",
                "instanceof",
                "int",
                "interface",
                "long",
                "native",
                "new",
                "null",
                "package",
                "private",
                "protected",
                "public",
                "return",
                "short",
                "static",
                "strictfp",
                "super",
                "switch",
                "synchronized",
                "this",
                "throw",
                "throws",
                "transient",
                "true",
                "try",
                "var",
                "void",
                "volatile",
                "while",
            ],
            Self::JavaScript => &[
                "async",
                "await",
                "break",
                "case",
                "catch",
                "class",
                "const",
                "continue",
                "default",
                "delete",
                "do",
                "else",
                "export",
                "extends",
                "false",
                "finally",
                "for",
                "from",
                "function",
                "if",
                "import",
                "in",
                "instanceof",
                "let",
                "new",
                "null",
                "return",
                "super",
                "switch",
                "this",
                "throw",
                "true",
                "try",
                "typeof",
                "undefined",
                "var",
                "void",
                "while",
                "yield",
            ],
            Self::Python => &[
                "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
                "del", "elif", "else", "except", "False", "finally", "for", "from", "global", "if",
                "import", "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise",
                "return", "True", "try", "while", "with", "yield",
            ],
            Self::Shell => &[
                "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function",
                "if", "in", "local", "return", "then", "while",
            ],
            Self::Toml => &["true", "false"],
            Self::Json => &["true", "false", "null"],
            Self::Xml | Self::Markdown | Self::Plain => &[],
        }
    }
}

/// token 类别，UI 据此取颜色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Keyword,
    /// 字符串与字符字面量。
    Str,
    Comment,
    Number,
    /// Java 注解、Rust 属性 `#[...]` 里的名字。
    Annotation,
    /// XML 标签名。
    Tag,
    /// XML 属性名。
    Attribute,
    /// Markdown 标题 / 强调 / 行内代码 / 链接。
    Heading,
    InlineCode,
    Link,
}

/// 一个着色区间（字节偏移，左闭右开）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

impl Token {
    fn new(start: usize, end: usize, kind: TokenKind) -> Self {
        Self { start, end, kind }
    }
}

/// 跨行状态：块注释是否未闭合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct State {
    in_block_comment: bool,
}

impl State {
    /// 当前是否处在未闭合的块注释里（供 UI 判断行尾样式）。
    pub fn in_block_comment(self) -> bool {
        self.in_block_comment
    }
}

/// 给整个文件逐行着色。
///
/// 返回长度等于 `text.lines()` 的行数的向量，每项是该行的 token 列表。
pub fn highlight(text: &str, lang: Language) -> Vec<Vec<Token>> {
    let mut state = State::default();
    text.lines()
        .map(|line| highlight_line(line, lang, &mut state))
        .collect()
}

/// 给单行着色，`state` 承载跨行块注释。
pub fn highlight_line(line: &str, lang: Language, state: &mut State) -> Vec<Token> {
    let bytes = line.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0usize;

    // 上一行遗留的块注释
    if state.in_block_comment && lang.c_style_comments() {
        match find(bytes, 0, b"*/") {
            Some(end) => {
                tokens.push(Token::new(0, end + 2, TokenKind::Comment));
                state.in_block_comment = false;
                i = end + 2;
            }
            None => {
                if !line.is_empty() {
                    tokens.push(Token::new(0, bytes.len(), TokenKind::Comment));
                }
                return tokens;
            }
        }
    }

    if lang == Language::Markdown {
        return highlight_markdown(line);
    }
    if lang == Language::Xml {
        return highlight_markup(line);
    }

    while i < bytes.len() {
        let rest = &line[i..];

        // 块注释
        if rest.starts_with("/*") {
            match find(bytes, i + 2, b"*/") {
                Some(end) => {
                    tokens.push(Token::new(i, end + 2, TokenKind::Comment));
                    i = end + 2;
                }
                None => {
                    tokens.push(Token::new(i, bytes.len(), TokenKind::Comment));
                    state.in_block_comment = true;
                    return tokens;
                }
            }
            continue;
        }

        // 行注释
        if (lang.c_style_comments() && rest.starts_with("//"))
            || (lang.hash_comments() && rest.starts_with('#'))
        {
            tokens.push(Token::new(i, bytes.len(), TokenKind::Comment));
            return tokens;
        }

        let ch = next_char(line, i);

        // 字符串
        if ch == '"' || ch == '\'' {
            let (end, _) = scan_string(bytes, i, ch);
            tokens.push(Token::new(i, end, TokenKind::Str));
            i = end;
            continue;
        }

        // 注解 / 属性
        if ch == '@'
            && lang == Language::Java
            && let Some(len) = ident_len(bytes, i + 1)
            && len > 0
        {
            tokens.push(Token::new(i, i + 1 + len, TokenKind::Annotation));
            i += 1 + len;
            continue;
        }

        // 数字
        if ch.is_ascii_digit() {
            let end = scan_number(bytes, i);
            tokens.push(Token::new(i, end, TokenKind::Number));
            i = end;
            continue;
        }

        // 标识符 / 关键字
        if ch.is_alphabetic() || ch == '_' {
            let len = ident_len(bytes, i).unwrap_or(0);
            if len > 0 {
                let word = &line[i..i + len];
                if lang.keywords().contains(&word) {
                    tokens.push(Token::new(i, i + len, TokenKind::Keyword));
                }
                i += len;
                continue;
            }
        }

        i += ch.len_utf8();
    }

    tokens
}

/// Markdown 是逐行独立的，没有跨行状态。
fn highlight_markdown(line: &str) -> Vec<Token> {
    let bytes = line.as_bytes();
    let mut tokens = Vec::new();
    let trimmed_start = line.len() - line.trim_start().len();

    // 标题
    if line.trim_start().starts_with('#') {
        return vec![Token::new(0, bytes.len(), TokenKind::Heading)];
    }

    let mut i = 0usize;
    while i < bytes.len() {
        let ch = next_char(line, i);
        // 行内代码
        if ch == '`' {
            if let Some(end) = line[i + 1..].find('`') {
                let stop = i + 1 + end + 1;
                tokens.push(Token::new(i, stop, TokenKind::InlineCode));
                i = stop;
                continue;
            }
            i += 1;
            continue;
        }
        // 链接 [text](url)
        if ch == '[' {
            if let Some(close) = line[i + 1..].find(']') {
                let after = i + 1 + close + 1;
                if line[after..].starts_with('(')
                    && let Some(paren) = line[after + 1..].find(')')
                {
                    let stop = after + 1 + paren + 1;
                    tokens.push(Token::new(i, stop, TokenKind::Link));
                    i = stop;
                    continue;
                }
            }
        }
        // 列表标记
        if (ch == '-' || ch == '*' || ch == '+')
            && i == trimmed_start
            && line[i + 1..].starts_with(' ')
        {
            tokens.push(Token::new(i, i + 1, TokenKind::Keyword));
        }
        i += ch.len_utf8();
    }
    tokens
}

/// XML / HTML：标签名、属性名、字符串、注释。
fn highlight_markup(line: &str) -> Vec<Token> {
    let bytes = line.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        if line[i..].starts_with("<!--") {
            match line[i..].find("-->") {
                Some(rel) => {
                    tokens.push(Token::new(i, i + rel + 3, TokenKind::Comment));
                    i += rel + 3;
                }
                None => {
                    tokens.push(Token::new(i, bytes.len(), TokenKind::Comment));
                    return tokens;
                }
            }
            continue;
        }

        let ch = next_char(line, i);
        if ch == '<' {
            // <name 或 </name
            let name_start = if line[i + 1..].starts_with('/') {
                i + 2
            } else {
                i + 1
            };
            let len = ident_len(bytes, name_start).unwrap_or(0);
            if len > 0 {
                tokens.push(Token::new(name_start, name_start + len, TokenKind::Tag));
                i = name_start + len;
            } else {
                i += 1;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            let (end, _) = scan_string(bytes, i, ch);
            tokens.push(Token::new(i, end, TokenKind::Str));
            i = end;
            continue;
        }
        // 属性名：出现在标签内部、后接 `=`
        if ch.is_alphabetic() || ch == '_' {
            let len = ident_len(bytes, i).unwrap_or(0);
            if len > 0 {
                let after = i + len;
                if line[after..].trim_start().starts_with('=') {
                    tokens.push(Token::new(i, after, TokenKind::Attribute));
                }
                i = after;
                continue;
            }
        }
        i += ch.len_utf8();
    }
    tokens
}

/// 取 `i` 处的字符；越界时返回空格，避免 panic。
fn next_char(s: &str, i: usize) -> char {
    s.get(i..).and_then(|r| r.chars().next()).unwrap_or(' ')
}

/// 从 `from` 开始找字节串，返回起始下标。
fn find(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from >= haystack.len() || needle.is_empty() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// 从 `start` 开始的标识符长度（ASCII 字母 / 数字 / 下划线）。
fn ident_len(bytes: &[u8], start: usize) -> Option<usize> {
    if start >= bytes.len() {
        return None;
    }
    let mut i = start;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_alphanumeric() || b == b'_' {
            i += 1;
        } else {
            break;
        }
    }
    Some(i - start)
}

/// 扫描字符串直到配对引号；返回结束下标与是否闭合。
fn scan_string(bytes: &[u8], start: usize, quote: char) -> (usize, bool) {
    let q = quote as u8;
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b if b == q => return (i + 1, true),
            _ => i += 1,
        }
    }
    (bytes.len(), false)
}

/// 扫描数字字面量（含小数与下划线分隔）。
fn scan_number(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let mut seen_dot = false;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_digit() || b == b'_' {
            i += 1;
        } else if b == b'.' && !seen_dot {
            seen_dot = true;
            i += 1;
        } else if b.is_ascii_alphabetic() {
            // 后缀：0u8、1.0f64
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// 一条诊断信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// 行号，从 1 开始。
    pub line: usize,
    /// 列号，从 1 开始（按字符计）。
    pub column: usize,
    pub message: String,
}

/// 检查括号与引号配对；返回按行号排序的诊断。
pub fn diagnose(text: &str, lang: Language) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let lines = highlight(text, lang);
    let mut stack: Vec<(char, usize, usize)> = Vec::new();

    for (idx, line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let toks = &lines[idx];
        let in_comment = |i: usize| {
            toks.iter()
                .any(|t| t.kind == TokenKind::Comment && i >= t.start && i < t.end)
        };
        // 该位置是否是一段字符串 token 的起点
        let str_at = |i: usize| {
            toks.iter()
                .find(|t| t.kind == TokenKind::Str && t.start == i)
        };

        for (byte_idx, ch) in line.char_indices() {
            if in_comment(byte_idx) {
                continue;
            }
            let column = line[..byte_idx].chars().count() + 1;

            // 字符串：整段跳过，同时检查引号是否闭合
            if let Some(tok) = str_at(byte_idx) {
                let closed = line[tok.start..tok.end].ends_with(ch)
                    && tok.end - tok.start > 1
                    && matches!(ch, '"' | '\'');
                if !closed && lang.uses_brackets() {
                    out.push(Diagnostic {
                        line: line_no,
                        column,
                        message: "引号未闭合".to_string(),
                    });
                }
                continue;
            }

            // 字符串内部（含未闭合的）都不参与括号检查
            if toks
                .iter()
                .any(|t| t.kind == TokenKind::Str && byte_idx > t.start && byte_idx < t.end)
            {
                continue;
            }

            if !lang.uses_brackets() {
                continue;
            }
            match ch {
                '(' | '[' | '{' => stack.push((ch, line_no, column)),
                ')' | ']' | '}' => {
                    let want = match ch {
                        ')' => '(',
                        ']' => '[',
                        _ => '{',
                    };
                    match stack.last().copied() {
                        // 正常配对
                        Some((open, _, _)) if open == want => {
                            stack.pop();
                        }
                        // 类型不匹配：报当前位置，开括号留在栈里，
                        // 这样文件末尾还能报出「未闭合」，信息更完整
                        Some(open_pair) => {
                            out.push(Diagnostic {
                                line: line_no,
                                column,
                                message: format!(
                                    "`{ch}` 与第 {} 行第 {} 列的 `{}` 不匹配",
                                    open_pair.1, open_pair.2, open_pair.0
                                ),
                            });
                        }
                        None => out.push(Diagnostic {
                            line: line_no,
                            column,
                            message: format!("多余的 `{ch}`，之前没有对应的开括号"),
                        }),
                    }
                }
                _ => {}
            }
        }
    }

    for (open, line, column) in stack {
        out.push(Diagnostic {
            line,
            column,
            message: format!("`{open}` 未闭合"),
        });
    }

    out.sort_by_key(|d| (d.line, d.column));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str, lang: Language) -> Vec<TokenKind> {
        let mut st = State::default();
        highlight_line(line, lang, &mut st)
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    fn text_of<'a>(line: &'a str, tok: &Token) -> &'a str {
        &line[tok.start..tok.end]
    }

    #[test]
    fn detects_language_by_extension() {
        assert_eq!(Language::from_path(Path::new("a.rs")), Language::Rust);
        assert_eq!(Language::from_path(Path::new("A.Java")), Language::Java);
        assert_eq!(Language::from_path(Path::new("x.md")), Language::Markdown);
        assert_eq!(Language::from_path(Path::new("a.xml")), Language::Xml);
        assert_eq!(Language::from_path(Path::new("noext")), Language::Plain);
        assert_eq!(
            Language::from_path(Path::new("a.unknownext")),
            Language::Plain
        );
    }

    #[test]
    fn rust_keywords_and_comments() {
        let line = "pub fn main() { // 入口";
        let mut st = State::default();
        let toks = highlight_line(line, Language::Rust, &mut st);
        assert_eq!(toks[0].kind, TokenKind::Keyword);
        assert_eq!(text_of(line, &toks[0]), "pub");
        assert_eq!(toks[1].kind, TokenKind::Keyword);
        assert_eq!(text_of(line, &toks[1]), "fn");
        let last = toks.last().expect("应有注释");
        assert_eq!(last.kind, TokenKind::Comment);
        assert!(text_of(line, last).contains("入口"));
    }

    #[test]
    fn string_with_braces_is_one_token() {
        let line = r#"let s = "a{b}c";"#;
        let mut st = State::default();
        let toks = highlight_line(line, Language::Rust, &mut st);
        let s = toks
            .iter()
            .find(|t| t.kind == TokenKind::Str)
            .expect("应有字符串");
        assert_eq!(text_of(line, s), r#""a{b}c""#);
    }

    #[test]
    fn escaped_quote_does_not_end_string() {
        let line = r#"x = "a\"b" + 1"#;
        let mut st = State::default();
        let toks = highlight_line(line, Language::Java, &mut st);
        let s = toks
            .iter()
            .find(|t| t.kind == TokenKind::Str)
            .expect("应有字符串");
        assert_eq!(text_of(line, s), r#""a\"b""#);
    }

    #[test]
    fn block_comment_spans_lines() {
        let mut st = State::default();
        let first = highlight_line("let a = 1; /* 开始", Language::Rust, &mut st);
        assert_eq!(first.last().expect("注释").kind, TokenKind::Comment);
        assert!(st.in_block_comment, "应进入块注释状态");

        let second = highlight_line("中间还有 /* 嵌套 */ 结束", Language::Rust, &mut st);
        assert_eq!(second[0].kind, TokenKind::Comment);
        assert!(!st.in_block_comment, "块注释应闭合");
    }

    #[test]
    fn java_annotation_is_recognized() {
        let line = "@Override public void run() {}";
        let mut st = State::default();
        let toks = highlight_line(line, Language::Java, &mut st);
        assert_eq!(toks[0].kind, TokenKind::Annotation);
        assert_eq!(text_of(line, &toks[0]), "@Override");
        assert_eq!(toks[1].kind, TokenKind::Keyword);
    }

    #[test]
    fn numbers_are_recognized() {
        let line = "let x = 42 + 3.14;";
        let mut st = State::default();
        let toks = highlight_line(line, Language::Rust, &mut st);
        let nums: Vec<&str> = toks
            .iter()
            .filter(|t| t.kind == TokenKind::Number)
            .map(|t| text_of(line, t))
            .collect();
        assert_eq!(nums, vec!["42", "3.14"]);
    }

    #[test]
    fn python_hash_comment() {
        assert_eq!(
            kinds("def f():  # 注释", Language::Python),
            vec![TokenKind::Keyword, TokenKind::Comment]
        );
    }

    #[test]
    fn markdown_heading_and_link() {
        let toks = highlight_markdown("## 标题");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::Heading);

        let toks = highlight_markdown("见 [文档](https://a.b) 说明");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::Link);
    }

    #[test]
    fn markdown_inline_code() {
        let toks = highlight_markdown("执行 `cargo test` 即可");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::InlineCode);
    }

    #[test]
    fn markup_tag_and_attribute() {
        let line = r#"<bean id="x" class="Y">"#;
        let toks = highlight_markup(line);
        let tags: Vec<&str> = toks
            .iter()
            .filter(|t| t.kind == TokenKind::Tag)
            .map(|t| text_of(line, t))
            .collect();
        assert_eq!(tags, vec!["bean"]);
        let attrs: Vec<&str> = toks
            .iter()
            .filter(|t| t.kind == TokenKind::Attribute)
            .map(|t| text_of(line, t))
            .collect();
        assert_eq!(attrs, vec!["id", "class"]);
    }

    #[test]
    fn tokens_are_sorted_and_non_overlapping() {
        let text = "pub fn f() { let s = \"x\"; }\n/* a\nb */\n@A class B {}";
        for line in highlight(text, Language::Rust) {
            let mut last_end = 0;
            for t in line {
                assert!(t.start >= last_end, "token 重叠或乱序");
                assert!(t.end > t.start, "空 token");
                last_end = t.end;
            }
        }
    }

    #[test]
    fn highlight_output_matches_line_count() {
        let text = "a\nb\nc\n";
        assert_eq!(highlight(text, Language::Plain).len(), 3);
        assert!(highlight("", Language::Rust).is_empty());
    }

    #[test]
    fn diagnose_finds_unclosed_brace() {
        let diags = diagnose("fn main() {\n    let a = 1;\n", Language::Rust);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].line, 1);
        assert!(diags[0].message.contains('{'));
    }

    #[test]
    fn diagnose_finds_extra_close() {
        let diags = diagnose("fn main() {}\n}\n", Language::Rust);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].line, 2);
        assert!(diags[0].message.contains("多余"));
    }

    #[test]
    fn diagnose_ignores_brackets_in_strings_and_comments() {
        let text = "let s = \"}\";\n// }\nfn ok() {}\n";
        assert!(
            diagnose(text, Language::Rust).is_empty(),
            "{:?}",
            diagnose(text, Language::Rust)
        );
    }

    #[test]
    fn diagnose_reports_mismatched_pair() {
        let diags = diagnose("f(a]\n", Language::Rust);
        // 同时报出「不匹配」与「`(` 未闭合」
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().any(|d| d.message.contains("不匹配")));
        assert!(diags.iter().any(|d| d.message.contains("未闭合")));
    }

    #[test]
    fn diagnose_reports_unclosed_quote() {
        let diags = diagnose("let s = \"abc;\n", Language::Rust);
        assert!(
            diags.iter().any(|d| d.message.contains("引号未闭合")),
            "{diags:?}"
        );
    }

    #[test]
    fn diagnose_clean_file_has_no_noise() {
        let text = "\
fn main() {
    let v = vec![1, 2, 3];
    println!(\"{:?}\", v);
}
";
        assert!(
            diagnose(text, Language::Rust).is_empty(),
            "{:?}",
            diagnose(text, Language::Rust)
        );
    }

    #[test]
    fn diagnose_skips_languages_without_brackets() {
        let text = "标题 ( 未闭合\n";
        assert!(diagnose(text, Language::Markdown).is_empty());
    }

    #[test]
    fn handles_non_ascii_safely() {
        let line = "let 名字 = \"中文\";";
        let mut st = State::default();
        let toks = highlight_line(line, Language::Rust, &mut st);
        for t in &toks {
            assert!(line.is_char_boundary(t.start));
            assert!(line.is_char_boundary(t.end));
        }
    }
}
