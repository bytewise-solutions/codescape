use codescape_facts::{Annotation, Decisions, FieldDecl, cyclomatic};

const DECISIONS: Decisions = Decisions {
    branches: &["if_statement", "for_statement", "for_in_statement", "while_statement", "do_statement", "catch_clause", "ternary_expression", "switch_case"],
    operators: &["&&", "||", "??"],
    not_branch: &[],
    arm: None,
};
use codescape_format::{SubKind, Visibility};
use tree_sitter::{Node, Parser};

pub const CALL_SIGNATURE: &str = "()";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    TypeScript,
    Tsx,
    JavaScript,
}

impl Dialect {
    pub fn for_extension(ext: &str) -> Option<Dialect> {
        match ext {
            "ts" | "mts" | "cts" => Some(Dialect::TypeScript),
            "tsx" => Some(Dialect::Tsx),
            "js" | "jsx" | "mjs" | "cjs" => Some(Dialect::JavaScript),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeExpr {
    Named(String),
    Path(String),
    CallOf(String),
    Await(Box<TypeExpr>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TsFile {
    pub imports: Vec<TsImport>,
    pub reexports: Vec<ReExport>,
    pub exports: Vec<(String, String)>,
    pub default_export: Option<String>,
    pub types: Vec<TsType>,
    pub funcs: Vec<TsFunc>,
    pub top: TsFunc,
    pub vars: Vec<(String, TypeExpr)>,
    pub lines: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TsImport {
    pub spec: String,
    pub default: Option<String>,
    pub namespace: Option<String>,
    pub named: Vec<(String, String)>,
    pub dynamic: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReExport {
    pub spec: String,
    pub names: Option<Vec<(String, String)>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TsUse {
    pub name: String,
    pub line: u32,
    pub col: u32,
    pub len: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TsType {
    pub name: String,
    pub kind: SubKind,
    pub extends: Vec<String>,
    pub implements: Vec<String>,
    pub decorators: Vec<Annotation>,
    pub decorator_refs: Vec<String>,
    pub fields: Vec<(String, TypeExpr)>,
    pub field_decls: Vec<FieldDecl>,
    pub methods: Vec<TsFunc>,
    pub type_uses: Vec<TsUse>,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TsFunc {
    pub name: String,
    pub sub: SubKind,
    pub visibility: Visibility,
    pub complexity: u16,
    pub param_fields: Vec<FieldDecl>,
    pub params: Vec<(String, String)>,
    pub returns: Option<String>,
    pub decorators: Vec<Annotation>,
    pub locals: Vec<(String, TypeExpr)>,
    pub calls: Vec<TsCall>,
    pub type_uses: Vec<TsUse>,
    pub declared: Vec<String>,
    pub start_line: u32,
    pub end_line: u32,
}

impl Default for TsFunc {
    fn default() -> Self {
        TsFunc {
            name: String::new(),
            sub: SubKind::Function,
            visibility: Visibility::Unknown,
            complexity: 1,
            param_fields: Vec::new(),
            params: Vec::new(),
            returns: None,
            decorators: Vec::new(),
            locals: Vec::new(),
            calls: Vec::new(),
            type_uses: Vec::new(),
            declared: Vec::new(),
            start_line: 0,
            end_line: 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TsCall {
    pub receiver: Option<String>,
    pub method: String,
    pub string_args: Vec<Option<String>>,
    pub ident_args: Vec<Option<String>>,
    pub is_new: bool,
    pub assigned_to: Option<String>,
    pub line: u32,
    pub col: u32,
    pub len: u32,
    pub in_field: bool,
}

pub const CHAIN: &str = "<chain>";

pub fn parse_ts(src: &[u8], dialect: Dialect) -> TsFile {
    let mut parser = Parser::new();
    let lang: tree_sitter::Language = match dialect {
        Dialect::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Dialect::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Dialect::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
    };
    parser.set_language(&lang).expect("typescript grammar");
    let lines = src.iter().filter(|&&b| b == b'\n').count() as u32 + 1;
    let Some(tree) = parser.parse(src, None) else { return TsFile { lines, ..Default::default() } };
    let mut out = TsFile { lines, ..Default::default() };
    out.top.start_line = 1;
    out.top.end_line = lines;
    let c = Ctx { src };
    let root = tree.root_node();
    let mut cur = root.walk();
    for n in root.named_children(&mut cur) {
        c.statement(n, &mut out, &[]);
    }
    out.top.calls.sort_by_key(|c| c.line);
    out
}

fn line(n: Node) -> u32 {
    n.start_position().row as u32 + 1
}

fn col(n: Node) -> u32 {
    n.start_position().column as u32
}

fn len(n: Node) -> u32 {
    (n.end_byte() - n.start_byte()) as u32
}

fn end_line(n: Node) -> u32 {
    n.end_position().row as u32 + 1
}

fn annotation_text(p: &Ctx, n: Node) -> String {
    p.text(n).trim_start_matches(['?', ':']).trim().to_string()
}

fn member_visibility(p: &Ctx, m: Node, name: &str) -> Visibility {
    if name.starts_with('#') {
        return Visibility::Private;
    }
    match named_kids(m).into_iter().find(|k| k.kind() == "accessibility_modifier").map(|k| p.text(k)) {
        Some("private") => Visibility::Private,
        Some("protected") => Visibility::Protected,
        _ => Visibility::Public,
    }
}

fn named_kids(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.named_children(&mut c).collect()
}

fn has_token(n: Node, word: &str) -> bool {
    let mut c = n.walk();
    n.children(&mut c).any(|x| !x.is_named() && x.kind() == word)
}

fn is_function(n: Node) -> bool {
    matches!(n.kind(), "arrow_function" | "function_expression" | "function" | "generator_function")
}

struct Ctx<'s> {
    src: &'s [u8],
}

impl<'s> Ctx<'s> {
    fn text(&self, n: Node) -> &'s str {
        n.utf8_text(self.src).unwrap_or("")
    }

    fn string(&self, n: Node) -> Option<String> {
        match n.kind() {
            "string" => Some(self.text(n).trim_matches(['"', '\'']).to_string()),
            "template_string" if !named_kids(n).iter().any(|k| k.kind() == "template_substitution") => Some(self.text(n).trim_matches('`').to_string()),
            _ => None,
        }
    }

    fn path(&self, n: Node) -> Option<String> {
        match n.kind() {
            "identifier" | "this" | "super" => Some(self.text(n).to_string()),
            "member_expression" => {
                let obj = self.path(n.child_by_field_name("object")?)?;
                let prop = n.child_by_field_name("property")?;
                (prop.kind() == "property_identifier").then(|| format!("{obj}.{}", self.text(prop)))
            }
            "non_null_expression" | "parenthesized_expression" => named_kids(n).first().and_then(|k| self.path(*k)),
            _ => None,
        }
    }

    fn chain_path(&self, n: Node) -> Option<String> {
        match n.kind() {
            "call_expression" => self.chain_path(n.child_by_field_name("function")?).map(|f| format!("{f}()")),
            "member_expression" => {
                let obj = self.chain_path(n.child_by_field_name("object")?)?;
                let prop = n.child_by_field_name("property")?;
                (prop.kind() == "property_identifier").then(|| format!("{obj}.{}", self.text(prop)))
            }
            "non_null_expression" | "parenthesized_expression" => named_kids(n).first().and_then(|k| self.chain_path(*k)),
            _ => self.path(n),
        }
    }

    fn type_name(&self, n: Node) -> String {
        match n.kind() {
            "type_annotation" | "parenthesized_type" | "readonly_type" => named_kids(n).last().map(|k| self.type_name(*k)).unwrap_or_default(),
            "generic_type" => {
                let name = n.child_by_field_name("name").map(|x| self.text(x).to_string()).unwrap_or_default();
                let arg = n.child_by_field_name("type_arguments").and_then(|a| named_kids(a).first().map(|k| self.type_name(*k)));
                match name.as_str() {
                    "Array" | "ReadonlyArray" => "[]".into(),
                    "Promise" => format!("Promise<{}>", arg.unwrap_or_default()),
                    _ => name,
                }
            }
            "array_type" => "[]".into(),
            "union_type" => named_kids(n).iter().map(|k| self.type_name(*k)).find(|t| !matches!(t.as_str(), "null" | "undefined" | "")).unwrap_or_default(),
            "literal_type" => named_kids(n)
                .first()
                .map(|k| match k.kind() {
                    "string" => "string".to_string(),
                    "number" => "number".to_string(),
                    _ => self.text(*k).to_string(),
                })
                .unwrap_or_default(),
            _ => self.text(n).to_string(),
        }
    }

    fn infer(&self, n: Node) -> Option<TypeExpr> {
        match n.kind() {
            "new_expression" => {
                let ctor = n.child_by_field_name("constructor")?;
                self.path(ctor).map(TypeExpr::Named)
            }
            "call_expression" => {
                let f = n.child_by_field_name("function")?;
                let callee = self.chain_path(f)?;
                if callee == "inject" {
                    let arg = n.child_by_field_name("arguments").and_then(|a| named_kids(a).into_iter().next())?;
                    let arg = if arg.kind() == "instantiation_expression" { named_kids(arg).into_iter().next()? } else { arg };
                    return self.path(arg).map(TypeExpr::Named);
                }
                Some(TypeExpr::CallOf(callee))
            }
            "await_expression" => named_kids(n).first().and_then(|k| self.infer(*k)).map(|t| TypeExpr::Await(Box::new(t))),
            "as_expression" | "satisfies_expression" => named_kids(n).last().map(|t| TypeExpr::Named(self.type_name(*t))),
            "non_null_expression" | "parenthesized_expression" => named_kids(n).first().and_then(|k| self.infer(*k)),
            "identifier" | "member_expression" => self.chain_path(n).map(TypeExpr::Path),
            "string" | "template_string" => Some(TypeExpr::Named("string".into())),
            "number" => Some(TypeExpr::Named("number".into())),
            "true" | "false" => Some(TypeExpr::Named("boolean".into())),
            "array" => Some(TypeExpr::Named("[]".into())),
            _ => None,
        }
    }

    fn params(&self, list: Option<Node>, f: &mut TsFunc, fields: Option<&mut Vec<(String, TypeExpr)>>) {
        let Some(list) = list else { return };
        let mut fields = fields;
        for p in named_kids(list) {
            match p.kind() {
                "required_parameter" | "optional_parameter" => {
                    let name = p.child_by_field_name("pattern").filter(|x| x.kind() == "identifier").map(|x| self.text(x).to_string());
                    let ty = p.child_by_field_name("type").map(|t| self.type_name(t)).unwrap_or_default();
                    for k in named_kids(p) {
                        if k.kind() == "decorator" {
                            f.decorators.push(self.decorator(k).0);
                        }
                    }
                    let property = named_kids(p).iter().any(|k| k.kind() == "accessibility_modifier") || has_token(p, "readonly");
                    if let (true, Some(name), Some(fields)) = (property, &name, fields.as_deref_mut()) {
                        fields.push((name.clone(), TypeExpr::Named(ty.clone())));
                        let declared = p.child_by_field_name("type").map(|t| annotation_text(self, t));
                        f.param_fields.push(FieldDecl {
                            name: name.clone(),
                            ty: declared.unwrap_or_default(),
                            visibility: member_visibility(self, p, name),
                            line: line(p),
                        });
                    }
                    f.params.push((name.unwrap_or_default(), ty));
                }
                "identifier" => f.params.push((self.text(p).to_string(), String::new())),
                "assignment_pattern" => {
                    let name = p.child_by_field_name("left").filter(|x| x.kind() == "identifier").map(|x| self.text(x).to_string()).unwrap_or_default();
                    f.params.push((name, String::new()));
                }
                _ => f.params.push((String::new(), String::new())),
            }
        }
    }

    fn top(&self, n: Node, out: &mut TsFile) {
        self.collect(n, &mut out.top);
        self.scan(n, &mut out.top);
    }

    fn scan(&self, n: Node, f: &mut TsFunc) {
        self.uses(n, &mut f.type_uses);
        self.declared_names(n, &mut f.declared);
    }

    fn uses(&self, root: Node, out: &mut Vec<TsUse>) {
        let mut skip = Vec::new();
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            match n.kind() {
                "type_parameter" => skip.extend(n.child_by_field_name("name").map(|x| self.text(x).to_string())),
                "class_declaration" | "abstract_class_declaration" | "interface_declaration" | "type_alias_declaration" | "enum_declaration" if n != root => {
                    skip.extend(n.child_by_field_name("name").map(|x| self.text(x).to_string()))
                }
                _ => {}
            }
            stack.extend(named_kids(n));
        }
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            match n.kind() {
                "type_identifier" => {
                    let declares = n.parent().is_some_and(|p| {
                        matches!(
                            p.kind(),
                            "type_parameter"
                                | "class_declaration"
                                | "abstract_class_declaration"
                                | "class"
                                | "interface_declaration"
                                | "type_alias_declaration"
                        ) && p.child_by_field_name("name") == Some(n)
                    });
                    let name = self.text(n);
                    if !declares && !skip.iter().any(|s| s == name) {
                        out.push(TsUse { name: name.to_string(), line: line(n), col: col(n), len: len(n) });
                    }
                }
                "nested_type_identifier" => {
                    if let (Some(module), Some(name)) = (n.child_by_field_name("module"), n.child_by_field_name("name"))
                        && module.kind() == "identifier"
                    {
                        out.push(TsUse { name: format!("{}.{}", self.text(module), self.text(name)), line: line(name), col: col(name), len: len(name) });
                    }
                }
                "class_declaration" | "class" | "abstract_class_declaration" if n != root => {}
                _ => stack.extend(named_kids(n)),
            }
        }
    }

    fn expr_use(&self, n: Node, out: &mut Vec<TsUse>) {
        match n.kind() {
            "identifier" => out.push(TsUse { name: self.text(n).to_string(), line: line(n), col: col(n), len: len(n) }),
            "member_expression" => {
                if let (Some(obj), Some(prop)) = (n.child_by_field_name("object"), n.child_by_field_name("property"))
                    && obj.kind() == "identifier"
                    && prop.kind() == "property_identifier"
                {
                    out.push(TsUse { name: format!("{}.{}", self.text(obj), self.text(prop)), line: line(prop), col: col(prop), len: len(prop) });
                }
            }
            _ => {}
        }
    }

    fn type_param_names(&self, n: Node) -> Vec<String> {
        let Some(list) = n.child_by_field_name("type_parameters") else { return Vec::new() };
        named_kids(list).into_iter().filter_map(|p| p.child_by_field_name("name")).map(|x| self.text(x).to_string()).collect()
    }

    fn drop_type_params(&self, n: Node, t: &mut TsType) {
        let skip = self.type_param_names(n);
        if skip.is_empty() {
            return;
        }
        t.type_uses.retain(|u| !skip.contains(&u.name));
        for m in &mut t.methods {
            m.type_uses.retain(|u| !skip.contains(&u.name));
        }
    }

    fn pattern_names(&self, n: Node, out: &mut Vec<String>) {
        match n.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => out.push(self.text(n).to_string()),
            "object_pattern" | "array_pattern" | "rest_pattern" => {
                for k in named_kids(n) {
                    self.pattern_names(k, out);
                }
            }
            "pair_pattern" => {
                if let Some(v) = n.child_by_field_name("value") {
                    self.pattern_names(v, out);
                }
            }
            "assignment_pattern" | "object_assignment_pattern" => {
                if let Some(l) = n.child_by_field_name("left") {
                    self.pattern_names(l, out);
                }
            }
            _ => {}
        }
    }

    fn declared_names(&self, root: Node, out: &mut Vec<String>) {
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            match n.kind() {
                "required_parameter" | "optional_parameter" => {
                    if let Some(p) = n.child_by_field_name("pattern") {
                        self.pattern_names(p, out);
                    }
                }
                "formal_parameters" => {
                    for k in named_kids(n).into_iter().filter(|k| !matches!(k.kind(), "required_parameter" | "optional_parameter" | "comment")) {
                        self.pattern_names(k, out);
                    }
                }
                "variable_declarator" => {
                    if let Some(p) = n.child_by_field_name("name") {
                        self.pattern_names(p, out);
                    }
                }
                "arrow_function" => {
                    if let Some(p) = n.child_by_field_name("parameter") {
                        self.pattern_names(p, out);
                    }
                }
                "catch_clause" => {
                    if let Some(p) = n.child_by_field_name("parameter") {
                        self.pattern_names(p, out);
                    }
                }
                "for_in_statement" => {
                    if let Some(l) = n.child_by_field_name("left") {
                        self.pattern_names(l, out);
                    }
                }
                "function_declaration" | "generator_function_declaration" | "function_expression" | "class_declaration" | "abstract_class_declaration"
                    if n != root =>
                {
                    out.extend(n.child_by_field_name("name").map(|x| self.text(x).to_string()))
                }
                _ => {}
            }
            stack.extend(named_kids(n));
        }
    }

    fn function(&self, name: &str, sub: SubKind, n: Node, fields: Option<&mut Vec<(String, TypeExpr)>>) -> TsFunc {
        let mut f = TsFunc {
            name: name.to_string(),
            sub,
            complexity: cyclomatic(n, self.src, &DECISIONS),
            start_line: line(n),
            end_line: end_line(n),
            ..Default::default()
        };
        match n.child_by_field_name("parameters") {
            Some(list) => self.params(Some(list), &mut f, fields),
            None => {
                if let Some(p) = n.child_by_field_name("parameter") {
                    f.params.push((self.text(p).to_string(), String::new()));
                }
            }
        }
        f.returns = n.child_by_field_name("return_type").map(|t| self.type_name(t));
        if let Some(body) = n.child_by_field_name("body") {
            self.collect(body, &mut f);
        }
        f.calls.sort_by_key(|c| c.line);
        self.scan(n, &mut f);
        f
    }

    fn decorator(&self, n: Node) -> (Annotation, Vec<String>) {
        let mut refs = Vec::new();
        let Some(inner) = named_kids(n).into_iter().next() else { return (Annotation::default(), refs) };
        let (callee, args) = match inner.kind() {
            "call_expression" => (inner.child_by_field_name("function"), inner.child_by_field_name("arguments")),
            _ => (Some(inner), None),
        };
        let full = callee.map(|c| self.text(c)).unwrap_or("");
        let name = full.rsplit('.').next().unwrap_or(full).to_string();
        let mut out = Vec::new();
        if let Some(args) = args {
            for a in named_kids(args) {
                self.decorator_value(a, &mut out, &mut refs);
            }
        }
        (Annotation { name, args: out }, refs)
    }

    fn decorator_value(&self, n: Node, strings: &mut Vec<String>, refs: &mut Vec<String>) {
        if let Some(s) = self.string(n) {
            strings.push(s);
            return;
        }
        match n.kind() {
            "identifier" | "member_expression" => refs.extend(self.path(n)),
            "object" => {
                for pair in named_kids(n).into_iter().filter(|p| p.kind() == "pair") {
                    if let Some(v) = pair.child_by_field_name("value") {
                        self.decorator_value(v, strings, refs);
                    }
                }
            }
            "array" => {
                for k in named_kids(n) {
                    self.decorator_value(k, strings, refs);
                }
            }
            _ => {}
        }
    }

    fn collect(&self, body: Node, f: &mut TsFunc) {
        let mut stack = vec![body];
        while let Some(n) = stack.pop() {
            match n.kind() {
                "class_declaration" | "class" | "abstract_class_declaration" => continue,
                "call_expression" => self.call(n, f),
                "new_expression" => {
                    if let Some(ctor) = n.child_by_field_name("constructor")
                        && let Some(name) = self.path(ctor)
                    {
                        let token = if ctor.kind() == "member_expression" { ctor.child_by_field_name("property").unwrap_or(ctor) } else { ctor };
                        let (string_args, ident_args) = self.args(n.child_by_field_name("arguments"));
                        f.calls.push(TsCall {
                            method: name,
                            is_new: true,
                            string_args,
                            ident_args,
                            assigned_to: self.assigned_to(n),
                            line: line(token),
                            col: col(token),
                            len: len(token),
                            ..Default::default()
                        });
                    }
                }
                "variable_declarator" => {
                    if let Some(name) = n.child_by_field_name("name").filter(|x| x.kind() == "identifier") {
                        let value = n.child_by_field_name("value");
                        let ty = match value {
                            Some(v) if is_function(v) => Some(TypeExpr::Named("Function".into())),
                            _ => n.child_by_field_name("type").map(|t| TypeExpr::Named(self.type_name(t))).or_else(|| value.and_then(|v| self.infer(v))),
                        };
                        if let Some(ty) = ty {
                            f.locals.push((self.text(name).to_string(), ty));
                        }
                    }
                }
                "arrow_function" | "function_expression" | "function" if n != body => {
                    let mut inner = TsFunc::default();
                    self.params(n.child_by_field_name("parameters"), &mut inner, None);
                    f.locals.extend(inner.params.into_iter().filter(|(n, t)| !n.is_empty() && !t.is_empty()).map(|(n, t)| (n, TypeExpr::Named(t))));
                }
                _ => {}
            }
            let kids = named_kids(n);
            stack.extend(kids.into_iter().rev());
        }
    }

    fn args(&self, args: Option<Node>) -> (Vec<Option<String>>, Vec<Option<String>>) {
        let (mut strings, mut idents) = (Vec::new(), Vec::new());
        if let Some(args) = args {
            for a in named_kids(args).into_iter().filter(|a| a.kind() != "comment") {
                strings.push(self.string(a));
                idents.push(matches!(a.kind(), "identifier" | "member_expression").then(|| self.path(a)).flatten());
            }
        }
        (strings, idents)
    }

    fn assigned_to(&self, n: Node) -> Option<String> {
        let mut p = n.parent()?;
        while matches!(p.kind(), "await_expression" | "parenthesized_expression" | "non_null_expression" | "as_expression") {
            p = p.parent()?;
        }
        match p.kind() {
            "variable_declarator" => p.child_by_field_name("name").filter(|x| x.kind() == "identifier").map(|x| self.text(x).to_string()),
            "assignment_expression" => p.child_by_field_name("left").and_then(|l| self.path(l)),
            _ => None,
        }
    }

    fn call(&self, n: Node, f: &mut TsFunc) {
        let Some(func) = n.child_by_field_name("function") else { return };
        let (receiver, method, token) = match func.kind() {
            "identifier" => (None, self.text(func).to_string(), func),
            "member_expression" => {
                let Some(prop) = func.child_by_field_name("property") else { return };
                let recv = func.child_by_field_name("object").and_then(|o| self.chain_path(o)).unwrap_or_else(|| CHAIN.to_string());
                (Some(recv), self.text(prop).to_string(), prop)
            }
            _ => return,
        };
        if receiver.is_none() && method == "require" {
            return;
        }
        let (string_args, ident_args) = self.args(n.child_by_field_name("arguments"));
        f.calls.push(TsCall {
            receiver,
            method,
            string_args,
            ident_args,
            is_new: false,
            assigned_to: self.assigned_to(n),
            line: line(token),
            col: col(token),
            len: len(token),
            in_field: false,
        });
    }

    fn import_of(&self, n: Node) -> Option<String> {
        if n.kind() != "call_expression" {
            return None;
        }
        let func = n.child_by_field_name("function")?;
        if !(func.kind() == "import" || (func.kind() == "identifier" && self.text(func) == "require")) {
            return None;
        }
        let arg = named_kids(n.child_by_field_name("arguments")?).into_iter().next()?;
        self.string(arg)
    }

    fn dynamic_imports(&self, n: Node, out: &mut TsFile) {
        let mut stack = vec![n];
        while let Some(x) = stack.pop() {
            if let Some(spec) = self.import_of(x) {
                out.imports.push(TsImport { spec, dynamic: true, ..Default::default() });
            }
            stack.extend(named_kids(x));
        }
    }

    fn statement(&self, n: Node, out: &mut TsFile, decorators: &[Node]) {
        match n.kind() {
            "import_statement" => self.import(n, out),
            "export_statement" => self.export(n, out),
            "class_declaration" | "abstract_class_declaration" | "class" => {
                if let Some(t) = self.class(n, decorators) {
                    out.types.push(t);
                }
            }
            "interface_declaration" => out.types.push(self.interface(n)),
            "type_alias_declaration" => {
                let Some(name) = n.child_by_field_name("name") else { return };
                out.types.push(self.type_alias(n, self.text(name)));
            }
            "enum_declaration" => {
                let Some(name) = n.child_by_field_name("name") else { return };
                out.types.push(TsType {
                    name: self.text(name).to_string(),
                    kind: SubKind::Enum,
                    extends: Vec::new(),
                    implements: Vec::new(),
                    decorators: Vec::new(),
                    decorator_refs: Vec::new(),
                    fields: Vec::new(),
                    field_decls: Vec::new(),
                    methods: Vec::new(),
                    type_uses: Vec::new(),
                    start_line: line(n),
                    end_line: end_line(n),
                });
                if let Some(t) = out.types.last_mut() {
                    self.uses(n, &mut t.type_uses);
                }
            }
            "function_declaration" | "generator_function_declaration" => {
                let name = n.child_by_field_name("name").map(|x| self.text(x).to_string()).unwrap_or_default();
                out.funcs.push(self.function(&name, SubKind::Function, n, None));
            }
            "lexical_declaration" | "variable_declaration" => {
                for d in named_kids(n).into_iter().filter(|d| d.kind() == "variable_declarator") {
                    self.declarator(d, out);
                }
            }
            "expression_statement" => {
                if let Some(f) = self.exports_assignment(n) {
                    out.funcs.push(f);
                    return;
                }
                self.dynamic_imports(n, out);
                self.top(n, out);
            }
            "ambient_declaration" | "internal_module" | "module" | "comment" | "empty_statement" => {}
            _ => {
                self.dynamic_imports(n, out);
                self.top(n, out);
            }
        }
    }

    fn exports_assignment(&self, n: Node) -> Option<TsFunc> {
        let a = named_kids(n).into_iter().next().filter(|a| a.kind() == "assignment_expression")?;
        let left = self.path(a.child_by_field_name("left")?)?;
        let right = a.child_by_field_name("right").filter(|r| is_function(*r))?;
        let name = left.strip_prefix("module.exports.").or_else(|| left.strip_prefix("exports."))?;
        (!name.contains('.')).then(|| self.function(name, SubKind::Function, right, None))
    }

    fn declarator(&self, d: Node, out: &mut TsFile) {
        let name = d.child_by_field_name("name");
        let value = d.child_by_field_name("value");
        if let Some(spec) = value.and_then(|v| self.import_of(v)) {
            let mut imp = TsImport { spec, ..Default::default() };
            match name.map(|x| x.kind()) {
                Some("identifier") => imp.default = Some(self.text(name.unwrap()).to_string()),
                Some("object_pattern") => {
                    for p in named_kids(name.unwrap()) {
                        match p.kind() {
                            "shorthand_property_identifier_pattern" => imp.named.push((self.text(p).to_string(), self.text(p).to_string())),
                            "pair_pattern" => {
                                let (k, v) = (p.child_by_field_name("key"), p.child_by_field_name("value"));
                                if let (Some(k), Some(v)) = (k, v) {
                                    imp.named.push((self.text(k).to_string(), self.text(v).to_string()));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            out.imports.push(imp);
            return;
        }
        let Some(name) = name.filter(|x| x.kind() == "identifier") else {
            if let Some(v) = value {
                self.top(v, out);
            }
            return;
        };
        let name = self.text(name).to_string();
        match value {
            Some(v) if is_function(v) => out.funcs.push(self.function(&name, SubKind::Function, v, None)),
            Some(v) if v.kind() == "class" => {
                if let Some(mut t) = self.class(v, &[]) {
                    t.name = name;
                    out.types.push(t);
                }
            }
            _ => {
                let ty = d.child_by_field_name("type").map(|t| TypeExpr::Named(self.type_name(t))).or_else(|| value.and_then(|v| self.infer(v)));
                if let Some(ty) = ty {
                    out.vars.push((name, ty));
                }
                self.uses(d, &mut out.top.type_uses);
                if let Some(v) = value {
                    self.dynamic_imports(v, out);
                    self.top(v, out);
                }
            }
        }
    }

    fn import(&self, n: Node, out: &mut TsFile) {
        let Some(spec) = n.child_by_field_name("source").and_then(|s| self.string(s)) else {
            if let Some(req) = named_kids(n).into_iter().find(|k| k.kind() == "import_require_clause") {
                let local = named_kids(req).into_iter().find(|k| k.kind() == "identifier").map(|k| self.text(k).to_string());
                let spec = req.child_by_field_name("source").and_then(|s| self.string(s)).unwrap_or_default();
                out.imports.push(TsImport { spec, default: local, ..Default::default() });
            }
            return;
        };
        let mut imp = TsImport { spec, ..Default::default() };
        if let Some(clause) = named_kids(n).into_iter().find(|k| k.kind() == "import_clause") {
            for part in named_kids(clause) {
                match part.kind() {
                    "identifier" => imp.default = Some(self.text(part).to_string()),
                    "namespace_import" => imp.namespace = named_kids(part).first().map(|i| self.text(*i).to_string()),
                    "named_imports" => {
                        for s in named_kids(part).into_iter().filter(|s| s.kind() == "import_specifier") {
                            let name = s.child_by_field_name("name").map(|x| self.text(x).trim_matches(['"', '\'']).to_string()).unwrap_or_default();
                            let alias = s.child_by_field_name("alias").map(|x| self.text(x).to_string()).unwrap_or_else(|| name.clone());
                            imp.named.push((name, alias));
                        }
                    }
                    _ => {}
                }
            }
        }
        out.imports.push(imp);
    }

    fn export(&self, n: Node, out: &mut TsFile) {
        let decorators: Vec<Node> = named_kids(n).into_iter().filter(|k| k.kind() == "decorator").collect();
        let is_default = has_token(n, "default");
        let source = n.child_by_field_name("source").and_then(|s| self.string(s));
        if let Some(decl) = n.child_by_field_name("declaration") {
            let before = (out.types.len(), out.funcs.len());
            self.statement(decl, out, &decorators);
            let declared: Vec<String> =
                out.types[before.0..].iter().map(|t| t.name.clone()).chain(out.funcs[before.1..].iter().map(|f| f.name.clone())).collect();
            out.exports.extend(declared.into_iter().map(|n| (n.clone(), n)));
            if is_default {
                out.default_export = out.types.get(before.0).map(|t| t.name.clone()).or_else(|| out.funcs.get(before.1).map(|f| f.name.clone()));
            }
            return;
        }
        if let Some(v) = n.child_by_field_name("value") {
            match v.kind() {
                "identifier" => out.default_export = Some(self.text(v).to_string()),
                k if is_function(v) || k == "class" => {
                    let name = v.child_by_field_name("name").map(|x| self.text(x).to_string()).unwrap_or_else(|| "default".into());
                    if k == "class" {
                        if let Some(mut t) = self.class(v, &decorators) {
                            t.name = name.clone();
                            out.types.push(t);
                        }
                    } else {
                        out.funcs.push(self.function(&name, SubKind::Function, v, None));
                    }
                    out.default_export = Some(name);
                }
                _ => {
                    self.dynamic_imports(v, out);
                    self.top(v, out);
                }
            }
            return;
        }
        let clause = named_kids(n).into_iter().find(|k| k.kind() == "export_clause");
        let names: Option<Vec<(String, String)>> = clause.map(|c| {
            named_kids(c)
                .into_iter()
                .filter(|s| s.kind() == "export_specifier")
                .map(|s| {
                    let name = s.child_by_field_name("name").map(|x| self.text(x).to_string()).unwrap_or_default();
                    let alias = s.child_by_field_name("alias").map(|x| self.text(x).to_string()).unwrap_or_else(|| name.clone());
                    (name, alias)
                })
                .collect()
        });
        match source {
            Some(spec) => {
                if names.is_none() && named_kids(n).iter().any(|k| k.kind() == "namespace_export") {
                    return;
                }
                out.reexports.push(ReExport { spec, names });
            }
            None => out.exports.extend(names.unwrap_or_default()),
        }
    }

    fn heritage(&self, n: Node, t: &mut TsType) {
        for h in named_kids(n).into_iter().filter(|h| h.kind() == "class_heritage") {
            for clause in named_kids(h) {
                match clause.kind() {
                    "extends_clause" => {
                        if let Some(value) = clause.child_by_field_name("value") {
                            t.extends.extend(self.path(value));
                            self.expr_use(value, &mut t.type_uses);
                        }
                        self.uses(clause, &mut t.type_uses);
                    }
                    "implements_clause" => {
                        t.implements.extend(named_kids(clause).iter().map(|k| self.type_name(*k)));
                        self.uses(clause, &mut t.type_uses);
                    }
                    _ => {
                        t.extends.extend(self.path(clause));
                        self.expr_use(clause, &mut t.type_uses);
                    }
                }
            }
        }
    }

    fn class(&self, n: Node, outer_decorators: &[Node]) -> Option<TsType> {
        let name = n.child_by_field_name("name").map(|x| self.text(x).to_string()).unwrap_or_else(|| "default".into());
        let mut t = TsType {
            name,
            kind: SubKind::Class,
            extends: Vec::new(),
            implements: Vec::new(),
            decorators: Vec::new(),
            decorator_refs: Vec::new(),
            fields: Vec::new(),
            field_decls: Vec::new(),
            methods: Vec::new(),
            type_uses: Vec::new(),
            start_line: line(n),
            end_line: end_line(n),
        };
        let own: Vec<Node> = named_kids(n).into_iter().filter(|k| k.kind() == "decorator").collect();
        for d in outer_decorators.iter().chain(&own) {
            let (a, refs) = self.decorator(*d);
            t.decorators.push(a);
            t.decorator_refs.extend(refs);
        }
        if let Some(first) = outer_decorators.first() {
            t.start_line = t.start_line.min(line(*first));
        }
        self.heritage(n, &mut t);
        let Some(body) = n.child_by_field_name("body") else {
            self.drop_type_params(n, &mut t);
            return Some(t);
        };
        let mut pending: Vec<Annotation> = Vec::new();
        for m in named_kids(body) {
            match m.kind() {
                "decorator" => pending.push(self.decorator(m).0),
                "method_definition" | "abstract_method_signature" | "method_signature" => {
                    let name = m.child_by_field_name("name").map(|x| self.text(x).to_string()).unwrap_or_default();
                    let sub = if name == "constructor" { SubKind::Constructor } else { SubKind::Method };
                    let mut f = self.function(&name, sub, m, Some(&mut t.fields));
                    f.visibility = member_visibility(self, m, &name);
                    f.decorators.splice(0..0, pending.drain(..));
                    if let Some(prev) = t.methods.iter_mut().find(|p| p.name == name) {
                        if sub == SubKind::Constructor {
                            f.calls.append(&mut prev.calls);
                            f.locals.append(&mut prev.locals);
                            f.calls.sort_by_key(|c| c.line);
                            f.decorators.append(&mut prev.decorators);
                            f.type_uses.append(&mut prev.type_uses);
                            f.declared.append(&mut prev.declared);
                            *prev = f;
                        } else if prev.end_line - prev.start_line < f.end_line - f.start_line {
                            f.decorators.append(&mut prev.decorators);
                            f.type_uses.append(&mut prev.type_uses);
                            *prev = f;
                        } else {
                            prev.type_uses.append(&mut f.type_uses);
                        }
                    } else {
                        t.methods.push(f);
                    }
                }
                "public_field_definition" | "field_definition" => {
                    let mut decorators: Vec<Annotation> = std::mem::take(&mut pending);
                    decorators.extend(named_kids(m).into_iter().filter(|k| k.kind() == "decorator").map(|k| self.decorator(k).0));
                    let Some(name) = m.child_by_field_name("name").or_else(|| m.child_by_field_name("property")) else { continue };
                    let name = self.text(name).to_string();
                    let value = m.child_by_field_name("value");
                    match value {
                        Some(v) if is_function(v) => {
                            let mut f = self.function(&name, SubKind::Method, v, None);
                            f.visibility = member_visibility(self, m, &name);
                            f.start_line = line(m);
                            f.decorators = decorators;
                            t.methods.push(f);
                            if let Some(ty) = m.child_by_field_name("type") {
                                self.uses(ty, &mut t.type_uses);
                            }
                        }
                        _ => {
                            let declared = m.child_by_field_name("type").map(|x| annotation_text(self, x)).unwrap_or_default();
                            t.field_decls.push(FieldDecl { name: name.clone(), ty: declared, visibility: member_visibility(self, m, &name), line: line(m) });
                            let ty = m.child_by_field_name("type").map(|x| TypeExpr::Named(self.type_name(x))).or_else(|| value.and_then(|v| self.infer(v)));
                            if let Some(ty) = ty {
                                t.fields.push((name, ty));
                            }
                            self.uses(m, &mut t.type_uses);
                            if let Some(v) = value {
                                let ctor = match t.methods.iter_mut().find(|f| f.sub == SubKind::Constructor) {
                                    Some(c) => c,
                                    None => {
                                        t.methods.push(TsFunc {
                                            name: "constructor".into(),
                                            sub: SubKind::Constructor,
                                            start_line: line(m),
                                            end_line: end_line(m),
                                            ..Default::default()
                                        });
                                        t.methods.last_mut().unwrap()
                                    }
                                };
                                let before = ctor.calls.len();
                                self.collect(v, ctor);
                                ctor.calls[before..].iter_mut().for_each(|c| c.in_field = true);
                                self.declared_names(v, &mut ctor.declared);
                            }
                        }
                    }
                }
                _ => pending.clear(),
            }
        }
        self.drop_type_params(n, &mut t);
        let from_params: Vec<FieldDecl> = t.methods.iter().filter(|f| f.sub == SubKind::Constructor).flat_map(|f| f.param_fields.clone()).collect();
        t.field_decls.extend(from_params);
        Some(t)
    }

    fn interface(&self, n: Node) -> TsType {
        let mut t = TsType {
            name: n.child_by_field_name("name").map(|x| self.text(x).to_string()).unwrap_or_default(),
            kind: SubKind::Interface,
            extends: Vec::new(),
            implements: Vec::new(),
            decorators: Vec::new(),
            decorator_refs: Vec::new(),
            fields: Vec::new(),
            field_decls: Vec::new(),
            methods: Vec::new(),
            type_uses: Vec::new(),
            start_line: line(n),
            end_line: end_line(n),
        };
        for k in named_kids(n) {
            if k.kind() == "extends_type_clause" {
                t.extends.extend(named_kids(k).iter().map(|x| self.type_name(*x)));
                self.uses(k, &mut t.type_uses);
            }
        }
        if let Some(body) = n.child_by_field_name("body") {
            self.members(body, &mut t);
        }
        self.drop_type_params(n, &mut t);
        t
    }

    fn members(&self, body: Node, t: &mut TsType) {
        for m in named_kids(body) {
            if m.kind() == "call_signature" {
                if !t.methods.iter().any(|f| f.name == CALL_SIGNATURE) {
                    t.methods.push(self.function(CALL_SIGNATURE, SubKind::Method, m, None));
                }
                continue;
            }
            let Some(name) = m.child_by_field_name("name").map(|x| self.text(x).to_string()) else { continue };
            match m.kind() {
                "method_signature" => {
                    if !t.methods.iter().any(|f| f.name == name) {
                        t.methods.push(self.function(&name, SubKind::Method, m, None));
                    }
                }
                "property_signature" => {
                    let ty = m.child_by_field_name("type");
                    let func = ty.and_then(|a| named_kids(a).into_iter().find(|k| k.kind() == "function_type"));
                    if let Some(func) = func {
                        if !t.methods.iter().any(|f| f.name == name) {
                            let mut f = self.function(&name, SubKind::Method, func, None);
                            f.start_line = line(m);
                            t.methods.push(f);
                        }
                        self.uses(m, &mut t.type_uses);
                        continue;
                    }
                    let declared = ty.map(|x| annotation_text(self, x)).unwrap_or_default();
                    t.field_decls.push(FieldDecl { name: name.clone(), ty: declared, visibility: Visibility::Public, line: line(m) });
                    self.uses(m, &mut t.type_uses);
                    if let Some(ty) = ty {
                        t.fields.push((name, TypeExpr::Named(self.type_name(ty))));
                    }
                }
                _ => {}
            }
        }
    }

    fn type_alias(&self, n: Node, name: &str) -> TsType {
        let mut t = TsType {
            name: name.to_string(),
            kind: SubKind::NamedType,
            extends: Vec::new(),
            implements: Vec::new(),
            decorators: Vec::new(),
            decorator_refs: Vec::new(),
            fields: Vec::new(),
            field_decls: Vec::new(),
            methods: Vec::new(),
            type_uses: Vec::new(),
            start_line: line(n),
            end_line: end_line(n),
        };
        self.uses(n, &mut t.type_uses);
        let mut parts = Vec::new();
        if let Some(v) = n.child_by_field_name("value") {
            self.flatten(v, &mut parts);
        }
        let intersection = parts.len() > 1;
        for p in parts {
            if p.kind() == "object_type" {
                self.members(p, &mut t);
            } else if intersection {
                let sup = self.type_name(p);
                if !sup.is_empty() {
                    t.extends.push(sup);
                }
            }
        }
        self.drop_type_params(n, &mut t);
        t
    }

    fn flatten<'t>(&self, n: Node<'t>, out: &mut Vec<Node<'t>>) {
        match n.kind() {
            "intersection_type" | "parenthesized_type" => named_kids(n).into_iter().for_each(|k| self.flatten(k, out)),
            _ => out.push(n),
        }
    }
}
