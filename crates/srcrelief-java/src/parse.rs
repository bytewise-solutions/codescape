use srcrelief_facts::{Annotation, Import};
use srcrelief_format::SubKind;
use tree_sitter::{Node, Parser};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct JavaFile {
    pub package: String,
    pub imports: Vec<Import>,
    pub types: Vec<JType>,
    pub lines: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JType {
    pub name: String,
    pub kind: SubKind,
    pub extends: Vec<String>,
    pub implements: Vec<String>,
    pub annotations: Vec<Annotation>,
    pub fields: Vec<(String, String)>,
    pub methods: Vec<JMethod>,
    pub nested: Vec<JType>,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct JMethod {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub return_type: String,
    pub is_static: bool,
    pub is_public: bool,
    pub annotations: Vec<Annotation>,
    pub locals: Vec<(String, String)>,
    pub calls: Vec<JCall>,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Receiver {
    None,
    This,
    Super,
    Ident(String),
    Field(String),
    Qualified(String),
    Chain,
    Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JCall {
    pub receiver: Receiver,
    pub method: String,
    pub string_args: Vec<Option<String>>,
    pub ident_args: Vec<Option<String>>,
    pub is_new: bool,
    pub assigned_to: Option<String>,
    pub line: u32,
}

pub fn parse_java(src: &[u8]) -> JavaFile {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_java::LANGUAGE.into()).expect("java grammar");
    let Some(tree) = parser.parse(src, None) else { return JavaFile::default() };
    let mut out = JavaFile { lines: src.iter().filter(|&&b| b == b'\n').count() as u32 + 1, ..Default::default() };
    let p = Ctx { src };
    let root = tree.root_node();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        match child.kind() {
            "package_declaration" => {
                if let Some(n) = first_named_of(child, &["scoped_identifier", "identifier"]) {
                    out.package = p.text(n).to_string();
                }
            }
            "import_declaration" => out.imports.push(p.import(child)),
            _ => {
                if let Some(t) = p.type_decl(child) {
                    out.types.push(t);
                }
            }
        }
    }
    out
}

fn first_named_of<'t>(node: Node<'t>, kinds: &[&str]) -> Option<Node<'t>> {
    let mut c = node.walk();
    node.named_children(&mut c).find(|n| kinds.contains(&n.kind()))
}

fn line(n: Node) -> u32 {
    n.start_position().row as u32 + 1
}

fn end_line(n: Node) -> u32 {
    n.end_position().row as u32 + 1
}

struct Ctx<'s> {
    src: &'s [u8],
}

