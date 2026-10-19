use crate::parse::{JCall, JMethod, JType, JUse, JavaFile, Receiver, parse_java};
use codescape_build::{Discovery, rel_path, walk_files_under};
use codescape_facts::{CallFacts, ExtractStats, FileFacts, FnFacts, ProjectFacts, TypeFacts, TypeUse};
use codescape_format::*;
use globset::GlobSet;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

pub const EXTERNAL_MODULE: &str = "(external)";

const JAVA_LANG: &[&str] = &[
    "Object",
    "String",
    "Enum",
    "Record",
    "Number",
    "Integer",
    "Long",
    "Double",
    "Float",
    "Short",
    "Byte",
    "Boolean",
    "Character",
    "Void",
    "Class",
    "Thread",
    "Runnable",
    "Comparable",
    "Iterable",
    "AutoCloseable",
    "Cloneable",
    "CharSequence",
    "StringBuilder",
    "Throwable",
    "Exception",
    "RuntimeException",
    "Error",
    "IllegalStateException",
    "IllegalArgumentException",
    "UnsupportedOperationException",
    "NullPointerException",
    "IndexOutOfBoundsException",
    "ArrayIndexOutOfBoundsException",
    "ArithmeticException",
    "ClassCastException",
    "NumberFormatException",
    "SecurityException",
    "InterruptedException",
    "CloneNotSupportedException",
    "ReflectiveOperationException",
    "ClassNotFoundException",
    "AssertionError",
    "StackOverflowError",
    "OutOfMemoryError",
];

const JDK_SUPERTYPES: &[(&str, &str)] = &[
    ("java.lang.Exception", "java.lang.Throwable"),
    ("java.lang.Error", "java.lang.Throwable"),
    ("java.lang.RuntimeException", "java.lang.Exception"),
    ("java.lang.IllegalStateException", "java.lang.RuntimeException"),
    ("java.lang.IllegalArgumentException", "java.lang.RuntimeException"),
    ("java.lang.NumberFormatException", "java.lang.IllegalArgumentException"),
    ("java.lang.UnsupportedOperationException", "java.lang.RuntimeException"),
    ("java.lang.NullPointerException", "java.lang.RuntimeException"),
    ("java.lang.IndexOutOfBoundsException", "java.lang.RuntimeException"),
    ("java.lang.ArrayIndexOutOfBoundsException", "java.lang.IndexOutOfBoundsException"),
    ("java.lang.ArithmeticException", "java.lang.RuntimeException"),
    ("java.lang.ClassCastException", "java.lang.RuntimeException"),
    ("java.lang.SecurityException", "java.lang.RuntimeException"),
    ("java.lang.InterruptedException", "java.lang.Exception"),
    ("java.lang.CloneNotSupportedException", "java.lang.Exception"),
    ("java.lang.ReflectiveOperationException", "java.lang.Exception"),
    ("java.lang.ClassNotFoundException", "java.lang.ReflectiveOperationException"),
    ("java.lang.AssertionError", "java.lang.Error"),
    ("java.lang.StackOverflowError", "java.lang.Error"),
    ("java.lang.OutOfMemoryError", "java.lang.Error"),
    ("java.io.IOException", "java.lang.Exception"),
    ("java.io.UncheckedIOException", "java.lang.RuntimeException"),
    ("java.io.FileNotFoundException", "java.io.IOException"),
    ("java.util.NoSuchElementException", "java.lang.RuntimeException"),
    ("java.util.ConcurrentModificationException", "java.lang.RuntimeException"),
];

struct SourceFile {
    module: usize,
    path: PathBuf,
    is_test: bool,
    is_dev: bool,
}

struct ParsedFile {
    module: usize,
    rel: String,
    is_test: bool,
    is_dev: bool,
    bytes: u64,
    jf: JavaFile,
}

#[derive(Clone, Debug)]
enum TypeRef {
    Project(usize),
    External(String),
}

struct TypeInfo {
    node: NodeId,
    fqn: String,
    kind: SubKind,
    file: usize,
    outer: Option<usize>,
    raw_supers: Vec<(String, bool)>,
    supers: Vec<TypeRef>,
    fields: HashMap<String, String>,
    methods: HashMap<String, NodeId>,
    decl: (usize, Vec<usize>),
    is_test: bool,
}

