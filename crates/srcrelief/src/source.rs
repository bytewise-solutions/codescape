use srcrelief_format::Lang;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MIN_CODE_PT: f32 = 5.0;
const MAX_CODE_PT: f32 = 13.0;
pub const LINE_SPACING: f32 = 1.25;
const TAB: &str = "    ";

const GO_WORDS: &[&str] = &[
    "break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "for", "func", "go", "goto", "if", "import",
    "interface", "map", "package", "range", "return", "select", "struct", "switch", "type", "var", "nil", "true", "false", "string", "int",
    "int64", "int32", "uint", "byte", "rune", "bool", "error", "float64", "any",
];

const JAVA_WORDS: &[&str] = &[
    "abstract", "boolean", "break", "byte", "case", "catch", "char", "class", "continue", "default", "do", "double", "else", "enum", "extends",
    "final", "finally", "float", "for", "if", "implements", "import", "instanceof", "int", "interface", "long", "new", "package", "private",
    "protected", "public", "return", "short", "static", "super", "switch", "synchronized", "this", "throw", "throws", "try", "var", "void",
    "volatile", "while", "null", "true", "false", "record", "yield",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Code {
    pub first_line: u32,
    pub lines: Vec<String>,
}

pub struct SourceCache {
    root: PathBuf,
    files: HashMap<String, Option<Arc<Vec<String>>>>,
    spans: HashMap<(String, u32, u32), Option<Arc<Code>>>,
}

impl SourceCache {
    pub fn new(root: &Path) -> SourceCache {
        SourceCache { root: root.to_path_buf(), files: HashMap::new(), spans: HashMap::new() }
    }

    pub fn span(&mut self, file: &str, start: u32, end: u32) -> Option<Arc<Code>> {
        let key = (file.to_string(), start, end);
        if let Some(c) = self.spans.get(&key) {
            return c.clone();
        }
        let code = self.read(file).filter(|_| start >= 1 && end >= start).map(|all| {
            let end = (end as usize).min(all.len());
            Arc::new(Code { first_line: start, lines: dedent(&all[(start as usize - 1).min(end)..end]) })
        });
        self.spans.insert(key, code.clone());
        code
    }

    fn read(&mut self, file: &str) -> Option<Arc<Vec<String>>> {
        let root = &self.root;
        self.files
            .entry(file.to_string())
            .or_insert_with(|| std::fs::read_to_string(root.join(file)).ok().map(|s| Arc::new(s.lines().map(|l| l.replace('\t', TAB)).collect())))
            .clone()
    }
}

fn dedent(lines: &[String]) -> Vec<String> {
    let indent = lines.iter().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
    lines.iter().map(|l| l.get(indent..).unwrap_or("").trim_end().to_string()).collect()
}

pub fn code_font(height: f32, lines: usize) -> Option<f32> {
    let font = height / (lines.max(1) as f32 * LINE_SPACING);
    (font >= MIN_CODE_PT).then_some(font.min(MAX_CODE_PT))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Text,
    Keyword,
    String,
    Comment,
}

pub fn highlight(lines: &[&str], lang: Lang) -> Vec<Vec<(String, Token)>> {
    let words = if lang == Lang::Java { JAVA_WORDS } else { GO_WORDS };
    let mut in_block = false;
    lines.iter().map(|line| highlight_line(line, words, &mut in_block)).collect()
}

fn highlight_line(line: &str, words: &[&str], in_block: &mut bool) -> Vec<(String, Token)> {
    let mut out: Vec<(String, Token)> = Vec::new();
    let mut push = |text: &str, kind: Token| match out.last_mut() {
        Some((t, k)) if *k == kind => t.push_str(text),
        _ if !text.is_empty() => out.push((text.to_string(), kind)),
        _ => {}
    };
    let mut rest = line;
    if *in_block {
        match rest.find("*/") {
            Some(i) => {
                push(&rest[..i + 2], Token::Comment);
                rest = &rest[i + 2..];
                *in_block = false;
            }
            None => {
                push(rest, Token::Comment);
                return out;
            }
        }
    }
    while let Some(c) = rest.chars().next() {
        if rest.starts_with("//") {
            push(rest, Token::Comment);
            break;
        }
        if rest.starts_with("/*") {
            match rest[2..].find("*/") {
                Some(i) => {
                    push(&rest[..i + 4], Token::Comment);
                    rest = &rest[i + 4..];
                }
                None => {
                    push(rest, Token::Comment);
                    *in_block = true;
                    break;
                }
            }
            continue;
        }
        if matches!(c, '"' | '\'' | '`') {
            let mut end = rest.len();
            let mut escaped = false;
            for (i, ch) in rest.char_indices().skip(1) {
                if escaped {
                    escaped = false;
                } else if ch == '\\' && c != '`' {
                    escaped = true;
                } else if ch == c {
                    end = i + 1;
                    break;
                }
            }
            push(&rest[..end], Token::String);
            rest = &rest[end..];
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let end = rest.find(|ch: char| !(ch.is_alphanumeric() || ch == '_')).unwrap_or(rest.len());
            let word = &rest[..end];
            push(word, if words.contains(&word) { Token::Keyword } else { Token::Text });
            rest = &rest[end..];
            continue;
        }
        push(&rest[..c.len_utf8()], Token::Text);
        rest = &rest[c.len_utf8()..];
    }
    out
}
