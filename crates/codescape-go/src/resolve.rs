use crate::parse::{CHAIN, GoCall, GoFile, GoFunc, GoUse, parse_go};
use codescape_build::{Discovery, rel_path, walk_files_under};
use codescape_facts::{CallFacts, ExtractStats, FileFacts, FnFacts, Import, ProjectFacts, TypeFacts, TypeUse};
use codescape_format::*;
use globset::GlobSet;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

const MAX_IMPLEMENTATIONS: usize = 8;

const BUILTINS: &[&str] = &[
    "append", "cap", "clear", "close", "complex", "copy", "delete", "imag", "len", "make", "max", "min", "new", "panic", "print", "println", "real", "recover",
    "string", "int", "int64", "int32", "uint", "uint64", "byte", "rune", "float64", "float32", "bool", "error", "any",
];

const PREDECLARED_TYPES: &[&str] = &[
    "error",
    "string",
    "bool",
    "byte",
    "rune",
    "any",
    "comparable",
    "uintptr",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "float32",
    "float64",
    "complex64",
    "complex128",
];

struct ParsedFile {
    module: usize,
    rel: String,
    dir: String,
    is_test: bool,
    is_mock: bool,
    bytes: u64,
    gf: GoFile,
}

#[derive(Clone, Debug)]
enum TypeRef {
    Project(usize),
    External(String),
}

struct TypeInfo {
    node: NodeId,
    pkg: usize,
    name: String,
    kind: SubKind,
    fields: HashMap<String, String>,
    embeds: Vec<String>,
    iface_methods: Vec<String>,
    iface_arity: Vec<usize>,
    methods: HashMap<String, NodeId>,
    arity: HashMap<String, usize>,
    file: usize,
    excluded: bool,
}

fn declared_times(func: &GoFunc, name: &str) -> usize {
    func.declared.iter().filter(|d| *d == name).count()
}

fn covers(ty: &TypeInfo, iface: &TypeInfo) -> bool {
    iface
        .iface_methods
        .iter()
        .enumerate()
        .all(|(i, m)| ty.methods.contains_key(m) && iface.iface_arity.get(i).is_none_or(|a| ty.arity.get(m).is_none_or(|b| a == b)))
}

struct PkgInfo {
    node: NodeId,
    module: usize,
    import_path: String,
    name: String,
    types: HashMap<String, usize>,
    funcs: HashMap<String, (NodeId, Vec<String>, usize)>,
    ctor_owner: HashMap<String, usize>,
}

pub fn extract_go(d: &Discovery, module_nodes: &[NodeId], excludes: &GlobSet, b: &mut GraphBuilder, facts: &mut ProjectFacts) -> ExtractStats {
    let sources = list_sources(d, excludes);
    let parsed: Vec<ParsedFile> = sources
        .par_iter()
        .filter_map(|(module, path)| {
            let bytes = std::fs::read(path).ok()?;
            let gf = parse_go(&bytes);
            let m = &d.modules[*module];
            let dir = rel_path(&m.dir, path.parent().unwrap());
            let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
            let is_mock =
                gf.generated_mock || dir.split('/').any(|s| s == "mock" || s == "mocks") || file_name.starts_with("mock_") || file_name.ends_with("_mock.go");
            Some(ParsedFile {
                module: *module,
                rel: rel_path(&d.root, path),
                dir,
                is_test: file_name.ends_with("_test.go"),
                is_mock,
                bytes: bytes.len() as u64,
                gf,
            })
        })
        .collect();
    let first = b.node_count() as NodeId;
    let mut r = Resolver::new(d, module_nodes, b, &parsed);
    r.visible = r
        .declared_reach()
        .into_iter()
        .enumerate()
        .map(|(i, mut v)| {
            v.insert(i);
            v
        })
        .collect();
    r.declare();
    for n in first..r.b.node_count() as NodeId {
        if matches!(r.b.node_kind(n), NodeKind::Type | NodeKind::Member) {
            let exported = r.b.name(n).chars().next().is_some_and(char::is_uppercase);
            r.b.set_visibility(n, if exported { Visibility::Public } else { Visibility::Package });
        }
    }
    r.implements_edges();
    r.module_edges();
    r.package_edges();
    r.resolve(facts);
    r.stats
}

