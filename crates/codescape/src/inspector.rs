use codescape_format::{ArchivedGraph, Lang, NodeFlags, NodeId, NodeKind, SubKind, Visibility};
use codescape_layout::{LayoutOptions, display_children};
use std::collections::BTreeMap;

pub const OWN_TYPE: &str = "this type";

#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub kind: String,
    pub name: String,
    pub owners: Vec<(NodeId, String)>,
    pub location: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub label: String,
    pub items: Vec<(NodeId, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chip {
    pub label: String,
    pub hint: &'static str,
}

fn kind_word(g: &ArchivedGraph, n: NodeId) -> &'static str {
    match g.sub_kind(n) {
        SubKind::Class => "class",
        SubKind::Interface => "interface",
        SubKind::Enum => "enum",
        SubKind::Record => "record",
        SubKind::Annotation => "annotation",
        SubKind::Struct => "struct",
        SubKind::NamedType => "type",
        SubKind::Method => "method",
        SubKind::Function => "function",
        SubKind::Field => "field",
        SubKind::Constructor => "constructor",
        SubKind::None => match g.kind(n) {
            NodeKind::Project => "project",
            NodeKind::Module => "module",
            NodeKind::Package => "package",
            NodeKind::Type => "type",
            NodeKind::Member => "member",
            NodeKind::External => "external",
        },
    }
}

fn plural(word: &str, n: usize) -> String {
    match (n, word.ends_with('s')) {
        (1, _) => word.to_string(),
        (_, true) => format!("{word}es"),
        _ => format!("{word}s"),
    }
}

fn nearest(g: &ArchivedGraph, n: NodeId, kind: NodeKind) -> Option<NodeId> {
    g.path(n).into_iter().rev().find(|&p| g.kind(p) == kind)
}

pub fn header(g: &ArchivedGraph, n: NodeId) -> Header {
    let word = kind_word(g, n).to_uppercase();
    let mut kind = if g.lang(n) == Lang::None { word } else { format!("{word} · {:?}", g.lang(n)) };
    let vis = match g.visibility(n) {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::Package => "package",
        Visibility::Private => "private",
        Visibility::Unknown => "",
    };
    if !vis.is_empty() {
        kind.push_str(&format!(" · {vis}"));
    }
    let owners = g.path(n).into_iter().rev().skip(1).filter(|&p| p != g.root()).take(2).map(|p| (p, g.name(p).to_string())).collect();
    let (start, end) = g.span(n);
    let location = g.file(n).filter(|_| start >= 1 && end >= start).map(|f| {
        let base = f.rsplit('/').next().unwrap_or(f);
        let lines = end - start + 1;
        format!("{base}:{start}-{end} · {lines} line{}", if lines == 1 { "" } else { "s" })
    });
    let location = match g.complexity(n) {
        Some(c) => Some(format!("{} · complexity {c}", location.unwrap_or_default()).trim_start_matches(" · ").to_string()),
        None => location,
    };
    Header { kind, name: g.name(n).to_string(), owners, location }
}

pub fn visible_links(g: &ArchivedGraph, list: &[NodeId], opts: &LayoutOptions) -> (Vec<NodeId>, usize) {
    let mut tests = 0;
    let mut shown = Vec::with_capacity(list.len());
    for &n in list {
        if !opts.hides(g, n) {
            shown.push(n);
        } else if g.flags(n).intersects(NodeFlags::TEST | NodeFlags::MOCK) {
            tests += 1;
        }
    }
    (shown, tests)
}

pub fn group_links(g: &ArchivedGraph, selected: NodeId, nodes: &[NodeId]) -> Vec<Group> {
    let own = nearest(g, selected, NodeKind::Type);
    let mut groups: BTreeMap<(bool, String), Vec<(NodeId, String)>> = BTreeMap::new();
    for &n in nodes {
        let ty = nearest(g, n, NodeKind::Type);
        let label = match ty.filter(|&t| t != n) {
            Some(_) if ty == own => g.name(n).to_string(),
            Some(t) => format!("{}.{}", g.name(t), g.name(n)),
            None => g.name(n).to_string(),
        };
        let key = if own.is_some() && ty == own {
            (true, OWN_TYPE.to_string())
        } else {
            let home = nearest(g, n, NodeKind::Package).or_else(|| nearest(g, n, NodeKind::Module)).unwrap_or(n);
            (false, g.name(home).to_string())
        };
        groups.entry(key).or_default().push((n, label));
    }
    groups
        .into_iter()
        .map(|((_, label), mut items)| {
            items.sort_by(|a, b| a.1.cmp(&b.1));
            Group { label, items }
        })
        .collect()
}

pub fn inside(g: &ArchivedGraph, n: NodeId, hidden: &dyn Fn(NodeId) -> bool) -> (String, Vec<(NodeId, String)>) {
    let mut kids: Vec<(NodeId, String)> = display_children(g, n).into_iter().filter(|&c| !hidden(c)).map(|c| (c, g.name(c).to_string())).collect();
    kids.sort_by(|a, b| a.1.cmp(&b.1));
    if kids.is_empty() {
        return (String::new(), kids);
    }
    let mut by_word: BTreeMap<&str, usize> = BTreeMap::new();
    for &(c, _) in &kids {
        *by_word.entry(kind_word(g, c)).or_default() += 1;
    }
    let first = g.kind(kids[0].0);
    let unit = if kids.iter().all(|&(c, _)| g.kind(c) == first) {
        match first {
            NodeKind::Member => "member",
            NodeKind::Type => "type",
            NodeKind::Package => "package",
            NodeKind::Module => "module",
            _ => "item",
        }
    } else {
        "item"
    };
    let parts: Vec<String> = by_word.into_iter().map(|(w, c)| format!("{c} {}", plural(w, c))).collect();
    (format!("{} {}: {}", kids.len(), plural(unit, kids.len()), parts.join(", ")), kids)
}

pub fn chips(g: &ArchivedGraph, n: NodeId, entry_owner: bool) -> Vec<Chip> {
    let mut out: Vec<Chip> = g
        .tags(n)
        .map(|t| match t.split_once(':') {
            Some(("layer", role)) => Chip { label: role.to_string(), hint: "its role in the layering: handler, service, repository or model" },
            Some(("filter", name)) => Chip { label: name.to_string(), hint: "matches one of your filters" },
            Some((_, rest)) => Chip { label: rest.to_string(), hint: "set by a rule pack" },
            None => Chip { label: t.to_string(), hint: "set by a rule pack" },
        })
        .collect();
    let flags = g.flags(n);
    for (flag, label, hint) in [
        (NodeFlags::WIRING, "wiring", "dependency-injection code, hidden unless View > Wiring is on"),
        (NodeFlags::DETAIL, "detail", "a data type or helper, hidden unless View > Details is on"),
        (NodeFlags::NOISE, "error type", "an error or exception type, hidden unless View > Errors is on"),
        (NodeFlags::TEST, "test code", "test code, hidden unless View > Tests is on"),
        (NodeFlags::MOCK, "mock", "a generated or hand-written mock"),
        (NodeFlags::GENERATED, "generated", "generated code"),
    ] {
        if flags.contains(flag) {
            out.push(Chip { label: label.into(), hint });
        }
    }
    if entry_owner {
        out.push(Chip { label: "has entry point".into(), hint: "an application entry point lives inside" });
    }
    out
}