pub fn extract_java(d: &Discovery, module_nodes: &[NodeId], excludes: &GlobSet, b: &mut GraphBuilder, facts: &mut ProjectFacts) -> ExtractStats {
    let sources = list_sources(d, excludes);
    let parsed: Vec<ParsedFile> = sources
        .par_iter()
        .filter_map(|s| {
            let bytes = std::fs::read(&s.path).ok()?;
            Some(ParsedFile {
                module: s.module,
                rel: rel_path(&d.root, &s.path),
                is_test: s.is_test,
                is_dev: s.is_dev,
                bytes: bytes.len() as u64,
                jf: parse_java(&bytes),
            })
        })
        .collect();
    let mut r = Resolver::new(d, module_nodes, b, &parsed);
    let with_self = |v: Vec<HashSet<usize>>| -> Vec<HashSet<usize>> {
        v.into_iter()
            .enumerate()
            .map(|(i, mut s)| {
                s.insert(i);
                s
            })
            .collect()
    };
    r.visible = with_self(r.declared_reach_scoped(true));
    r.visible_test = with_self(r.declared_reach_scoped(false));
    r.declare();
    r.link_supertypes();
    r.link_known_supertypes();
    r.module_edges();
    r.package_edges();
    r.resolve_calls(facts);
    r.stats
}

