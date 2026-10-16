use srcrelief_format::{Lang, NodeId};

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
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileFacts {
    pub rel_path: String,
    pub module: NodeId,
    pub lang: Lang,
    pub is_test: bool,
    pub is_dev: bool,
    pub imports: Vec<Import>,
    pub types: Vec<TypeFacts>,
    pub functions: Vec<FnFacts>,
    pub calls: Vec<CallFacts>,
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
    pub guessed: u64,
    pub unresolved: u64,
}

impl ExtractStats {
    pub fn add(&mut self, o: ExtractStats) {
        self.files += o.files;
        self.lines += o.lines;
        self.bytes += o.bytes;
        self.resolved += o.resolved;
        self.guessed += o.guessed;
        self.unresolved += o.unresolved;
    }
}