fn list_sources(d: &Discovery, excludes: &GlobSet) -> Vec<(usize, PathBuf)> {
    let go_dirs: Vec<(usize, &Path)> = d.modules.iter().enumerate().filter(|(_, m)| m.lang == Lang::Go).map(|(i, m)| (i, m.dir.as_path())).collect();
    let mut out = Vec::new();
    for &(mi, dir) in &go_dirs {
        let _ = walk_files_under(&d.root, dir, excludes, |p| {
            if p.extension().is_none_or(|e| e != "go") {
                return;
            }
            let owner = go_dirs.iter().filter(|(_, md)| p.starts_with(md)).max_by_key(|(_, md)| md.as_os_str().len()).map(|(i, _)| *i);
            if owner == Some(mi) {
                out.push((mi, p.to_path_buf()));
            }
        });
    }
    out.sort();
    out
}

fn default_import_name(path: &str) -> &str {
    let mut segs = path.rsplit('/');
    let last = segs.next().unwrap_or(path);
    if last.len() > 1 && last.starts_with('v') && last[1..].chars().all(|c| c.is_ascii_digit()) { segs.next().unwrap_or(last) } else { last }
}

struct Resolver<'a> {
    d: &'a Discovery,
    module_nodes: &'a [NodeId],
    b: &'a mut GraphBuilder,
    files: &'a [ParsedFile],
    pkgs: Vec<PkgInfo>,
    pkg_by_key: HashMap<(usize, String), usize>,
    pkg_by_path: HashMap<String, usize>,
    file_pkg: Vec<usize>,
    types: Vec<TypeInfo>,
    method_names: HashMap<String, Vec<(NodeId, usize)>>,
    visible: Vec<HashSet<usize>>,
    impl_cache: HashMap<(usize, usize), Option<usize>>,
    stats: ExtractStats,
}

impl<'a> Resolver<'a> {
    fn new(d: &'a Discovery, module_nodes: &'a [NodeId], b: &'a mut GraphBuilder, files: &'a [ParsedFile]) -> Self {
        Resolver {
            d,
            module_nodes,
            b,
            files,
            pkgs: Vec::new(),
            pkg_by_key: HashMap::new(),
            pkg_by_path: HashMap::new(),
            file_pkg: Vec::new(),
            types: Vec::new(),
            method_names: HashMap::new(),
            visible: Vec::new(),
            impl_cache: HashMap::new(),
            stats: ExtractStats::default(),
        }
    }

