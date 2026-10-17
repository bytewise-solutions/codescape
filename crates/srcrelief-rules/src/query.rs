use anyhow::{Result, anyhow};
use srcrelief_format::Lang;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

fn language(lang: Lang) -> Option<Language> {
    match lang {
        Lang::Java => Some(tree_sitter_java::LANGUAGE.into()),
        Lang::Go => Some(tree_sitter_go::LANGUAGE.into()),
        Lang::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        Lang::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
        Lang::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
        Lang::None => None,
    }
}

pub fn validate(lang: Lang, query: &str) -> Result<()> {
    let l = language(lang).ok_or_else(|| anyhow!("no grammar"))?;
    Query::new(&l, query).map(|_| ()).map_err(|e| anyhow!("{e}"))
}

pub fn captures(lang: Lang, query: &str, capture: &str, src: &[u8]) -> Vec<(u32, String)> {
    let Some(l) = language(lang) else { return Vec::new() };
    let Ok(q) = Query::new(&l, query) else { return Vec::new() };
    let Some(index) = q.capture_index_for_name(capture) else { return Vec::new() };
    let mut parser = Parser::new();
    if parser.set_language(&l).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(src, None) else { return Vec::new() };
    let mut cursor = QueryCursor::new();
    let mut out = Vec::new();
    let mut matches = cursor.matches(&q, tree.root_node(), src);
    while let Some(m) = matches.next() {
        for c in m.captures().iter().filter(|c| c.index == index) {
            let text = c.node.utf8_text(src).unwrap_or("").to_string();
            out.push((c.node.start_position().row as u32 + 1, text));
        }
    }
    out
}
