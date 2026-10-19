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

pub fn cyclomatic(node: tree_sitter::Node, src: &[u8], d: &Decisions) -> u16 {
    let mut count: i64 = 1;
    let mut cursor = node.walk();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        let kind = n.kind();
        let text = || n.utf8_text(src).unwrap_or("");
        if d.branches.contains(&kind) && !d.not_branch.iter().any(|(k, prefix)| *k == kind && text().trim_start().starts_with(prefix)) {
            count += 1;
        }
        if kind == "binary_expression" && n.child_by_field_name("operator").is_some_and(|o| d.operators.contains(&o.utf8_text(src).unwrap_or(""))) {
            count += 1;
        }
        if let Some((arm, parent)) = d.arm {
            if kind == arm {
                count += 1;
            } else if kind == parent {
                count -= 1;
            }
        }
        stack.extend(n.named_children(&mut cursor));
    }
    count.clamp(1, u16::MAX as i64) as u16
}