    fn declare(&mut self) {
        for f in self.files {
            self.stats.files += 1;
            self.stats.lines += f.gf.lines as u64;
            self.stats.bytes += f.bytes;
            let key = (f.module, f.dir.clone());
            let pi = match self.pkg_by_key.get(&key) {
                Some(&p) => p,
                None => {
                    let m = &self.d.modules[f.module];
                    let name = if f.dir.is_empty() { "(root)".to_string() } else { f.dir.clone() };
                    let node = self.b.add_node(self.module_nodes[f.module], NodeKind::Package, SubKind::None, &name, Lang::Go);
                    let base = m.go_module_path.clone().unwrap_or_else(|| m.name.clone());
                    let import_path = if f.dir.is_empty() { base } else { format!("{base}/{}", f.dir) };
                    let p = self.pkgs.len();
                    self.pkgs.push(PkgInfo {
                        node,
                        module: f.module,
                        import_path: import_path.clone(),
                        name: String::new(),
                        types: HashMap::new(),
                        funcs: HashMap::new(),
                        ctor_owner: HashMap::new(),
                    });
                    self.pkg_by_key.insert(key, p);
                    self.pkg_by_path.insert(import_path, p);
                    if f.is_mock {
                        self.b.add_flags(node, NodeFlags::MOCK);
                    }
                    p
                }
            };
            if !f.is_test || self.pkgs[pi].name.is_empty() {
                self.pkgs[pi].name = f.gf.package.trim_end_matches("_test").to_string();
            }
            self.file_pkg.push(pi);
        }
        for (fi, f) in self.files.iter().enumerate() {
            let pi = self.file_pkg[fi];
            for t in &f.gf.types {
                let node = self.b.add_node(self.pkgs[pi].node, NodeKind::Type, t.kind, &t.name, Lang::Go);
                self.b.set_span(node, &f.rel, t.start_line, t.end_line);
                self.b.set_lines(node, t.end_line - t.start_line + 1);
                self.flag(node, f);
                let mut methods = HashMap::new();
                for m in &t.methods {
                    let id = self.b.add_node(node, NodeKind::Member, SubKind::Method, m, Lang::Go);
                    self.flag(id, f);
                    methods.insert(m.clone(), id);
                }
                for fd in &t.field_decls {
                    let id = self.b.add_node(node, NodeKind::Member, SubKind::Field, &fd.name, Lang::Go);
                    self.b.set_detail(id, &fd.ty);
                    self.b.set_span(id, &f.rel, fd.line, fd.line);
                    self.flag(id, f);
                }
                let ti = self.types.len();
                self.types.push(TypeInfo {
                    node,
                    pkg: pi,
                    name: t.name.clone(),
                    kind: t.kind,
                    fields: t.fields.iter().cloned().collect(),
                    embeds: t.embeds.clone(),
                    iface_methods: t.methods.clone(),
                    iface_arity: t.arity.clone(),
                    methods,
                    arity: HashMap::new(),
                    file: fi,
                    excluded: f.is_test || f.is_mock,
                });
                self.pkgs[pi].types.insert(t.name.clone(), ti);
            }
        }
        for (fi, f) in self.files.iter().enumerate() {
            let pi = self.file_pkg[fi];
            for func in &f.gf.funcs {
                let owner = func.receiver.as_ref().and_then(|r| self.pkgs[pi].types.get(r).copied());
                let ctor = if func.receiver.is_none() { self.constructor_owner(pi, func) } else { None };
                let parent = owner.or(ctor).map_or(self.pkgs[pi].node, |t| self.types[t].node);
                let sub = match (owner, ctor) {
                    (Some(_), _) => SubKind::Method,
                    (None, Some(t)) => {
                        self.pkgs[pi].ctor_owner.insert(func.name.clone(), t);
                        SubKind::Constructor
                    }
                    (None, None) => SubKind::Function,
                };
                let id = match owner.and_then(|t| self.types[t].methods.get(&func.name).copied()) {
                    Some(id) => id,
                    None => self.b.add_node(parent, NodeKind::Member, sub, &func.name, Lang::Go),
                };
                self.b.set_span(id, &f.rel, func.start_line, func.end_line);
                self.b.set_complexity(id, func.complexity);
                self.b.set_lines(id, func.end_line - func.start_line + 1);
                self.flag(id, f);
                match owner {
                    Some(t) => {
                        self.types[t].methods.insert(func.name.clone(), id);
                        self.types[t].arity.insert(func.name.clone(), func.params.len());
                    }
                    None => {
                        self.pkgs[pi].funcs.entry(func.name.clone()).or_insert((id, func.results.clone(), fi));
                    }
                }
                if !f.is_test && !f.is_mock {
                    self.method_names.entry(func.name.clone()).or_default().push((id, f.module));
                }
            }
        }
    }

    fn constructor_owner(&self, pi: usize, func: &GoFunc) -> Option<usize> {
        let raw = func.results.first()?.trim_start_matches('*');
        if raw.contains('.') {
            return None;
        }
        let &t = self.pkgs[pi].types.get(raw)?;
        if self.types[t].kind != SubKind::Interface {
            return Some(t);
        }
        let mut built: Vec<usize> = func
            .calls
            .iter()
            .filter_map(|c| c.composite.as_deref())
            .filter(|c| !c.contains('.'))
            .filter_map(|c| self.pkgs[pi].types.get(c.trim_start_matches('*')).copied())
            .filter(|&c| self.types[c].kind != SubKind::Interface)
            .collect();
        built.sort();
        built.dedup();
        (built.len() == 1).then(|| built[0])
    }

