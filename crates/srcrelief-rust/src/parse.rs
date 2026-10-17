use srcrelief_facts::Annotation;
use srcrelief_format::SubKind;
use tree_sitter::{Node, Parser};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RsFile {
    pub bodies: Vec<Body>,
    pub lines: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Body {
    pub mods: Vec<RsMod>,
    pub uses: Vec<RsUse>,
    pub types: Vec<RsType>,
    pub impls: Vec<RsImpl>,
    pub fns: Vec<RsFn>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RsMod {
    pub name: String,
    pub attrs: Vec<Annotation>,
    pub inline: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RsUse {
    pub path: Vec<String>,
    pub alias: Option<String>,
    pub glob: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RsType {
    pub name: String,
    pub kind: SubKind,
    pub fields: Vec<(String, String)>,
    pub attrs: Vec<Annotation>,
    pub methods: Vec<RsFn>,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RsImpl {
    pub self_ty: String,
    pub trait_: Option<String>,
    pub attrs: Vec<Annotation>,
    pub generics: Vec<(String, String)>,
    pub fns: Vec<RsFn>,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RsFn {
    pub name: String,
    pub attrs: Vec<Annotation>,
    pub params: Vec<(String, String)>,
    pub has_self: bool,
    pub ret: Option<String>,
    pub generics: Vec<(String, String)>,
    pub locals: Vec<(String, Expr)>,
    pub calls: Vec<RsCall>,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Ty(String),
    Var(String, Vec<String>),
    Call(Box<Callee>),
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Callee {
    Path(Vec<String>),
    Method(Expr, String),
    Struct(Vec<String>),
    Opaque(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RsCall {
    pub callee: Callee,
    pub string_args: Vec<Option<String>>,
    pub ident_args: Vec<Option<String>>,
    pub assigned_to: Option<String>,
    pub line: u32,
}

const WRAPPERS: &[&str] = &["Box", "Rc", "Arc", "Option"];
const RESULTS: &[&str] = &["Box", "Rc", "Arc", "Option", "Result"];

pub fn parse_rust(src: &[u8]) -> RsFile {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into()).expect("rust grammar");
    let mut out = RsFile { bodies: vec![Body::default()], lines: src.iter().filter(|&&b| b == b'\n').count() as u32 + 1 };
    let Some(tree) = parser.parse(src, None) else { return out };
    let c = Ctx { src };
    c.items(tree.root_node(), 0, &mut out.bodies);
    out
}

pub fn has_cfg_test(attrs: &[Annotation]) -> bool {
    attrs.iter().any(|a| a.name == "cfg" && a.args.iter().any(|x| x == "test"))
}

pub fn is_test_attr(attrs: &[Annotation]) -> bool {
    attrs.iter().any(|a| matches!(a.name.as_str(), "test" | "bench" | "rstest" | "test_case") || a.name.ends_with("::test")) || has_cfg_test(attrs)
}

fn line(n: Node) -> u32 {
    n.start_position().row as u32 + 1
}

fn end_line(n: Node) -> u32 {
    n.end_position().row as u32 + 1
}

fn named_children(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.named_children(&mut c).collect()
}

fn split_top(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut depth, mut cur) = (0i32, String::new());
    for ch in s.chars() {
        match ch {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

struct Ctx<'s> {
    src: &'s [u8],
}

impl<'s> Ctx<'s> {
    fn text(&self, n: Node) -> &'s str {
        n.utf8_text(self.src).unwrap_or("")
    }

    fn attribute(&self, item: Node) -> Option<Annotation> {
        let attr = named_children(item).into_iter().find(|n| n.kind() == "attribute")?;
        let path = named_children(attr).into_iter().find(|n| matches!(n.kind(), "identifier" | "scoped_identifier" | "crate" | "self" | "super"))?;
        let name = self.text(path).split_whitespace().collect::<String>();
        let args = if let Some(tt) = attr.child_by_field_name("arguments") {
            let t = self.text(tt);
            split_top(&t[1..t.len().saturating_sub(1)])
        } else if let Some(v) = attr.child_by_field_name("value") {
            vec![self.text(v).trim_matches('"').to_string()]
        } else {
            Vec::new()
        };
        Some(Annotation { name, args })
    }

    fn items(&self, container: Node, body: usize, bodies: &mut Vec<Body>) {
        let mut attrs: Vec<Annotation> = Vec::new();
        for n in named_children(container) {
            match n.kind() {
                "attribute_item" => {
                    attrs.extend(self.attribute(n));
                    continue;
                }
                "inner_attribute_item" | "line_comment" | "block_comment" => continue,
                "mod_item" => {
                    let name = n.child_by_field_name("name").map(|x| self.text(x).to_string()).unwrap_or_default();
                    let inline = n.child_by_field_name("body").map(|list| {
                        bodies.push(Body::default());
                        let bi = bodies.len() - 1;
                        self.items(list, bi, bodies);
                        bi
                    });
                    bodies[body].mods.push(RsMod { name, attrs: std::mem::take(&mut attrs), inline });
                }
                "use_declaration" => {
                    if let Some(arg) = n.child_by_field_name("argument") {
                        self.use_tree(arg, &[], &mut bodies[body].uses);
                    }
                }
                "struct_item" | "union_item" | "enum_item" | "type_item" | "trait_item" => {
                    if let Some(t) = self.type_item(n, std::mem::take(&mut attrs)) {
                        bodies[body].types.push(t);
                    }
                }
                "impl_item" => {
                    let i = self.impl_item(n, std::mem::take(&mut attrs));
                    bodies[body].impls.push(i);
                }
                "function_item" => {
                    let f = self.func(n, std::mem::take(&mut attrs), &[]);
                    bodies[body].fns.push(f);
                }
                _ => {}
            }
            attrs.clear();
        }
    }

    fn path_segments(&self, n: Node) -> Option<Vec<String>> {
        match n.kind() {
            "identifier" | "type_identifier" | "crate" | "self" | "super" | "field_identifier" => Some(vec![self.text(n).to_string()]),
            "scoped_identifier" | "scoped_type_identifier" => {
                let mut segs = match n.child_by_field_name("path") {
                    Some(p) => self.path_segments(p)?,
                    None => Vec::new(),
                };
                segs.push(self.text(n.child_by_field_name("name")?).to_string());
                Some(segs)
            }
            "generic_type" | "generic_type_with_turbofish" => self.path_segments(n.child_by_field_name("type")?),
            "generic_function" => self.path_segments(n.child_by_field_name("function")?),
            _ => None,
        }
    }

    fn use_tree(&self, n: Node, prefix: &[String], out: &mut Vec<RsUse>) {
        let join = |extra: Vec<String>| -> Vec<String> { prefix.iter().cloned().chain(extra).collect() };
        match n.kind() {
            "use_as_clause" => {
                if let (Some(p), Some(a)) = (n.child_by_field_name("path").and_then(|p| self.path_segments(p)), n.child_by_field_name("alias")) {
                    out.push(RsUse { path: join(p), alias: Some(self.text(a).to_string()), glob: false });
                }
            }
            "use_wildcard" => {
                let base = named_children(n).first().and_then(|p| self.path_segments(*p)).unwrap_or_default();
                out.push(RsUse { path: join(base), alias: None, glob: true });
            }
            "scoped_use_list" => {
                let base = n.child_by_field_name("path").and_then(|p| self.path_segments(p)).unwrap_or_default();
                let full = join(base);
                if let Some(list) = n.child_by_field_name("list") {
                    for c in named_children(list) {
                        self.use_tree(c, &full, out);
                    }
                }
            }
            "use_list" => {
                for c in named_children(n) {
                    self.use_tree(c, prefix, out);
                }
            }
            _ => {
                if let Some(p) = self.path_segments(n) {
                    let path = join(p);
                    if path.last().is_some_and(|l| l == "self") {
                        let path: Vec<String> = path[..path.len() - 1].to_vec();
                        if !path.is_empty() {
                            out.push(RsUse { path, alias: None, glob: false });
                        }
                    } else {
                        out.push(RsUse { path, alias: None, glob: false });
                    }
                }
            }
        }
    }

    fn type_name(&self, n: Node, wrappers: &[&str]) -> String {
        match n.kind() {
            "reference_type" | "pointer_type" => n.child_by_field_name("type").map(|t| self.type_name(t, wrappers)).unwrap_or_default(),
            "dynamic_type" | "abstract_type" => n.child_by_field_name("trait").map(|t| self.type_name(t, wrappers)).unwrap_or_default(),
            "bounded_type" => named_children(n).first().map(|t| self.type_name(*t, wrappers)).unwrap_or_default(),
            "generic_type" => {
                let base = n.child_by_field_name("type").and_then(|t| self.path_segments(t)).unwrap_or_default();
                if base.last().is_some_and(|l| wrappers.contains(&l.as_str())) {
                    if let Some(arg) = n.child_by_field_name("type_arguments").and_then(|a| named_children(a).into_iter().find(|x| x.kind() != "lifetime")) {
                        return self.type_name(arg, wrappers);
                    }
                }
                base.join("::")
            }
            "type_identifier" | "scoped_type_identifier" | "primitive_type" => self.path_segments(n).map(|s| s.join("::")).unwrap_or_else(|| self.text(n).to_string()),
            _ => String::new(),
        }
    }

    fn generics(&self, n: Node) -> Vec<(String, String)> {
        let Some(tp) = n.child_by_field_name("type_parameters") else { return Vec::new() };
        let mut out = Vec::new();
        for p in named_children(tp) {
            if p.kind() != "type_parameter" {
                continue;
            }
            let Some(name) = p.child_by_field_name("name") else { continue };
            let bound = p
                .child_by_field_name("bounds")
                .and_then(|b| named_children(b).into_iter().find(|x| x.kind() != "lifetime"))
                .map(|b| self.type_name(b, WRAPPERS))
                .unwrap_or_default();
            out.push((self.text(name).to_string(), bound));
        }
        out
    }

    fn type_item(&self, n: Node, attrs: Vec<Annotation>) -> Option<RsType> {
        let name = self.text(n.child_by_field_name("name")?).to_string();
        let kind = match n.kind() {
            "enum_item" => SubKind::Enum,
            "type_item" => SubKind::NamedType,
            "trait_item" => SubKind::Interface,
            _ => SubKind::Struct,
        };
        let mut t = RsType { name, kind, fields: Vec::new(), attrs, methods: Vec::new(), start_line: line(n), end_line: end_line(n) };
        let body = n.child_by_field_name("body");
        match (n.kind(), body) {
            ("trait_item", Some(list)) => {
                let mut attrs = Vec::new();
                for item in named_children(list) {
                    match item.kind() {
                        "attribute_item" => attrs.extend(self.attribute(item)),
                        "function_item" | "function_signature_item" => t.methods.push(self.func(item, std::mem::take(&mut attrs), &[])),
                        _ => attrs.clear(),
                    }
                }
            }
            (_, Some(list)) if list.kind() == "field_declaration_list" => {
                for fd in named_children(list).into_iter().filter(|f| f.kind() == "field_declaration") {
                    if let (Some(fname), Some(fty)) = (fd.child_by_field_name("name"), fd.child_by_field_name("type")) {
                        t.fields.push((self.text(fname).to_string(), self.type_name(fty, WRAPPERS)));
                    }
                }
            }
            (_, Some(list)) if list.kind() == "ordered_field_declaration_list" => {
                let mut c = list.walk();
                for (i, fty) in list.children_by_field_name("type", &mut c).enumerate() {
                    t.fields.push((i.to_string(), self.type_name(fty, WRAPPERS)));
                }
            }
            _ => {}
        }
        Some(t)
    }

    fn impl_item(&self, n: Node, attrs: Vec<Annotation>) -> RsImpl {
        let self_ty = n.child_by_field_name("type").map(|t| self.type_name(t, &[])).unwrap_or_default();
        let trait_ = n.child_by_field_name("trait").map(|t| self.type_name(t, &[]));
        let generics = self.generics(n);
        let mut fns = Vec::new();
        if let Some(list) = n.child_by_field_name("body") {
            let mut attrs = Vec::new();
            for item in named_children(list) {
                match item.kind() {
                    "attribute_item" => attrs.extend(self.attribute(item)),
                    "function_item" => fns.push(self.func(item, std::mem::take(&mut attrs), &generics)),
                    _ => attrs.clear(),
                }
            }
        }
        RsImpl { self_ty, trait_, attrs, generics, fns, start_line: line(n), end_line: end_line(n) }
    }

    fn func(&self, n: Node, attrs: Vec<Annotation>, outer_generics: &[(String, String)]) -> RsFn {
        let mut generics = outer_generics.to_vec();
        generics.extend(self.generics(n));
        let mut f = RsFn {
            name: n.child_by_field_name("name").map(|x| self.text(x).to_string()).unwrap_or_default(),
            attrs,
            ret: n.child_by_field_name("return_type").map(|t| self.type_name(t, RESULTS)).filter(|s| !s.is_empty()),
            generics,
            start_line: line(n),
            end_line: end_line(n),
            ..Default::default()
        };
        if let Some(params) = n.child_by_field_name("parameters") {
            for p in named_children(params) {
                match p.kind() {
                    "self_parameter" => f.has_self = true,
                    "parameter" => {
                        let pat = p.child_by_field_name("pattern");
                        let ty = p.child_by_field_name("type").map(|t| self.type_name(t, WRAPPERS)).unwrap_or_default();
                        match pat.map(|x| (x.kind(), x)) {
                            Some(("self", _)) => f.has_self = true,
                            Some(("identifier", x)) => f.params.push((self.text(x).to_string(), ty)),
                            Some((_, x)) => {
                                let name = named_children(x).into_iter().find(|c| c.kind() == "identifier").map(|c| self.text(c).to_string()).unwrap_or_default();
                                f.params.push((name, ty));
                            }
                            None => f.params.push((String::new(), ty)),
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(body) = n.child_by_field_name("body") {
            self.body(body, &mut f);
        }
        f.calls.sort_by_key(|c| c.line);
        f
    }

    fn body(&self, body: Node, f: &mut RsFn) {
        let mut stack = vec![body];
        while let Some(n) = stack.pop() {
            match n.kind() {
                "function_item" => {
                    if let Some(name) = n.child_by_field_name("name") {
                        f.locals.push((self.text(name).to_string(), Expr::Unknown));
                    }
                    continue;
                }
                "macro_invocation" | "impl_item" | "trait_item" | "mod_item" => continue,
                "call_expression" => {
                    if let Some(call) = self.call(n) {
                        f.calls.push(call);
                    }
                }
                "struct_expression" => {
                    if let Some(path) = n.child_by_field_name("name").and_then(|t| self.path_segments(t)) {
                        f.calls.push(RsCall { callee: Callee::Struct(path), string_args: Vec::new(), ident_args: Vec::new(), assigned_to: None, line: line(n) });
                    }
                }
                "let_declaration" => {
                    if let Some(name) = n.child_by_field_name("pattern").and_then(|p| self.binding(p)) {
                        let ty = n.child_by_field_name("type").map(|t| Expr::Ty(self.type_name(t, WRAPPERS))).filter(|e| *e != Expr::Ty(String::new()));
                        let expr = ty.or_else(|| n.child_by_field_name("value").map(|v| self.expr(v))).unwrap_or(Expr::Unknown);
                        f.locals.push((name, expr));
                    }
                }
                _ => {}
            }
            let kids = named_children(n);
            stack.extend(kids.into_iter().rev());
        }
    }

    fn binding(&self, p: Node) -> Option<String> {
        match p.kind() {
            "identifier" => Some(self.text(p).to_string()),
            "mut_pattern" | "ref_pattern" => named_children(p).into_iter().find_map(|c| self.binding(c)),
            _ => None,
        }
    }

    fn expr(&self, n: Node) -> Expr {
        match n.kind() {
            "struct_expression" => n.child_by_field_name("name").and_then(|t| self.path_segments(t)).map_or(Expr::Unknown, |p| Expr::Ty(p.join("::"))),
            "call_expression" => self.call(n).map_or(Expr::Unknown, |c| Expr::Call(Box::new(c.callee))),
            "identifier" | "self" => Expr::Var(self.text(n).to_string(), Vec::new()),
            "field_expression" => {
                let (Some(value), Some(field)) = (n.child_by_field_name("value"), n.child_by_field_name("field")) else { return Expr::Unknown };
                match self.expr(value) {
                    Expr::Var(head, mut fields) => {
                        fields.push(self.text(field).to_string());
                        Expr::Var(head, fields)
                    }
                    _ => Expr::Unknown,
                }
            }
            "try_expression" | "await_expression" | "reference_expression" | "parenthesized_expression" | "unary_expression" => {
                named_children(n).into_iter().rev().find(|c| !matches!(c.kind(), "mutable_specifier")).map_or(Expr::Unknown, |c| self.expr(c))
            }
            _ => Expr::Unknown,
        }
    }

    fn call(&self, n: Node) -> Option<RsCall> {
        let func = n.child_by_field_name("function")?;
        let func = if func.kind() == "generic_function" { func.child_by_field_name("function")? } else { func };
        let callee = match func.kind() {
            "identifier" | "scoped_identifier" => match self.path_segments(func) {
                Some(p) => Callee::Path(p),
                None => Callee::Opaque(self.text(func).chars().take(40).collect()),
            },
            "field_expression" => {
                let method = self.text(func.child_by_field_name("field")?).to_string();
                let value = func.child_by_field_name("value")?;
                Callee::Method(self.expr(value), method)
            }
            _ => Callee::Opaque(self.text(func).chars().take(40).collect()),
        };
        let mut string_args = Vec::new();
        let mut ident_args = Vec::new();
        if let Some(args) = n.child_by_field_name("arguments") {
            for a in named_children(args) {
                string_args.push((a.kind() == "string_literal").then(|| self.text(a).trim_matches('"').to_string()));
                ident_args.push(matches!(a.kind(), "identifier" | "scoped_identifier").then(|| self.text(a).to_string()));
            }
        }
        Some(RsCall { callee, string_args, ident_args, assigned_to: self.assigned_to(n), line: line(n) })
    }

    fn assigned_to(&self, n: Node) -> Option<String> {
        let p = n.parent()?;
        match p.kind() {
            "let_declaration" => p.child_by_field_name("pattern").and_then(|x| self.binding(x)),
            "assignment_expression" => p.child_by_field_name("left").map(|l| self.text(l).to_string()),
            _ => None,
        }
    }
}
