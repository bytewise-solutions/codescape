use codescape_format::{Lang, NodeId};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Import {
    pub path: String,
    pub alias: Option<String>,
    pub wildcard: bool,
    pub is_static: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Annotation {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeFacts {
    pub node: NodeId,
    pub fqn: String,
    pub supertypes: Vec<String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FnFacts {
    pub node: NodeId,
    pub owner: Option<NodeId>,
    pub name: String,
    pub params: Vec<(String, String)>,
    pub annotations: Vec<Annotation>,
    pub is_main: bool,
    pub overrides_external: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallFacts {
    pub caller: NodeId,
    pub method: String,
    pub receiver_var: Option<String>,
    pub receiver_type: Option<String>,
    pub string_args: Vec<Option<String>>,
    pub ident_args: Vec<Option<String>>,
    pub arg_targets: Vec<Option<NodeId>>,
    pub assigned_to: Option<String>,
    pub target: Option<NodeId>,
    pub instantiates: Option<String>,
    pub line: u32,
    pub col: u32,
    pub len: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TypeUse {
    pub owner: NodeId,
    pub name: String,
    pub line: u32,
    pub col: u32,
    pub len: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileFacts {
    pub rel_path: String,
    pub module: NodeId,
    pub package: Option<NodeId>,
    pub import_targets: Vec<NodeId>,
    pub lang: Lang,
    pub is_test: bool,
    pub is_dev: bool,
    pub imports: Vec<Import>,
    pub types: Vec<TypeFacts>,
    pub functions: Vec<FnFacts>,
    pub calls: Vec<CallFacts>,
    pub type_uses: Vec<TypeUse>,
}

#[derive(Clone, Debug, Default)]
pub struct ProjectFacts {
    pub files: Vec<FileFacts>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExtractStats {
    pub files: u32,
    pub lines: u64,
    pub bytes: u64,
    pub resolved: u64,
    pub external: u64,
    pub guessed: u64,
    pub unresolved: u64,
}

impl ExtractStats {
    pub fn add(&mut self, o: ExtractStats) {
        self.files += o.files;
        self.lines += o.lines;
        self.bytes += o.bytes;
        self.resolved += o.resolved;
        self.external += o.external;
        self.guessed += o.guessed;
        self.unresolved += o.unresolved;
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldDecl {
    pub name: String,
    pub ty: String,
    pub visibility: codescape_format::Visibility,
    pub line: u32,
}

pub struct Decisions {
    pub branches: &'static [&'static str],
    pub operators: &'static [&'static str],
    pub not_branch: &'static [(&'static str, &'static str)],
    pub arm: Option<(&'static str, &'static str)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComplexityDetail {
    pub total: u16,
    pub decisions: Vec<(&'static str, u16)>,
    pub nesting: u16,
}

impl Default for ComplexityDetail {
    fn default() -> Self {
        ComplexityDetail { total: 1, decisions: Vec::new(), nesting: 0 }
    }
}

fn label(kind: &'static str) -> &'static str {
    match kind {
        "if_statement" | "if_expression" => "if",
        "for_statement" | "enhanced_for_statement" | "for_in_statement" | "for_expression" => "for",
        "while_statement" | "while_expression" => "while",
        "do_statement" => "do",
        "catch_clause" => "catch",
        "ternary_expression" => "?:",
        "switch_label" | "switch_case" | "expression_case" | "type_case" | "communication_case" => "case",
        "match_arm" => "match arm",
        other => other,
    }
}

fn tally(counts: &mut Vec<(&'static str, i64)>, name: &'static str, by: i64) {
    match counts.iter_mut().find(|(n, _)| *n == name) {
        Some((_, c)) => *c += by,
        None => counts.push((name, by)),
    }
}

pub fn complexity_detail(node: tree_sitter::Node, src: &[u8], d: &Decisions) -> ComplexityDetail {
    let mut counts: Vec<(&'static str, i64)> = Vec::new();
    let mut nesting = 0u16;
    let mut cursor = node.walk();
    let mut stack = vec![(node, 0u16)];
    while let Some((n, depth)) = stack.pop() {
        let kind = n.kind();
        let mut depth = depth;
        if let Some(&k) = d.branches.iter().find(|&&k| k == kind)
            && !d.not_branch.iter().any(|(nk, prefix)| *nk == kind && n.utf8_text(src).unwrap_or("").trim_start().starts_with(prefix))
        {
            let name = label(k);
            tally(&mut counts, name, 1);
            if name != "case" {
                depth += 1;
                nesting = nesting.max(depth);
            }
        }
        if kind == "binary_expression"
            && let Some(op) = n.child_by_field_name("operator").and_then(|o| o.utf8_text(src).ok())
            && let Some(&known) = d.operators.iter().find(|&&k| k == op)
        {
            tally(&mut counts, known, 1);
        }
        if let Some((arm, parent)) = d.arm {
            if kind == arm {
                tally(&mut counts, label(arm), 1);
            } else if kind == parent {
                tally(&mut counts, label(arm), -1);
            }
        }
        let children: Vec<_> = n.named_children(&mut cursor).collect();
        stack.extend(children.into_iter().rev().map(|c| (c, depth)));
    }
    let sum: i64 = counts.iter().map(|(_, c)| *c).sum();
    ComplexityDetail {
        total: (1 + sum).clamp(1, u16::MAX as i64) as u16,
        decisions: counts.into_iter().filter(|(_, c)| *c > 0).map(|(n, c)| (n, c.min(u16::MAX as i64) as u16)).collect(),
        nesting,
    }
}

pub fn cyclomatic(node: tree_sitter::Node, src: &[u8], d: &Decisions) -> u16 {
    complexity_detail(node, src, d).total
}