    fn implements_edges(&mut self) {
        let mut by_method: HashMap<&str, Vec<usize>> = HashMap::new();
        for (ti, t) in self.types.iter().enumerate() {
            if t.kind == SubKind::Struct && !t.excluded {
                for m in t.methods.keys() {
                    by_method.entry(m.as_str()).or_default().push(ti);
                }
            }
        }
        let mut edges: Vec<(NodeId, NodeId)> = Vec::new();
        for (ii, iface) in self.types.iter().enumerate() {
            if iface.kind != SubKind::Interface || iface.excluded || iface.iface_methods.is_empty() {
                continue;
            }
            let iface_module = self.pkgs[iface.pkg].module;
            let Some(seed) = iface.iface_methods.iter().map(|m| by_method.get(m.as_str()).map_or(&[][..], |v| v.as_slice())).min_by_key(|v| v.len()) else {
                continue;
            };
            let impls: Vec<usize> = seed
                .iter()
                .copied()
                .filter(|&s| s != ii)
                .filter(|&s| self.visible[self.pkgs[self.types[s].pkg].module].contains(&iface_module))
                .filter(|&s| covers(&self.types[s], iface))
                .collect();
            if impls.is_empty() || impls.len() > MAX_IMPLEMENTATIONS {
                continue;
            }
            edges.extend(impls.iter().map(|&s| (self.types[s].node, iface.node)));
        }
        for (from, to) in edges {
            self.b.add_edge(from, to, EdgeKind::Implements, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
        }
    }

    fn flag(&mut self, node: NodeId, f: &ParsedFile) {
        if f.is_test {
            self.b.add_flags(node, NodeFlags::TEST);
        }
        if f.is_mock {
            self.b.add_flags(node, NodeFlags::MOCK);
        }
    }

    fn module_edges(&mut self) {
        for l in &self.d.links {
            if self.d.modules[l.from].lang == Lang::Go && self.d.modules[l.to].lang == Lang::Go {
                self.b.add_edge(self.module_nodes[l.from], self.module_nodes[l.to], EdgeKind::DependsOn, l.scope, Confidence::Resolved, EdgeFlags::NONE);
            }
        }
    }

    fn declared_reach(&self) -> Vec<HashSet<usize>> {
        (0..self.d.modules.len())
            .map(|start| {
                let mut set = HashSet::new();
                let mut q = VecDeque::from([start]);
                while let Some(m) = q.pop_front() {
                    for l in self.d.links.iter().filter(|l| l.from == m) {
                        if set.insert(l.to) {
                            q.push_back(l.to);
                        }
                    }
                }
                set
            })
            .collect()
    }

    fn package_edges(&mut self) {
        let reach = self.declared_reach();
        for (fi, f) in self.files.iter().enumerate() {
            let from = self.file_pkg[fi];
            for imp in &f.gf.imports {
                let Some(&to) = self.pkg_by_path.get(&imp.path) else { continue };
                let (fm, tm) = (self.pkgs[from].module, self.pkgs[to].module);
                if fm == tm {
                    continue;
                }
                let flags = if reach[fm].contains(&tm) { EdgeFlags::NONE } else { EdgeFlags::UNDECLARED };
                let scope = if f.is_test { Scope::Test } else { Scope::Main };
                self.b.add_edge(self.pkgs[from].node, self.pkgs[to].node, EdgeKind::DependsOn, scope, Confidence::Resolved, flags);
            }
        }
    }

    fn import_for(&self, fi: usize, qualifier: &str) -> Option<&'a Import> {
        let imports = &self.files[fi].gf.imports;
        let declared = |imp: &Import| match &imp.alias {
            Some(a) => a == qualifier,
            None => match self.pkg_by_path.get(&imp.path) {
                Some(&p) => self.pkgs[p].name == qualifier,
                None => default_import_name(&imp.path) == qualifier,
            },
        };
        imports.iter().find(|i| declared(i)).or_else(|| {
            imports.iter().find(|i| i.alias.is_none() && !self.pkg_by_path.contains_key(&i.path) && default_import_name(&i.path).starts_with(qualifier))
        })
    }

    fn resolve_type(&self, fi: usize, raw: &str) -> Option<TypeRef> {
        let raw = raw.trim_start_matches('*').trim_start_matches("[]");
        if raw.is_empty() {
            return None;
        }
        match raw.split_once('.') {
            Some((q, name)) => {
                let imp = self.import_for(fi, q)?;
                match self.pkg_by_path.get(&imp.path) {
                    Some(&p) => self.pkgs[p].types.get(name).map(|&t| TypeRef::Project(t)),
                    None => Some(TypeRef::External(raw.to_string())),
                }
            }
            None => match self.pkgs[self.file_pkg[fi]].types.get(raw) {
                Some(&t) => Some(TypeRef::Project(t)),
                None if PREDECLARED_TYPES.contains(&raw) => Some(TypeRef::External(raw.to_string())),
                None => None,
            },
        }
    }

    fn type_name(&self, r: &TypeRef) -> String {
        match r {
            TypeRef::Project(t) => format!("{}.{}", self.pkgs[self.types[*t].pkg].import_path, self.types[*t].name),
            TypeRef::External(s) => s.clone(),
        }
    }