impl<'s> Ctx<'s> {
    fn text(&self, n: Node) -> &'s str {
        n.utf8_text(self.src).unwrap_or("")
    }

    fn import(&self, n: Node) -> Import {
        let mut imp = Import::default();
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            match ch.kind() {
                "static" => imp.is_static = true,
                "asterisk" => imp.wildcard = true,
                "scoped_identifier" | "identifier" => imp.path = self.text(ch).to_string(),
                _ => {}
            }
        }
        imp
    }

    fn type_name(&self, n: Node) -> String {
        match n.kind() {
            "generic_type" => first_named_of(n, &["type_identifier", "scoped_type_identifier"])
                .map(|t| self.type_name(t))
                .unwrap_or_default(),
            "array_type" => n.child_by_field_name("element").map(|t| self.type_name(t)).unwrap_or_default(),
            "annotated_type" => {
                let mut c = n.walk();
                n.named_children(&mut c).filter(|x| !x.kind().ends_with("annotation")).last().map(|t| self.type_name(t)).unwrap_or_default()
            }
            "scoped_type_identifier" => {
                let t = self.text(n);
                match t.find('<') {
                    Some(i) => t[..i].to_string(),
                    None => t.to_string(),
                }
            }
            _ => self.text(n).to_string(),
        }
    }

    fn type_list(&self, n: Node) -> Vec<String> {
        let mut out = Vec::new();
        let mut c = n.walk();
        for ch in n.named_children(&mut c) {
            if ch.kind() == "type_list" {
                out.extend(self.type_list(ch));
            } else {
                out.push(self.type_name(ch));
            }
        }
        out
    }

    fn annotations(&self, modifiers: Option<Node>) -> Vec<Annotation> {
        let Some(m) = modifiers else { return Vec::new() };
        let mut out = Vec::new();
        let mut c = m.walk();
        for a in m.named_children(&mut c) {
            if !matches!(a.kind(), "marker_annotation" | "annotation") {
                continue;
            }
            let name = a.child_by_field_name("name").map(|n| self.text(n)).unwrap_or("");
            let name = name.rsplit('.').next().unwrap_or(name).to_string();
            let mut args = Vec::new();
            if let Some(list) = a.child_by_field_name("arguments") {
                self.annotation_args(list, &mut args);
            }
            out.push(Annotation { name, args });
        }
        out
    }

    fn annotation_args(&self, n: Node, out: &mut Vec<String>) {
        let mut c = n.walk();
        for ch in n.named_children(&mut c) {
            match ch.kind() {
                "string_literal" => out.push(self.string(ch)),
                "element_value_array_initializer" => self.annotation_args(ch, out),
                "element_value_pair" => {
                    let key = ch.child_by_field_name("key").map(|k| self.text(k)).unwrap_or("");
                    if matches!(key, "value" | "path") {
                        if let Some(v) = ch.child_by_field_name("value") {
                            if v.kind() == "string_literal" {
                                out.push(self.string(v));
                            } else {
                                self.annotation_args(v, out);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn string(&self, n: Node) -> String {
        let t = self.text(n);
        t.trim_start_matches("\"\"\"").trim_end_matches("\"\"\"").trim_matches('"').to_string()
    }

    fn modifiers(n: Node) -> Option<Node> {
        first_named_of(n, &["modifiers"])
    }

    fn has_modifier(&self, n: Node, word: &str) -> bool {
        Self::modifiers(n).is_some_and(|m| {
            let mut c = m.walk();
            m.children(&mut c).any(|x| self.text(x) == word)
        })
    }

    fn type_decl(&self, n: Node) -> Option<JType> {
        let kind = match n.kind() {
            "class_declaration" => SubKind::Class,
            "interface_declaration" => SubKind::Interface,
            "enum_declaration" => SubKind::Enum,
            "record_declaration" => SubKind::Record,
            "annotation_type_declaration" => SubKind::Annotation,
            _ => return None,
        };
        let name = self.text(n.child_by_field_name("name")?).to_string();
        let mut t = JType {
            name,
            kind,
            extends: Vec::new(),
            implements: Vec::new(),
            annotations: self.annotations(Self::modifiers(n)),
            fields: Vec::new(),
            methods: Vec::new(),
            nested: Vec::new(),
            start_line: line(n),
            end_line: end_line(n),
        };
        let mut c = n.walk();
        for ch in n.named_children(&mut c) {
            match ch.kind() {
                "superclass" | "extends_interfaces" => t.extends.extend(self.type_list(ch)),
                "super_interfaces" => t.implements.extend(self.type_list(ch)),
                _ => {}
            }
        }
        if let Some(body) = n.child_by_field_name("body") {
            self.type_body(body, &mut t);
        }
        Some(t)
    }

    fn type_body(&self, body: Node, t: &mut JType) {
        let mut c = body.walk();
        for m in body.named_children(&mut c) {
            match m.kind() {
                "enum_body_declarations" => self.type_body(m, t),
                "field_declaration" | "constant_declaration" => {
                    let ty = m.child_by_field_name("type").map(|x| self.type_name(x)).unwrap_or_default();
                    let mut dc = m.walk();
                    for d in m.children_by_field_name("declarator", &mut dc) {
                        if let Some(name) = d.child_by_field_name("name") {
                            t.fields.push((self.text(name).to_string(), ty.clone()));
                        }
                    }
                }
                "method_declaration" | "constructor_declaration" | "compact_constructor_declaration" | "annotation_type_element_declaration" => {
                    t.methods.push(self.method(m))
                }
                "static_initializer" => {
                    let mut jm = JMethod { name: "<clinit>".into(), is_static: true, start_line: line(m), end_line: end_line(m), ..Default::default() };
                    self.collect_body(m, &mut jm);
                    t.methods.push(jm);
                }
                _ => {
                    if let Some(inner) = self.type_decl(m) {
                        t.nested.push(inner);
                    }
                }
            }
        }
    }

    fn method(&self, m: Node) -> JMethod {
        let is_ctor = m.kind() != "method_declaration" && m.kind() != "annotation_type_element_declaration";
        let name = if is_ctor { "<init>".to_string() } else { m.child_by_field_name("name").map(|n| self.text(n).to_string()).unwrap_or_default() };
        let mut jm = JMethod {
            name,
            return_type: m.child_by_field_name("type").map(|t| self.type_name(t)).unwrap_or_default(),
            is_static: self.has_modifier(m, "static"),
            is_public: self.has_modifier(m, "public"),
            annotations: self.annotations(Self::modifiers(m)),
            start_line: line(m),
            end_line: end_line(m),
            ..Default::default()
        };
        if let Some(params) = m.child_by_field_name("parameters") {
            let mut c = params.walk();
            for p in params.named_children(&mut c) {
                if !matches!(p.kind(), "formal_parameter" | "spread_parameter") {
                    continue;
                }
                let ty = p.child_by_field_name("type").or_else(|| {
                    first_named_of(p, &["type_identifier", "generic_type", "scoped_type_identifier", "array_type", "integral_type"])
                });
                let name = p
                    .child_by_field_name("name")
                    .or_else(|| first_named_of(p, &["variable_declarator"]).and_then(|v| v.child_by_field_name("name")));
                if let (Some(ty), Some(name)) = (ty, name) {
                    jm.params.push((self.text(name).to_string(), self.type_name(ty)));
                }
            }
        }
        if let Some(body) = m.child_by_field_name("body") {
            self.collect_body(body, &mut jm);
        }
        jm
    }

    fn collect_body(&self, n: Node, jm: &mut JMethod) {
        let mut stack = vec![n];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "local_variable_declaration" => {
                    if let Some(ty_node) = node.child_by_field_name("type") {
                        let declared = self.type_name(ty_node);
                        let mut dc = node.walk();
                        let declarators: Vec<Node> = node.children_by_field_name("declarator", &mut dc).collect();
                        for d in declarators {
                            if let Some(name) = d.child_by_field_name("name") {
                                let ty = self.infer_var(&declared, d.child_by_field_name("value"));
                                jm.locals.push((self.text(name).to_string(), ty));
                            }
                        }
                    }
                }
                "resource" => {
                    if let (Some(ty), Some(name)) = (node.child_by_field_name("type"), node.child_by_field_name("name")) {
                        let declared = self.type_name(ty);
                        jm.locals.push((self.text(name).to_string(), self.infer_var(&declared, node.child_by_field_name("value"))));
                    }
                }
                "enhanced_for_statement" => {
                    if let (Some(ty), Some(name)) = (node.child_by_field_name("type"), node.child_by_field_name("name")) {
                        jm.locals.push((self.text(name).to_string(), self.type_name(ty)));
                    }
                }
                "catch_formal_parameter" => {
                    let ty = first_named_of(node, &["catch_type"]).and_then(|ct| {
                        let mut c = ct.walk();
                        ct.named_children(&mut c).next()
                    });
                    if let (Some(ty), Some(name)) = (ty, node.child_by_field_name("name")) {
                        jm.locals.push((self.text(name).to_string(), self.type_name(ty)));
                    }
                }
                "method_invocation" => jm.calls.push(self.invocation(node)),
                "object_creation_expression" => {
                    if let Some(ty) = node.child_by_field_name("type") {
                        let (string_args, ident_args) = self.args(node.child_by_field_name("arguments"));
                        jm.calls.push(JCall {
                            receiver: Receiver::None,
                            method: self.type_name(ty),
                            string_args,
                            ident_args,
                            is_new: true,
                            assigned_to: self.assigned_to(node),
                            line: line(node),
                        });
                    }
                }
                "explicit_constructor_invocation" => {
                    let first = node.child(0).map(|c| self.text(c)).unwrap_or("");
                    let (string_args, ident_args) = self.args(node.child_by_field_name("arguments"));
                    jm.calls.push(JCall {
                        receiver: if first == "super" { Receiver::Super } else { Receiver::This },
                        method: "<init>".into(),
                        string_args,
                        ident_args,
                        is_new: false,
                        assigned_to: None,
                        line: line(node),
                    });
                }
                "lambda_expression" => {
                    if let Some(params) = node.child_by_field_name("parameters") {
                        if params.kind() == "identifier" {
                            jm.locals.push((self.text(params).to_string(), String::new()));
                        }
                    }
                }
                _ => {}
            }
            let mut c = node.walk();
            let kids: Vec<Node> = node.named_children(&mut c).collect();
            for k in kids.into_iter().rev() {
                stack.push(k);
            }
        }
        jm.calls.sort_by_key(|c| c.line);
    }

    fn infer_var(&self, declared: &str, value: Option<Node>) -> String {
        if declared != "var" {
            return declared.to_string();
        }
        match value {
            Some(v) if v.kind() == "object_creation_expression" => v.child_by_field_name("type").map(|t| self.type_name(t)).unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn assigned_to(&self, node: Node) -> Option<String> {
        let parent = node.parent()?;
        match parent.kind() {
            "variable_declarator" => parent.child_by_field_name("name").map(|n| self.text(n).to_string()),
            "assignment_expression" => parent.child_by_field_name("left").map(|n| self.text(n).to_string()),
            _ => None,
        }
    }

    fn args(&self, list: Option<Node>) -> (Vec<Option<String>>, Vec<Option<String>>) {
        let mut strings = Vec::new();
        let mut idents = Vec::new();
        let Some(list) = list else { return (strings, idents) };
        let mut c = list.walk();
        for a in list.named_children(&mut c) {
            strings.push((a.kind() == "string_literal").then(|| self.string(a)));
            idents.push(matches!(a.kind(), "identifier" | "field_access" | "method_reference").then(|| self.text(a).to_string()));
        }
        (strings, idents)
    }

    fn invocation(&self, node: Node) -> JCall {
        let receiver = match node.child_by_field_name("object") {
            None => Receiver::None,
            Some(o) => match o.kind() {
                "this" => Receiver::This,
                "super" => Receiver::Super,
                "identifier" => Receiver::Ident(self.text(o).to_string()),
                "field_access" => {
                    let field = o.child_by_field_name("field").map(|f| self.text(f).to_string()).unwrap_or_default();
                    match o.child_by_field_name("object").map(|x| x.kind()) {
                        Some("this") => Receiver::Field(field),
                        _ if self.text(o).chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '$')) => {
                            Receiver::Qualified(self.text(o).to_string())
                        }
                        _ => Receiver::Expr,
                    }
                }
                "scoped_identifier" => Receiver::Qualified(self.text(o).to_string()),
                "method_invocation" => Receiver::Chain,
                _ => Receiver::Expr,
            },
        };
        let (string_args, ident_args) = self.args(node.child_by_field_name("arguments"));
        JCall {
            receiver,
            method: node.child_by_field_name("name").map(|n| self.text(n).to_string()).unwrap_or_default(),
            string_args,
            ident_args,
            is_new: false,
            assigned_to: self.assigned_to(node),
            line: line(node),
        }
    }
}