fn list_sources(d: &Discovery, excludes: &GlobSet) -> Vec<SourceFile> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (mi, m) in d.modules.iter().enumerate() {
        if m.lang != Lang::Java {
            continue;
        }
        let roots = m
            .test_roots
            .iter()
            .map(|r| (r, true, false))
            .chain(m.dev_roots.iter().map(|r| (r, false, true)))
            .chain(m.source_roots.iter().map(|r| (r, false, false)));
        for (root, is_test, is_dev) in roots {
            let _ = walk_files_under(&d.root, root, excludes, |p| {
                if p.extension().is_some_and(|e| e == "java") && seen.insert(p.to_path_buf()) {
                    out.push(SourceFile { module: mi, path: p.to_path_buf(), is_test, is_dev });
                }
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

struct Resolver<'a> {
    d: &'a Discovery,
    module_nodes: &'a [NodeId],
    b: &'a mut GraphBuilder,
    files: &'a [ParsedFile],
    types: Vec<TypeInfo>,
    by_fqn: HashMap<String, Vec<usize>>,
    visible: Vec<HashSet<usize>>,
    visible_test: Vec<HashSet<usize>>,
    file_types: Vec<Vec<usize>>,
    packages: HashMap<(usize, String), NodeId>,
    package_module: HashMap<String, usize>,
    externals: HashMap<String, NodeId>,
    external_module: Option<NodeId>,
    subtypes: HashMap<usize, Vec<usize>>,
    method_names: HashMap<String, Vec<(NodeId, usize)>>,
    stats: ExtractStats,
}

impl<'a> Resolver<'a> {
    fn new(d: &'a Discovery, module_nodes: &'a [NodeId], b: &'a mut GraphBuilder, files: &'a [ParsedFile]) -> Self {
        Resolver {
            d,
            module_nodes,
            b,
            files,
            types: Vec::new(),
            by_fqn: HashMap::new(),
            visible: Vec::new(),
            visible_test: Vec::new(),
            file_types: vec![Vec::new(); files.len()],
            packages: HashMap::new(),
            package_module: HashMap::new(),
            externals: HashMap::new(),
            external_module: None,
            subtypes: HashMap::new(),
            method_names: HashMap::new(),
            stats: ExtractStats::default(),
        }
    }

    fn declare(&mut self) {
        for (fi, f) in self.files.iter().enumerate() {
            self.stats.files += 1;
            self.stats.lines += f.jf.lines as u64;
            self.stats.bytes += f.bytes;
            let pkg_name = if f.jf.package.is_empty() { "(default)".to_string() } else { f.jf.package.clone() };
            let module_node = self.module_nodes[f.module];
            let pkg = *self
                .packages
                .entry((f.module, pkg_name.clone()))
                .or_insert_with(|| self.b.add_node(module_node, NodeKind::Package, SubKind::None, &pkg_name, Lang::Java));
            self.package_module.entry(f.jf.package.clone()).or_insert(f.module);
            for (ti, t) in f.jf.types.iter().enumerate() {
                self.declare_type(fi, pkg, &f.jf.package, None, t, (ti, Vec::new()));
            }
        }
    }

    fn declare_type(&mut self, fi: usize, parent: NodeId, prefix: &str, outer: Option<usize>, t: &JType, decl: (usize, Vec<usize>)) {
        let f = &self.files[fi];
        let fqn = if prefix.is_empty() { t.name.clone() } else { format!("{prefix}.{}", t.name) };
        let node = self.b.add_node(parent, NodeKind::Type, t.kind, &t.name, Lang::Java);
        self.b.set_span(node, &f.rel, t.start_line, t.end_line);
        self.b.set_lines(node, t.end_line - t.start_line + 1);
        self.b.set_visibility(node, t.visibility);
        let public_members = matches!(t.kind, SubKind::Interface | SubKind::Annotation);
        if f.is_test {
            self.b.add_flags(node, NodeFlags::TEST);
        }
        let mut methods = HashMap::new();
        for m in &t.methods {
            let id = *methods.entry(m.name.clone()).or_insert_with(|| {
                let id = self.b.add_node(node, NodeKind::Member, SubKind::Method, &m.name, Lang::Java);
                self.b.set_span(id, &f.rel, m.start_line, m.end_line);
                self.b.set_lines(id, m.end_line - m.start_line + 1);
                self.b.set_visibility(id, if public_members && m.visibility == Visibility::Package { Visibility::Public } else { m.visibility });
                self.b.set_complexity(id, m.complexity);
                if f.is_test {
                    self.b.add_flags(id, NodeFlags::TEST);
                }
                id
            });
            if !f.is_test && m.name != "<init>" && m.name != "<clinit>" {
                self.method_names.entry(m.name.clone()).or_default().push((id, f.module));
            }
        }
        for fd in &t.field_decls {
            let id = self.b.add_node(node, NodeKind::Member, SubKind::Field, &fd.name, Lang::Java);
            self.b.set_detail(id, &fd.ty);
            self.b.set_visibility(id, if public_members && fd.visibility == Visibility::Package { Visibility::Public } else { fd.visibility });
            self.b.set_span(id, &f.rel, fd.line, fd.line);
            if f.is_test {
                self.b.add_flags(id, NodeFlags::TEST);
            }
        }
        let idx = self.types.len();
        let raw_supers = t.extends.iter().map(|s| (s.clone(), false)).chain(t.implements.iter().map(|s| (s.clone(), true))).collect();
        self.types.push(TypeInfo {
            node,
            fqn: fqn.clone(),
            kind: t.kind,
            file: fi,
            outer,
            raw_supers,
            supers: Vec::new(),
            fields: t.fields.iter().cloned().collect(),
            methods,
            decl: decl.clone(),
            is_test: f.is_test,
        });
        self.by_fqn.entry(fqn.clone()).or_default().push(idx);
        self.file_types[fi].push(idx);
        for (ni, inner) in t.nested.iter().enumerate() {
            let mut path = decl.1.clone();
            path.push(ni);
            self.declare_type(fi, node, &fqn, Some(idx), inner, (decl.0, path));
        }
    }

    fn fqn_visible(&self, name: &str, fi: usize) -> Option<usize> {
        let visible = self.visible_from(fi);
        self.by_fqn.get(name)?.iter().copied().find(|t| visible.contains(&self.files[self.types[*t].file].module))
    }

    fn fqn(&self, name: &str, fi: usize) -> Option<usize> {
        let candidates = self.by_fqn.get(name)?;
        let module = self.files[fi].module;
        let module_of = |t: &usize| self.files[self.types[*t].file].module;
        candidates
            .iter()
            .find(|t| module_of(t) == module)
            .or_else(|| candidates.iter().find(|t| self.visible_from(fi).contains(&module_of(t))))
            .or(candidates.first())
            .copied()
    }

    fn jtype(&self, ti: usize) -> &'a JType {
        let t = &self.types[ti];
        let mut cur = &self.files[t.file].jf.types[t.decl.0];
        for &i in &t.decl.1 {
            cur = &cur.nested[i];
        }
        cur
    }

    fn resolve_type(&self, raw: &str, fi: usize, enclosing: Option<usize>) -> Option<TypeRef> {
        if raw.is_empty() || raw == "var" || raw.chars().next().is_some_and(|c| c.is_lowercase()) && !raw.contains('.') {
            return None;
        }
        let f = &self.files[fi];
        if raw.contains('.') {
            if let Some(i) = self.fqn(raw, fi) {
                return Some(TypeRef::Project(i));
            }
            let (head, rest) = raw.split_once('.').unwrap();
            if head.chars().next().is_some_and(|c| c.is_uppercase()) {
                if let Some(TypeRef::Project(h)) = self.resolve_type(head, fi, enclosing) {
                    let full = format!("{}.{rest}", self.types[h].fqn);
                    if let Some(i) = self.fqn(&full, fi) {
                        return Some(TypeRef::Project(i));
                    }
                }
                return None;
            }
            return Some(TypeRef::External(raw.to_string()));
        }
        let mut e = enclosing;
        while let Some(ti) = e {
            let candidate = format!("{}.{raw}", self.types[ti].fqn);
            if let Some(i) = self.fqn_visible(&candidate, fi) {
                return Some(TypeRef::Project(i));
            }
            e = self.types[ti].outer;
        }
        for imp in f.jf.imports.iter().filter(|i| !i.wildcard && !i.is_static) {
            if imp.path.rsplit('.').next() == Some(raw) {
                return Some(match self.fqn(&imp.path, fi) {
                    Some(i) => TypeRef::Project(i),
                    None => TypeRef::External(imp.path.clone()),
                });
            }
        }
        let same_pkg = if f.jf.package.is_empty() { raw.to_string() } else { format!("{}.{raw}", f.jf.package) };
        if let Some(i) = self.fqn_visible(&same_pkg, fi) {
            return Some(TypeRef::Project(i));
        }
        let mut external_wildcards = Vec::new();
        for imp in f.jf.imports.iter().filter(|i| i.wildcard && !i.is_static) {
            let candidate = format!("{}.{raw}", imp.path);
            if let Some(i) = self.fqn_visible(&candidate, fi) {
                return Some(TypeRef::Project(i));
            }
            if !self.package_module.contains_key(&imp.path) {
                external_wildcards.push(candidate);
            }
        }
        if external_wildcards.len() == 1 {
            return Some(TypeRef::External(external_wildcards.remove(0)));
        }
        JAVA_LANG.contains(&raw).then(|| TypeRef::External(format!("java.lang.{raw}")))
    }

    fn link_known_supertypes(&mut self) {
        let mut todo: Vec<String> = self.externals.keys().cloned().collect();
        let mut linked = HashSet::new();
        while let Some(fqn) = todo.pop() {
            let Some(&(_, parent)) = JDK_SUPERTYPES.iter().find(|(sub, _)| *sub == fqn) else { continue };
            if !linked.insert(fqn.clone()) {
                continue;
            }
            let (from, to) = (self.external_node(&fqn), self.external_node(parent));
            self.b.add_edge(from, to, EdgeKind::Inherits, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
            todo.push(parent.to_string());
        }
    }

    fn external_node(&mut self, fqn: &str) -> NodeId {
        if let Some(&n) = self.externals.get(fqn) {
            return n;
        }
        let module = *self.external_module.get_or_insert_with(|| {
            let root = self.b.root();
            let m = self.b.child(root, EXTERNAL_MODULE).unwrap_or_else(|| self.b.add_node(root, NodeKind::Module, SubKind::None, EXTERNAL_MODULE, Lang::None));
            self.b.add_flags(m, NodeFlags::EXTERNAL);
            m
        });
        let n = self.b.child(module, fqn).unwrap_or_else(|| self.b.add_node(module, NodeKind::External, SubKind::None, fqn, Lang::Java));
        self.b.add_flags(n, NodeFlags::EXTERNAL);
        self.externals.insert(fqn.to_string(), n);
        n
    }

    fn ref_node(&mut self, r: &TypeRef) -> NodeId {
        match r {
            TypeRef::Project(i) => self.types[*i].node,
            TypeRef::External(f) => self.external_node(f),
        }
    }

    fn ref_name(&self, r: &TypeRef) -> String {
        match r {
            TypeRef::Project(i) => self.types[*i].fqn.clone(),
            TypeRef::External(f) => f.clone(),
        }
    }

    fn link_supertypes(&mut self) {
        for ti in 0..self.types.len() {
            let raws = self.types[ti].raw_supers.clone();
            let fi = self.types[ti].file;
            let scope = if self.types[ti].is_test { Scope::Test } else { Scope::Main };
            let mut supers = Vec::new();
            for (raw, implements) in raws {
                let resolved = self.resolve_type(&raw, fi, self.types[ti].outer.or(Some(ti)));
                let Some(r) = resolved else { continue };
                let to = self.ref_node(&r);
                let kind = if implements || self.types[ti].kind == SubKind::Interface { EdgeKind::Implements } else { EdgeKind::Inherits };
                let kind = match &r {
                    TypeRef::Project(s) if self.types[*s].kind == SubKind::Interface => EdgeKind::Implements,
                    _ => kind,
                };
                self.b.add_edge(self.types[ti].node, to, kind, scope, Confidence::Resolved, EdgeFlags::NONE);
                if let TypeRef::Project(s) = r {
                    self.subtypes.entry(s).or_default().push(ti);
                }
                supers.push(r);
            }
            self.types[ti].supers = supers;
        }
    }

    fn visible_from(&self, fi: usize) -> &HashSet<usize> {
        let f = &self.files[fi];
        if f.is_test { &self.visible_test[f.module] } else { &self.visible[f.module] }
    }

    fn declared_reach_scoped(&self, main_only: bool) -> Vec<HashSet<usize>> {
        let n = self.d.modules.len();
        let mut out = vec![HashSet::new(); n];
        for (start, set) in out.iter_mut().enumerate() {
            let mut q = VecDeque::from([start]);
            let mut first = true;
            while let Some(m) = q.pop_front() {
                for l in self.d.links.iter().filter(|l| l.from == m && (!main_only && first || l.scope == Scope::Main)) {
                    if set.insert(l.to) {
                        q.push_back(l.to);
                    }
                }
                first = false;
            }
        }
        out
    }

    fn declared_reach(&self) -> Vec<HashSet<usize>> {
        let n = self.d.modules.len();
        let mut out = vec![HashSet::new(); n];
        for (start, set) in out.iter_mut().enumerate() {
            let mut q = VecDeque::from([start]);
            while let Some(m) = q.pop_front() {
                for l in self.d.links.iter().filter(|l| l.from == m) {
                    if set.insert(l.to) {
                        q.push_back(l.to);
                    }
                }
            }
        }
        out
    }

    fn module_edges(&mut self) {
        for l in &self.d.links {
            let (a, b) = (&self.d.modules[l.from], &self.d.modules[l.to]);
            if a.lang == Lang::Java && b.lang == Lang::Java {
                self.b.add_edge(self.module_nodes[l.from], self.module_nodes[l.to], EdgeKind::DependsOn, l.scope, Confidence::Resolved, EdgeFlags::NONE);
            }
        }
    }

    fn package_edges(&mut self) {
        let reach = self.declared_reach();
        for f in self.files {
            let pkg_name = if f.jf.package.is_empty() { "(default)".to_string() } else { f.jf.package.clone() };
            let from = self.packages[&(f.module, pkg_name)];
            let scope = if f.is_test { Scope::Test } else { Scope::Main };
            let mut targets: Vec<(usize, String)> = Vec::new();
            for imp in f.jf.imports.iter().filter(|i| !i.is_static) {
                if imp.wildcard {
                    if let Some(&m) = self.package_module.get(&imp.path) {
                        targets.push((m, imp.path.clone()));
                    }
                } else if let Some(&ti) =
                    self.by_fqn.get(&imp.path).and_then(|v| v.iter().find(|&&t| self.files[self.types[t].file].module == f.module).or(v.first()))
                {
                    let tf = &self.files[self.types[ti].file];
                    targets.push((tf.module, tf.jf.package.clone()));
                }
            }
            for (m, pkg) in targets {
                if m == f.module {
                    continue;
                }
                let Some(&to) = self.packages.get(&(m, pkg)) else { continue };
                let flags = if reach[f.module].contains(&m) { EdgeFlags::NONE } else { EdgeFlags::UNDECLARED };
                self.b.add_edge(from, to, EdgeKind::DependsOn, scope, Confidence::Resolved, flags);
            }
        }
    }

    fn lookup_method(&self, ti: usize, name: &str) -> Option<(usize, NodeId)> {
        let mut seen = HashSet::new();
        let mut q = VecDeque::from([ti]);
        while let Some(t) = q.pop_front() {
            if !seen.insert(t) {
                continue;
            }
            if let Some(&m) = self.types[t].methods.get(name) {
                return Some((t, m));
            }
            for s in &self.types[t].supers {
                if let TypeRef::Project(s) = s {
                    q.push_back(*s);
                }
            }
        }
        None
    }

    fn external_super(&self, ti: usize) -> Option<String> {
        let mut seen = HashSet::new();
        let mut q = VecDeque::from([ti]);
        while let Some(t) = q.pop_front() {
            if !seen.insert(t) {
                continue;
            }
            for s in &self.types[t].supers {
                match s {
                    TypeRef::External(f) => return Some(f.clone()),
                    TypeRef::Project(p) => q.push_back(*p),
                }
            }
        }
        None
    }

    fn declared_in_project_super(&self, ti: usize, name: &str) -> bool {
        self.types[ti].supers.iter().any(|s| matches!(s, TypeRef::Project(p) if self.lookup_method(*p, name).is_some()))
    }

    fn single_impl(&self, ti: usize, name: &str, fi: usize) -> Option<NodeId> {
        let visible = self.visible_from(fi);
        let abstract_like = self.types[ti].kind == SubKind::Interface;
        if !abstract_like {
            return None;
        }
        let mut impls = Vec::new();
        let mut seen = HashSet::new();
        let mut q = VecDeque::from([ti]);
        while let Some(t) = q.pop_front() {
            for &s in self.subtypes.get(&t).into_iter().flatten() {
                if !seen.insert(s) || self.types[s].is_test {
                    continue;
                }
                if self.types[s].kind != SubKind::Interface && visible.contains(&self.files[self.types[s].file].module) {
                    impls.push(s);
                }
                q.push_back(s);
            }
        }
        if impls.len() != 1 {
            return None;
        }
        self.lookup_method(impls[0], name).map(|(_, m)| m)
    }

    fn resolve_calls(&mut self, facts: &mut ProjectFacts) {
        for fi in 0..self.files.len() {
            let f = &self.files[fi];
            let pkg_name = if f.jf.package.is_empty() { "(default)".to_string() } else { f.jf.package.clone() };
            let package = self.packages.get(&(f.module, pkg_name)).copied();
            let mut import_targets: Vec<NodeId> = Vec::new();
            for imp in f.jf.imports.iter().filter(|i| !i.is_static) {
                let target = if imp.wildcard {
                    self.package_module.get(&imp.path).and_then(|&m| self.packages.get(&(m, imp.path.clone()))).copied()
                } else {
                    self.fqn(&imp.path, fi).map(|t| {
                        let tf = &self.files[self.types[t].file];
                        let name = if tf.jf.package.is_empty() { "(default)".to_string() } else { tf.jf.package.clone() };
                        self.packages[&(tf.module, name)]
                    })
                };
                if let Some(t) = target
                    && Some(t) != package
                    && !import_targets.contains(&t)
                {
                    import_targets.push(t);
                }
            }
            let mut ff = FileFacts {
                rel_path: f.rel.clone(),
                module: self.module_nodes[f.module],
                package,
                import_targets,
                lang: Lang::Java,
                is_test: f.is_test,
                is_dev: f.is_dev,
                imports: f.jf.imports.clone(),
                types: Vec::new(),
                functions: Vec::new(),
                calls: Vec::new(),
                type_uses: Vec::new(),
            };
            for ti in self.file_types[fi].clone() {
                let jt = self.jtype(ti);
                ff.types.push(TypeFacts {
                    node: self.types[ti].node,
                    fqn: self.types[ti].fqn.clone(),
                    supertypes: self.types[ti].supers.iter().map(|s| self.ref_name(s)).collect(),
                    annotations: jt.annotations.clone(),
                });
                let own = self.types[ti].node;
                let enclosing = self.types[ti].outer.or(Some(ti));
                self.record_uses(fi, own, enclosing, &jt.header_uses, &mut ff.type_uses);
                self.record_uses(fi, own, Some(ti), &jt.type_uses, &mut ff.type_uses);
                let external = self.external_super(ti);
                for m in &jt.methods {
                    let node = self.types[ti].methods[&m.name];
                    let overrides_external = (m.annotations.iter().any(|a| a.name == "Override") && !self.declared_in_project_super(ti, &m.name))
                        .then(|| external.clone())
                        .flatten();
                    ff.functions.push(FnFacts {
                        node,
                        owner: Some(self.types[ti].node),
                        name: m.name.clone(),
                        params: m.params.clone(),
                        annotations: m.annotations.clone(),
                        is_main: is_main(m),
                        overrides_external,
                    });
                    self.record_uses(fi, node, Some(ti), &m.type_uses, &mut ff.type_uses);
                    for call in &m.calls {
                        let cf = self.resolve_call(fi, ti, node, m, call);
                        ff.calls.push(cf);
                    }
                }
            }
            facts.files.push(ff);
        }
    }

    fn record_uses(&mut self, fi: usize, from: NodeId, enclosing: Option<usize>, uses: &[JUse], out: &mut Vec<TypeUse>) {
        for u in uses {
            out.push(TypeUse { owner: from, name: u.name.clone(), line: u.line, col: u.col, len: u.len });
            if let Some(TypeRef::Project(t)) = self.resolve_type(&u.name, fi, enclosing) {
                self.b.add_ref(from, self.types[t].node, u.line, u.col, u.len);
            }
        }
    }

    fn var_type(&self, fi: usize, ti: usize, m: &JMethod, name: &str) -> Option<TypeRef> {
        if let Some((_, ty)) = m.locals.iter().rev().find(|(n, _)| n == name) {
            return self.resolve_type(ty, fi, Some(ti));
        }
        if let Some((_, ty)) = m.params.iter().find(|(n, _)| n == name) {
            return self.resolve_type(ty, fi, Some(ti));
        }
        self.field_type(ti, name)
    }

    fn field_type(&self, ti: usize, name: &str) -> Option<TypeRef> {
        let mut seen = HashSet::new();
        let mut q = VecDeque::from([ti]);
        while let Some(t) = q.pop_front() {
            if !seen.insert(t) {
                continue;
            }
            if let Some(ty) = self.types[t].fields.get(name) {
                return self.resolve_type(ty, self.types[t].file, Some(t));
            }
            for s in &self.types[t].supers {
                if let TypeRef::Project(p) = s {
                    q.push_back(*p);
                }
            }
            if let Some(o) = self.types[t].outer {
                q.push_back(o);
            }
        }
        None
    }

    fn resolve_call(&mut self, fi: usize, ti: usize, caller: NodeId, m: &JMethod, call: &JCall) -> CallFacts {
        let scope = if self.types[ti].is_test { Scope::Test } else { Scope::Main };
        let mut cf = CallFacts {
            caller,
            method: call.method.clone(),
            string_args: call.string_args.clone(),
            ident_args: call.ident_args.clone(),
            assigned_to: call.assigned_to.clone(),
            line: call.line,
            col: call.col,
            len: call.len,
            ..Default::default()
        };
        cf.arg_targets = call.ident_args.iter().map(|a| a.as_deref().and_then(|a| self.method_ref(fi, ti, a))).collect();
        if call.is_new {
            if let Some(r) = self.resolve_type(&call.method, fi, Some(ti)) {
                let to = self.ref_node(&r);
                self.b.add_edge_at(caller, to, EdgeKind::Instantiates, scope, Confidence::Resolved, EdgeFlags::NONE, call.line);
                cf.instantiates = Some(self.ref_name(&r));
                cf.receiver_type = cf.instantiates.clone();
            }
            return cf;
        }
        let receiver_type: Option<TypeRef> = match &call.receiver {
            Receiver::None => {
                let own = self.lookup_in_enclosing(ti, &call.method);
                match own {
                    Some(t) => Some(TypeRef::Project(t)),
                    None => self.static_import(fi, &call.method),
                }
            }
            Receiver::This => Some(TypeRef::Project(ti)),
            Receiver::Super => self.types[ti].supers.first().cloned(),
            Receiver::Ident(name) => {
                cf.receiver_var = Some(name.clone());
                match self.var_type(fi, ti, m, name) {
                    Some(t) => Some(t),
                    None => {
                        let t = self.resolve_type(name, fi, Some(ti));
                        if let (Some(TypeRef::Project(p)), Some(u)) = (&t, &call.receiver_use) {
                            self.b.add_ref(caller, self.types[*p].node, u.line, u.col, u.len);
                        }
                        t
                    }
                }
            }
            Receiver::Field(name) => {
                cf.receiver_var = Some(name.clone());
                self.field_type(ti, name)
            }
            Receiver::Qualified(text) => self.resolve_type(text, fi, Some(ti)),
            Receiver::Chain | Receiver::Expr => None,
        };
        if let Some(r) = &receiver_type {
            cf.receiver_type = Some(self.ref_name(r));
        }
        match receiver_type {
            Some(TypeRef::Project(t)) => {
                if let Some((owner, target)) = self.lookup_method(t, &call.method) {
                    self.b.add_edge_at(caller, target, EdgeKind::Calls, scope, Confidence::Resolved, EdgeFlags::NONE, call.line);
                    self.b.add_ref(caller, target, call.line, call.col, call.len);
                    cf.target = Some(target);
                    if let Some(imp) = self.single_impl(owner, &call.method, fi) {
                        self.b.add_edge_at(caller, imp, EdgeKind::Calls, scope, Confidence::Resolved, EdgeFlags::NONE, call.line);
                    }
                    self.stats.resolved += 1;
                } else if self.external_super(t).is_some() {
                    self.stats.resolved += 1;
                } else {
                    self.stats.unresolved += 1;
                }
            }
            Some(TypeRef::External(_)) => self.stats.resolved += 1,
            None => self.guess(fi, caller, scope, &call.method, &mut cf),
        }
        cf
    }

    fn guess(&mut self, fi: usize, caller: NodeId, scope: Scope, name: &str, cf: &mut CallFacts) {
        let visible = self.visible_from(fi);
        let candidates: Vec<NodeId> =
            self.method_names.get(name).map(|v| v.iter().filter(|(_, m)| visible.contains(m)).map(|(n, _)| *n).collect()).unwrap_or_default();
        match candidates.as_slice() {
            [target] => {
                let target = *target;
                self.b.add_edge_at(caller, target, EdgeKind::Calls, scope, Confidence::Guessed, EdgeFlags::NONE, cf.line);
                self.b.add_ref(caller, target, cf.line, cf.col, cf.len);
                cf.target = Some(target);
                self.stats.guessed += 1;
            }
            _ => self.stats.unresolved += 1,
        }
    }

    fn lookup_in_enclosing(&self, ti: usize, name: &str) -> Option<usize> {
        let mut t = Some(ti);
        while let Some(cur) = t {
            if let Some((owner, _)) = self.lookup_method(cur, name) {
                return Some(owner);
            }
            t = self.types[cur].outer;
        }
        None
    }

    fn static_import(&self, fi: usize, name: &str) -> Option<TypeRef> {
        for imp in self.files[fi].jf.imports.iter().filter(|i| i.is_static) {
            let (owner, member) = if imp.wildcard { (imp.path.as_str(), name) } else { imp.path.rsplit_once('.')? };
            if member != name {
                continue;
            }
            return Some(match self.fqn(owner, fi) {
                Some(i) => TypeRef::Project(i),
                None => TypeRef::External(owner.to_string()),
            });
        }
        None
    }

    fn method_ref(&self, fi: usize, ti: usize, text: &str) -> Option<NodeId> {
        let (lhs, name) = text.split_once("::")?;
        let t = match lhs {
            "this" => Some(TypeRef::Project(ti)),
            _ => self.resolve_type(lhs, fi, Some(ti)).or_else(|| self.field_type(ti, lhs)),
        };
        match t {
            Some(TypeRef::Project(t)) => self.lookup_method(t, name).map(|(_, m)| m),
            _ => None,
        }
    }
}

fn is_main(m: &JMethod) -> bool {
    m.name == "main" && m.is_static && m.is_public && m.params.len() == 1 && m.params[0].1 == "String"
}
