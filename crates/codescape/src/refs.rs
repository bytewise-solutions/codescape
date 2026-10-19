use crate::source::TAB;
use codescape_format::{ArchivedGraph, NodeId};
use codescape_layout::LayoutOptions;
use std::collections::HashMap;

pub fn display_span(raw: &str, byte_col: usize, byte_len: usize) -> (usize, usize) {
    let width = |s: &str| s.chars().map(|c| if c == '\t' { TAB.len() } else { 1 }).sum::<usize>();
    let at = |b: usize| (0..=b.min(raw.len())).rev().find(|&i| raw.is_char_boundary(i)).unwrap_or(0);
    let (start, end) = (at(byte_col), at(byte_col.saturating_add(byte_len)));
    (width(&raw[..start]), width(&raw[start..end]))
}

pub fn followable(g: &ArchivedGraph, opts: &LayoutOptions, target: NodeId) -> bool {
    !g.path(target).into_iter().any(|n| opts.hides(g, n))
}

struct Link {
    col: usize,
    len: usize,
    target: Option<NodeId>,
}

pub struct FileRefs {
    lines: HashMap<usize, Vec<Link>>,
}

impl FileRefs {
    pub fn new(g: &ArchivedGraph, owner: NodeId, raw: &[String]) -> FileRefs {
        let mut lines: HashMap<usize, Vec<Link>> = HashMap::new();
        let Some(path) = g.file(owner) else { return FileRefs { lines } };
        let mut stack: Vec<NodeId> = match g.parent(owner) {
            Some(p) => g.children(p).filter(|&c| g.file(c) == Some(path)).collect(),
            None => vec![owner],
        };
        while let Some(n) = stack.pop() {
            stack.extend(g.children(n));
            if g.file(n) != Some(path) {
                continue;
            }
            for r in g.refs_from(n) {
                let (line, to) = (r.line.to_native() as usize, r.to.to_native());
                let Some(text) = line.checked_sub(1).and_then(|i| raw.get(i)) else { continue };
                if g.file(to).is_none() {
                    continue;
                }
                let (col, len) = display_span(text, r.col.to_native() as usize, r.len.to_native() as usize);
                if len == 0 {
                    continue;
                }
                let links = lines.entry(line).or_default();
                match links.iter_mut().find(|l| l.col == col && l.len == len) {
                    Some(l) if l.target != Some(to) => l.target = None,
                    Some(_) => {}
                    None => links.push(Link { col, len, target: Some(to) }),
                }
            }
        }
        FileRefs { lines }
    }

    pub fn at(&self, line: usize, col: usize) -> Option<(usize, usize, NodeId)> {
        let covering: Vec<&Link> = self.lines.get(&line)?.iter().filter(|l| (l.col..l.col + l.len).contains(&col)).collect();
        let len = covering.iter().map(|l| l.len).min()?;
        match covering.iter().filter(|l| l.len == len).collect::<Vec<_>>()[..] {
            [l] => l.target.map(|t| (l.col, l.len, t)),
            _ => None,
        }
    }
}