    fn func_result(&self, fi: usize, callee: &str, func: &GoFunc) -> Option<TypeRef> {
        let pi = match callee.split_once('.') {
            Some((q, _)) if self.var_type_raw(func, q).is_some() => return None,
            Some((q, _)) => *self.pkg_by_path.get(&self.import_for(fi, q)?.path)?,
            None => self.file_pkg[fi],
        };
        let name = callee.rsplit('.').next().unwrap_or(callee);
        let (_, results, def_file) = self.pkgs[pi].funcs.get(name)?;
        self.resolve_type(*def_file, results.first()?)
    }

    fn var_type_raw<'f>(&self, func: &'f GoFunc, name: &str) -> Option<&'f str> {
        func.locals.iter().rev().find(|(n, _)| n == name).or_else(|| func.params.iter().find(|(n, _)| n == name)).map(|(_, t)| t.as_str())
    }

    fn var_type(&self, fi: usize, func: &GoFunc, name: &str) -> Option<TypeRef> {
        let raw = self.var_type_raw(func, name)?;
        match raw.strip_prefix("()") {
            Some(callee) => self.func_result(fi, callee, func),
            None => self.resolve_type(fi, raw),
        }
    }

    fn field_type(&self, t: usize, field: &str) -> Option<TypeRef> {
        let mut seen = HashSet::new();
        let mut q = VecDeque::from([t]);
        while let Some(cur) = q.pop_front() {
            if !seen.insert(cur) {
                continue;
            }
            let info = &self.types[cur];
            let fi = info.file;
            if let Some(ty) = info.fields.get(field) {
                return self.resolve_type(fi, ty);
            }
            for e in &info.embeds {
                if let Some(TypeRef::Project(et)) = self.resolve_type(fi, e) {
                    q.push_back(et);
                }
            }
        }
        None
    }

    fn lookup_method(&self, t: usize, name: &str) -> Option<(usize, NodeId)> {
        let mut seen = HashSet::new();
        let mut q = VecDeque::from([t]);
        while let Some(cur) = q.pop_front() {
            if !seen.insert(cur) {
                continue;
            }
            if let Some(&m) = self.types[cur].methods.get(name) {
                return Some((cur, m));
            }
            let fi = self.types[cur].file;
            for e in &self.types[cur].embeds {
                if let Some(TypeRef::Project(et)) = self.resolve_type(fi, e) {
                    q.push_back(et);
                }
            }
        }
        None
    }

    fn single_impl(&mut self, iface: usize, from_module: usize) -> Option<usize> {
        if let Some(&c) = self.impl_cache.get(&(iface, from_module)) {
            return c;
        }
        let wanted = &self.types[iface].iface_methods;
        let found: Vec<usize> = if wanted.is_empty() {
            Vec::new()
        } else {
            (0..self.types.len())
                .filter(|&t| {
                    let ty = &self.types[t];
                    ty.kind == SubKind::Struct
                        && !ty.excluded
                        && self.visible[from_module].contains(&self.pkgs[ty.pkg].module)
                        && covers(ty, &self.types[iface])
                })
                .collect()
        };
        let res = (found.len() == 1).then(|| found[0]);
        self.impl_cache.insert((iface, from_module), res);
        res
    }

    fn resolve(&mut self, facts: &mut ProjectFacts) {
        for fi in 0..self.files.len() {
            let f = &self.files[fi];
            let pi = self.file_pkg[fi];
            let own = self.pkgs[pi].node;
            let mut import_targets: Vec<NodeId> = Vec::new();
            for imp in &f.gf.imports {
                if let Some(&p) = self.pkg_by_path.get(&imp.path) {
                    let n = self.pkgs[p].node;
                    if n != own && !import_targets.contains(&n) {
                        import_targets.push(n);
                    }
                }
            }
            let mut ff = FileFacts {
                rel_path: f.rel.clone(),
                module: self.module_nodes[f.module],
                package: Some(own),
                import_targets,
                lang: Lang::Go,
                is_test: f.is_test,
                is_dev: false,
                imports: f.gf.imports.clone(),
                types: Vec::new(),
                functions: Vec::new(),
                calls: Vec::new(),
                type_uses: Vec::new(),
            };
            for t in &f.gf.types {
                let ti = self.pkgs[pi].types[&t.name];
                ff.types.push(TypeFacts {
                    node: self.types[ti].node,
                    fqn: format!("{}.{}", self.pkgs[pi].import_path, t.name),
                    supertypes: t.embeds.clone(),
                    annotations: Vec::new(),
                });
                self.record_uses(fi, self.types[ti].node, &t.type_uses, &mut ff.type_uses);
            }
            for func in &f.gf.funcs {
                let owner = func.receiver.as_ref().and_then(|r| self.pkgs[pi].types.get(r).copied());
                let node = match owner {
                    Some(t) => self.types[t].methods[&func.name],
                    None => self.pkgs[pi].funcs[&func.name].0,
                };
                let owner = owner.or_else(|| if func.receiver.is_none() { self.pkgs[pi].ctor_owner.get(&func.name).copied() } else { None });
                ff.functions.push(FnFacts {
                    node,
                    owner: owner.map(|t| self.types[t].node),
                    name: func.name.clone(),
                    params: func.params.clone(),
                    annotations: Vec::new(),
                    is_main: func.name == "main" && func.receiver.is_none() && f.gf.package == "main",
                    overrides_external: None,
                });
                self.record_uses(fi, node, &func.type_uses, &mut ff.type_uses);
                let scope = if f.is_test { Scope::Test } else { Scope::Main };
                for call in &func.calls {
                    let cf = self.resolve_call(fi, func, node, scope, call);
                    ff.calls.push(cf);
                }
            }
            facts.files.push(ff);
        }
    }

    fn record_uses(&mut self, fi: usize, from: NodeId, uses: &[GoUse], out: &mut Vec<TypeUse>) {
        for u in uses {
            out.push(TypeUse { owner: from, name: u.name.clone(), line: u.line, col: u.col, len: u.len });
            if let Some(TypeRef::Project(t)) = self.resolve_type(fi, &u.name) {
                self.b.add_ref(from, self.types[t].node, u.line, u.col, u.len);
            }
        }
    }

    fn resolve_call(&mut self, fi: usize, func: &GoFunc, caller: NodeId, scope: Scope, call: &GoCall) -> CallFacts {
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
        cf.arg_targets = call.ident_args.iter().map(|a| a.as_deref().and_then(|a| self.func_value(fi, func, a))).collect();
        if let Some(comp) = &call.composite {
            if let Some(r) = self.resolve_type(fi, comp) {
                cf.instantiates = Some(self.type_name(&r));
                if let TypeRef::Project(t) = r {
                    let to = self.types[t].node;
                    self.b.add_edge_at(caller, to, EdgeKind::Instantiates, scope, Confidence::Resolved, EdgeFlags::NONE, call.line);
                }
            }
            return cf;
        }
        let Some(recv) = call.receiver.as_deref() else {
            let pi = self.file_pkg[fi];
            if let Some(&(target, _, _)) = self.pkgs[pi].funcs.get(&call.method) {
                self.b.add_edge_at(caller, target, EdgeKind::Calls, scope, Confidence::Resolved, EdgeFlags::NONE, call.line);
                if declared_times(func, &call.method) == 0 {
                    self.b.add_ref(caller, target, call.line, call.col, call.len);
                }
                cf.target = Some(target);
                self.stats.resolved += 1;
            } else if let Some(&t) = self.pkgs[pi].types.get(&call.method) {
                if declared_times(func, &call.method) == 0 {
                    self.b.add_ref(caller, self.types[t].node, call.line, call.col, call.len);
                }
            } else if BUILTINS.contains(&call.method.as_str()) {
            } else {
                self.stats.unresolved += 1;
            }
            return cf;
        };
        if recv == CHAIN {
            match &call.chain_head {
                Some((head_recv, head)) if self.returns_library_value(fi, func, head_recv.as_deref(), head) => self.stats.external += 1,
                _ => self.guess(fi, caller, scope, &call.method, &mut cf),
            }
            return cf;
        }
        let mut segs = recv.split('.');
        let head = segs.next().unwrap();
        cf.receiver_var = Some(head.to_string());
        let mut current = self.var_type(fi, func, head);
        let rest: Vec<&str> = segs.collect();
        if current.is_none()
            && let Some(imp) = self.import_for(fi, head)
        {
            match self.pkg_by_path.get(&imp.path).copied() {
                Some(p) if rest.is_empty() => {
                    if let Some(&(target, _, _)) = self.pkgs[p].funcs.get(&call.method) {
                        self.b.add_edge_at(caller, target, EdgeKind::Calls, scope, Confidence::Resolved, EdgeFlags::NONE, call.line);
                        if declared_times(func, head) == 0 {
                            self.b.add_ref(caller, target, call.line, call.col, call.len);
                        }
                        cf.target = Some(target);
                        self.stats.resolved += 1;
                    } else if let Some(&t) = self.pkgs[p].types.get(&call.method) {
                        if declared_times(func, head) == 0 {
                            self.b.add_ref(caller, self.types[t].node, call.line, call.col, call.len);
                        }
                    } else {
                        self.stats.unresolved += 1;
                    }
                    return cf;
                }
                None => {
                    self.stats.external += 1;
                    return cf;
                }
                _ => {}
            }
        }
        for seg in &rest {
            current = match current {
                Some(TypeRef::Project(t)) => self.field_type(t, seg),
                ext => ext,
            };
        }
        match current {
            Some(TypeRef::Project(t)) => {
                cf.receiver_type = Some(self.type_name(&TypeRef::Project(t)));
                match self.lookup_method(t, &call.method) {
                    Some((owner, target)) => {
                        self.b.add_edge_at(caller, target, EdgeKind::Calls, scope, Confidence::Resolved, EdgeFlags::NONE, call.line);
                        if declared_times(func, head) <= 1 {
                            self.b.add_ref(caller, target, call.line, call.col, call.len);
                        }
                        cf.target = Some(target);
                        if self.types[owner].kind == SubKind::Interface
                            && let Some(imp) = self.single_impl(owner, self.files[fi].module)
                            && let Some(&m) = self.types[imp].methods.get(&call.method)
                        {
                            self.b.add_edge_at(caller, m, EdgeKind::Calls, scope, Confidence::Resolved, EdgeFlags::NONE, call.line);
                        }
                        self.stats.resolved += 1;
                    }
                    None => self.guess(fi, caller, scope, &call.method, &mut cf),
                }
            }
            Some(TypeRef::External(name)) => {
                cf.receiver_type = Some(name);
                self.stats.external += 1;
            }
            None => self.guess(fi, caller, scope, &call.method, &mut cf),
        }
        cf
    }

    fn returns_library_value(&self, fi: usize, func: &GoFunc, recv: Option<&str>, name: &str) -> bool {
        let Some(path) = recv else { return matches!(self.func_result(fi, name, func), Some(TypeRef::External(_))) };
        let mut segs = path.split('.');
        let head = segs.next().unwrap_or_default();
        let rest: Vec<&str> = segs.collect();
        let mut current = self.var_type(fi, func, head);
        if current.is_none() {
            let Some(imp) = self.import_for(fi, head) else { return false };
            return match self.pkg_by_path.get(&imp.path) {
                None => true,
                Some(_) if rest.is_empty() => matches!(self.func_result(fi, &format!("{path}.{name}"), func), Some(TypeRef::External(_))),
                Some(_) => false,
            };
        }
        for seg in &rest {
            current = match current {
                Some(TypeRef::Project(t)) => self.field_type(t, seg),
                ext => ext,
            };
        }
        matches!(current, Some(TypeRef::External(_)))
    }

    fn guess(&mut self, fi: usize, caller: NodeId, scope: Scope, name: &str, cf: &mut CallFacts) {
        let module = self.files[fi].module;
        let candidates: Vec<NodeId> =
            self.method_names.get(name).map(|v| v.iter().filter(|(_, m)| self.visible[module].contains(m)).map(|(n, _)| *n).collect()).unwrap_or_default();
        match candidates.as_slice() {
            [target] => {
                let target = *target;
                self.b.add_edge_at(caller, target, EdgeKind::Calls, scope, Confidence::Guessed, EdgeFlags::NONE, cf.line);
                cf.target = Some(target);
                self.stats.guessed += 1;
            }
            _ => self.stats.unresolved += 1,
        }
    }

    fn func_value(&self, fi: usize, func: &GoFunc, text: &str) -> Option<NodeId> {
        match text.split_once('.') {
            None => self.pkgs[self.file_pkg[fi]].funcs.get(text).map(|(n, _, _)| *n),
            Some((head, name)) => {
                if let Some(TypeRef::Project(t)) = self.var_type(fi, func, head) {
                    return self.lookup_method(t, name).map(|(_, m)| m);
                }
                let imp = self.import_for(fi, head)?;
                let p = *self.pkg_by_path.get(&imp.path)?;
                self.pkgs[p].funcs.get(name).map(|(n, _, _)| *n)
            }
        }
    }
}
