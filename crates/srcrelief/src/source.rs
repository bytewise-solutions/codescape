use srcrelief_format::Lang;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const FILE_MIN_PT: f32 = 7.0;
const FILE_COLUMNS: f32 = 100.0;
const MONO_ADVANCE: f32 = 0.62;
pub const LINE_SPACING: f32 = 1.25;
const TAB: &str = "    ";

const GO_WORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
    "nil",
    "true",
    "false",
    "string",
    "int",
    "int64",
    "int32",
    "uint",
    "byte",
    "rune",
    "bool",
    "error",
    "float64",
    "any",
];

const JAVA_WORDS: &[&str] = &[
    "abstract",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
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
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "try",
    "var",
    "void",
    "volatile",
    "while",
    "null",
    "true",
    "false",
    "record",
    "yield",
];

const TS_WORDS: &[&str] = &[
    "abstract",
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "constructor",
    "continue",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "namespace",
    "new",
    "null",
    "of",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "set",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "yield",
    "string",
    "number",
    "boolean",
    "any",
    "unknown",
    "never",
    "object",
];

const RUST_WORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "bool", "char", "str", "u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "i64", "isize", "f32", "f64", "Option", "Result", "Some", "None",
    "Ok", "Err", "Vec", "String", "Box",
];

const CHAR_LITERAL_MAX: usize = 4;

pub struct SourceCache {
    root: PathBuf,
    files: HashMap<String, Option<Arc<Vec<String>>>>,
}

impl SourceCache {
    pub fn new(root: &Path) -> SourceCache {
        SourceCache { root: root.to_path_buf(), files: HashMap::new() }
    }

    pub fn file(&mut self, file: &str) -> Option<Arc<Vec<String>>> {
        let root = &self.root;
        self.files
            .entry(file.to_string())
            .or_insert_with(|| {
                std::fs::read_to_string(root.join(file)).ok().map(|s| Arc::new(s.lines().map(|l| l.replace('\t', TAB).trim_end().to_string()).collect()))
            })
            .clone()
    }
}

pub fn file_font(box_width: f32) -> Option<f32> {
    let font = box_width / (FILE_COLUMNS * MONO_ADVANCE);
    (font >= FILE_MIN_PT).then_some(font)
}

pub fn advance(font: f32) -> f32 {
    font * MONO_ADVANCE
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Text,
    Keyword,
    String,
    Comment,
}

pub fn highlight(lines: &[&str], lang: Lang) -> Vec<Vec<(String, Token)>> {
    let words = match lang {
        Lang::Java => JAVA_WORDS,
        Lang::TypeScript | Lang::JavaScript => TS_WORDS,
        Lang::Rust => RUST_WORDS,
        Lang::Go | Lang::None => GO_WORDS,
    };
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
        if c == '\'' && !rest.char_indices().skip(1).take(CHAR_LITERAL_MAX).any(|(i, ch)| ch == '\'' && !rest[..i].ends_with('\\')) {
            push("'", Token::Text);
            rest = &rest[1..];
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

pub fn merge_spans(mut spans: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    spans.retain(|(s, e)| *s >= 1 && e >= s);
    spans.sort();
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (s, e) in spans {
        match out.last_mut() {
            Some(last) if s <= last.1 + 1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

const END_MARGIN_LINES: f32 = 2.0;

pub fn initial_scroll(start_line: u32, box_y: f32, view_y: f32, line_h: f32) -> f32 {
    (start_line.saturating_sub(1) as f32 - (box_y - view_y) / line_h).max(0.0)
}

pub fn max_scroll(total: usize, visible: usize) -> f32 {
    (total as f32 - visible as f32 + END_MARGIN_LINES).max(0.0)
}

pub fn scroll_by(current: f32, wheel_px: f32, line_h: f32, total: usize, visible: usize) -> f32 {
    (current - wheel_px / line_h).clamp(0.0, max_scroll(total, visible))
}

const MIN_THUMB_PX: f32 = 24.0;

fn thumb_len(total: usize, visible: usize, track: f32) -> f32 {
    (track * visible as f32 / total.max(1) as f32).clamp(MIN_THUMB_PX.min(track), track)
}

pub fn thumb(scroll: f32, total: usize, visible: usize, track: f32) -> (f32, f32) {
    let len = thumb_len(total, visible, track);
    let max = max_scroll(total, visible);
    let top = if max > 0.0 { (track - len) * (scroll / max).clamp(0.0, 1.0) } else { 0.0 };
    (top, len)
}

pub fn scroll_at(y: f32, total: usize, visible: usize, track: f32) -> f32 {
    let len = thumb_len(total, visible, track);
    let room = track - len;
    if room <= 0.0 {
        return 0.0;
    }
    ((y - len / 2.0).clamp(0.0, room) / room) * max_scroll(total, visible)
}

const END_MARGIN_COLUMNS: f32 = 4.0;

pub fn hscroll_by(current: f32, wheel_px: f32, longest_line: usize, advance: f32, text_width: f32) -> f32 {
    let max = ((longest_line as f32 + END_MARGIN_COLUMNS) * advance - text_width).max(0.0);
    (current - wheel_px).clamp(0.0, max)
}
