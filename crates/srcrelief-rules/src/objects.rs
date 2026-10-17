use std::collections::{HashMap, HashSet};
use tree_sitter::{Language, Node, Parser};

pub struct RouteShape<'r> {
    pub var_types: &'r [String],
    pub calls: &'r [String],
    pub path_key: &'r str,
    pub target_keys: &'r [String],
    pub children_key: &'r str,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RouteTarget {
    Local(String),
    Import { spec: String, member: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct FoundRoute {
    pub line: u32,
    pub path: String,
    pub target: Option<RouteTarget>,
}

fn language(rel_path: &str) -> Option<Language> {
    let ext = rel_path.rsplit('.').next()?;
    match ext {
        "ts" | "mts" | "cts" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "js" | "jsx" | "mjs" | "cjs" => Some(tree_sitter_javascript::LANGUAGE.into()),
        _ => None,
    }
}

fn kids(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.named_children(&mut c).collect()
}

fn descendants(n: Node) -> Vec<Node> {
    let mut out = Vec::new();
    let mut stack = vec![n];
    while let Some(x) = stack.pop() {
        out.push(x);
        stack.extend(kids(x).into_iter().rev());
    }
    out
}

pub fn join_route(prefix: &str, seg: &str) -> String {
    let parts: Vec<&str> = prefix.split('/').chain(seg.split('/')).filter(|s| !s.is_empty()).collect();
    format!("/{}", parts.join("/"))
}

struct Walk<'s, 'r> {
    src: &'s [u8],
    shape: &'r RouteShape<'r>,
    imports: HashMap<String, (String, String)>,
    decls: HashMap<String, Node<'s>>,
    out: Vec<FoundRoute>,
}

impl<'s, 'r> Walk<'s, 'r> {
    fn text(&self, n: Node) -> &'s str {
        n.utf8_text(self.src).unwrap_or("")
    }

    fn string(&self, n: Node) -> Option<String> {
        matches!(n.kind(), "string" | "template_string").then(|| self.text(n).trim_matches(['"', '\'', '`']).to_string())
    }

    fn key(&self, pair: Node) -> String {
        pair.child_by_field_name("key").map(|k| self.text(k).trim_matches(['"', '\'']).to_string()).unwrap_or_default()
    }

    fn routes(&mut self, arr: Node<'s>, prefix: &str) {
        for obj in kids(arr).into_iter().filter(|o| o.kind() == "object") {
            let pairs: Vec<(String, Node<'s>)> = kids(obj).into_iter().filter(|p| p.kind() == "pair").filter_map(|p| Some((self.key(p), p.child_by_field_name("value")?))).collect();
            let get = |k: &str| pairs.iter().find(|(key, _)| key == k).map(|(_, v)| *v);
            let seg = get(self.shape.path_key).and_then(|v| self.string(v)).unwrap_or_default();
            let full = join_route(prefix, &seg);
            let targets: Vec<Node<'s>> = self.shape.target_keys.iter().filter_map(|k| get(k)).collect();
            if let Some(&first) = targets.first() {
                let target = self.target(first, 0);
                self.out.push(FoundRoute { line: obj.start_position().row as u32 + 1, path: full.clone(), target });
            }
            if let Some(children) = get(self.shape.children_key).filter(|c| c.kind() == "array") {
                self.routes(children, &full);
            }
        }
    }

    fn target(&self, v: Node<'s>, depth: u32) -> Option<RouteTarget> {
        match v.kind() {
            "identifier" => {
                let name = self.text(v);
                if let Some((spec, imported)) = self.imports.get(name) {
                    return Some(RouteTarget::Import { spec: spec.clone(), member: Some(imported.clone()) });
                }
                match self.decls.get(name) {
                    Some(&value) if depth < 3 => self.target(value, depth + 1),
                    _ => Some(RouteTarget::Local(name.to_string())),
                }
            }
            _ => {
                let all = descendants(v);
                let import = all.iter().find(|n| n.kind() == "call_expression" && n.child_by_field_name("function").is_some_and(|f| f.kind() == "import"))?;
                let spec = import.child_by_field_name("arguments").and_then(|a| kids(a).into_iter().next()).and_then(|a| self.string(a))?;
                let member = all
                    .iter()
                    .filter(|n| n.kind() == "call_expression")
                    .filter_map(|n| n.child_by_field_name("function"))
                    .filter(|f| f.kind() == "member_expression" && f.child_by_field_name("property").is_some_and(|p| self.text(p) == "then"))
                    .filter_map(|f| f.parent()?.child_by_field_name("arguments")?.named_child(0))
                    .filter_map(|cb| cb.child_by_field_name("body"))
                    .find(|body| body.kind() == "member_expression")
                    .and_then(|body| body.child_by_field_name("property"))
                    .map(|p| self.text(p).to_string());
                Some(RouteTarget::Import { spec, member })
            }
        }
    }
}

pub fn object_routes(rel_path: &str, src: &[u8], shape: &RouteShape) -> Vec<FoundRoute> {
    let Some(lang) = language(rel_path) else { return Vec::new() };
    let mut parser = Parser::new();
    if parser.set_language(&lang).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(src, None) else { return Vec::new() };
    let mut w = Walk { src, shape, imports: HashMap::new(), decls: HashMap::new(), out: Vec::new() };
    let nodes = descendants(tree.root_node());
    for n in &nodes {
        match n.kind() {
            "import_specifier" => {
                let Some(name) = n.child_by_field_name("name") else { continue };
                let local = n.child_by_field_name("alias").unwrap_or(name);
                let Some(spec) = n.parent().and_then(|p| p.parent()).and_then(|c| c.parent()).and_then(|s| s.child_by_field_name("source")).and_then(|s| w.string(s)) else { continue };
                w.imports.insert(w.text(local).to_string(), (spec, w.text(name).to_string()));
            }
            "variable_declarator" => {
                if let (Some(name), Some(value)) = (n.child_by_field_name("name"), n.child_by_field_name("value")) {
                    w.decls.insert(w.text(name).to_string(), value);
                }
            }
            _ => {}
        }
    }
    let mut seen: HashSet<usize> = HashSet::new();
    for n in &nodes {
        let arr = match n.kind() {
            "variable_declarator" => {
                let typed = n.child_by_field_name("type").map(|t| w.text(t).trim_start_matches(':').split_whitespace().collect::<String>());
                if !typed.is_some_and(|t| shape.var_types.contains(&t)) {
                    continue;
                }
                n.child_by_field_name("value")
            }
            "call_expression" => {
                let callee = n.child_by_field_name("function").map(|f| w.text(f)).unwrap_or("");
                if !shape.calls.iter().any(|c| callee.rsplit('.').next() == Some(c.as_str())) {
                    continue;
                }
                n.child_by_field_name("arguments").and_then(|a| a.named_child(0))
            }
            _ => continue,
        };
        if let Some(arr) = arr.filter(|a| a.kind() == "array") {
            if seen.insert(arr.id()) {
                w.routes(arr, "");
            }
        }
    }
    w.out
}
