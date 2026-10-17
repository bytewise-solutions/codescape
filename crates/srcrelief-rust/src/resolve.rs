use crate::parse::{Callee, Expr, RsCall, RsFile, RsFn, has_cfg_test, is_test_attr, parse_rust};
use globset::GlobSet;
use rayon::prelude::*;
use srcrelief_build::{Discovery, cargo_crate, rel_path, walk_files_under};
use srcrelief_facts::{CallFacts, ExtractStats, FileFacts, FnFacts, Import, ProjectFacts, TypeFacts};
use srcrelief_format::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

pub const EXTERNAL_MODULE: &str = "(external)";
const LIB_ROOT: &str = "(lib)";
const BUILD_ROOT: &str = "(build)";
const MAX_DEPTH: u32 = 12;

const STD_CRATES: &[&str] = &["std", "core", "alloc"];

const PRELUDE: &[&str] = &[
    "String", "Vec", "Box", "Option", "Result", "Some", "None", "Ok", "Err", "ToString", "ToOwned", "Default", "Iterator", "IntoIterator",
    "From", "Into", "TryFrom", "TryInto", "Clone", "Copy", "Send", "Sync", "Sized", "Unpin", "Drop", "Fn", "FnMut", "FnOnce", "PartialEq",
    "Eq", "PartialOrd", "Ord", "AsRef", "AsMut", "Extend", "DoubleEndedIterator", "ExactSizeIterator", "FromIterator", "u8", "u16", "u32",
    "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64", "bool", "char", "str",
];

const SELF_RETURNING: &[&str] = &["default", "from", "try_from", "from_str"];

const BUILTIN_CALLS: &[&str] = &["Some", "Ok", "Err", "drop"];

const PASS_THROUGH: &[&str] = &["unwrap", "expect", "unwrap_or_default", "clone", "to_owned", "as_ref", "as_mut", "borrow", "borrow_mut"];

const STD_METHODS: &[&str] = &[
    "clone", "clone_from", "to_string", "to_owned", "fmt", "eq", "ne", "cmp", "partial_cmp", "lt", "le", "gt", "ge", "hash", "into", "from",
    "try_into", "try_from", "default", "as_ref", "as_mut", "borrow", "borrow_mut", "deref", "deref_mut", "drop", "iter", "iter_mut",
    "into_iter", "next", "map", "filter", "filter_map", "flat_map", "collect", "unwrap", "expect", "unwrap_or", "unwrap_or_else",
    "unwrap_or_default", "ok", "err", "ok_or", "ok_or_else", "and_then", "or_else", "is_some", "is_none", "is_ok", "is_err", "len",
    "is_empty", "push", "pop", "insert", "remove", "get", "get_mut", "contains", "contains_key", "extend", "clear", "entry", "or_insert",
    "or_insert_with", "or_default", "keys", "values", "values_mut", "first", "last", "sort", "sort_by", "sort_by_key", "dedup", "retain",
    "join", "split", "trim", "starts_with", "ends_with", "chars", "bytes", "lines", "parse", "as_str", "as_slice", "to_vec", "copied",
    "cloned", "enumerate", "zip", "rev", "skip", "take", "chain", "any", "all", "find", "position", "count", "sum", "min", "max",
    "min_by_key", "max_by_key", "fold", "for_each", "lock", "read", "write", "send", "recv", "write_all", "flush", "with_capacity",
    "replace", "set", "as_deref", "map_err", "context", "with_context", "new", "display", "parent", "file_name", "file_stem",
    "extension", "exists", "is_file", "is_dir", "to_path_buf", "to_str", "to_string_lossy", "canonicalize", "strip_prefix", "with_extension",
    "open", "add", "sub", "mul", "div", "neg", "union", "intersects", "abs", "round", "floor", "ceil", "sqrt", "powi", "clamp",
];

struct ParsedFile {
    module: usize,
    path: PathBuf,
    rel: String,
    is_test: bool,
    is_dev: bool,
    bytes: u64,
    rf: RsFile,
}

#[derive(Clone, Debug, PartialEq)]
enum Item {
    Pkg(usize),
    Type(usize),
    Fn(usize),
    Variant(usize),
    Assoc(usize),
    Ext(String),
}

#[derive(Clone, Debug)]
enum TypeRef {
    Project(usize),
    External(String),
    Opaque,
}

struct Pkg {
    node: NodeId,
    module: usize,
    root: usize,
    parent: Option<usize>,
    segs: Vec<String>,
    file: usize,
    body: usize,
    test: bool,
    main_root: bool,
    children: HashMap<String, usize>,
    types: HashMap<String, usize>,
    fns: HashMap<String, usize>,
}

struct TypeInfo {
    node: NodeId,
    pkg: usize,
    kind: SubKind,
    fqn: String,
    fields: HashMap<String, String>,
    methods: HashMap<String, usize>,
    traits: Vec<usize>,
    implementors: Vec<usize>,
    supers: Vec<String>,
}

struct FnInfo {
    node: NodeId,
    pkg: usize,
    owner: Option<usize>,
    ret: Option<String>,
    generics: Vec<(String, String)>,
}

#[derive(Clone, Copy)]
enum FnRef {
    Free(usize, usize),
    TraitMethod(usize, usize, usize),
    ImplMethod(usize, usize, usize),
}

struct FnOcc {
    pkg: usize,
    at: FnRef,
    info: usize,
    overrides_external: Option<String>,
}

#[derive(Default)]
struct CrateInfo {
    lib_name: String,
    lib_root: Option<usize>,
    externs: HashMap<String, usize>,
    ext_names: HashSet<String>,
}

pub fn extract_rust(d: &Discovery, module_nodes: &[NodeId], excludes: &GlobSet, b: &mut GraphBuilder, facts: &mut ProjectFacts) -> ExtractStats {
    let sources = list_sources(d, excludes);
    if sources.is_empty() {
        return ExtractStats::default();
    }
    let parsed: Vec<ParsedFile> = sources
        .par_iter()
        .filter_map(|(module, path, is_test, is_dev)| {
            let bytes = std::fs::read(path).ok()?;
            Some(ParsedFile { module: *module, path: path.clone(), rel: rel_path(&d.root, path), is_test: *is_test, is_dev: *is_dev, bytes: bytes.len() as u64, rf: parse_rust(&bytes) })
        })
        .collect();
    let mut r = Resolver::new(d, module_nodes, b, &parsed);
    r.build_packages();
    r.declare();
    r.declare_impl_only_types();
    r.attach_impls();
    r.module_edges();
    r.package_edges();
    r.resolve(facts);
    r.stats
}

fn list_sources(d: &Discovery, excludes: &GlobSet) -> Vec<(usize, PathBuf, bool, bool)> {
    let rust: Vec<(usize, &Path)> = d.modules.iter().enumerate().filter(|(_, m)| m.lang == Lang::Rust).map(|(i, m)| (i, m.dir.as_path())).collect();
    let owner = |p: &Path| rust.iter().filter(|(_, md)| p.starts_with(md)).max_by_key(|(_, md)| md.as_os_str().len()).map(|(i, _)| *i);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for &(mi, _) in &rust {
        let m = &d.modules[mi];
        let roots = m.test_roots.iter().map(|r| (r, true, false)).chain(m.dev_roots.iter().map(|r| (r, false, true))).chain(m.source_roots.iter().map(|r| (r, false, false)));
        for (root, is_test, is_dev) in roots {
            let _ = walk_files_under(&d.root, root, excludes, |p| {
                if p.extension().is_some_and(|e| e == "rs") && owner(p) == Some(mi) && seen.insert(p.to_path_buf()) {
                    out.push((mi, p.to_path_buf(), is_test, is_dev));
                }
            });
        }
        let cc = cargo_crate(d, mi);
        let extra = cc.lib.iter().map(|p| (p, false)).chain(cc.bins.iter().map(|(_, p)| (p, false))).chain(cc.build_script.iter().map(|p| (p, true)));
        for (p, is_dev) in extra {
            if p.is_file() && seen.insert(p.clone()) {
                out.push((mi, p.clone(), false, is_dev));
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

fn is_std(path: &str) -> bool {
    STD_CRATES.iter().any(|c| path.starts_with(&format!("{c}."))) || (!path.contains('.') && PRELUDE.contains(&path))
}

fn segments(s: &str) -> Vec<String> {
    s.split("::").map(str::to_string).collect()
}

struct Resolver<'a> {
    d: &'a Discovery,
    module_nodes: &'a [NodeId],
    b: &'a mut GraphBuilder,
    files: &'a [ParsedFile],
    file_ix: HashMap<PathBuf, usize>,
    file_pkg: Vec<Option<usize>>,
    crates: HashMap<usize, CrateInfo>,
    pkgs: Vec<Pkg>,
    types: Vec<TypeInfo>,
    fns: Vec<FnInfo>,
    occs: Vec<Vec<FnOcc>>,
    type_occs: Vec<Vec<(usize, usize, usize)>>,
    method_names: HashMap<String, Vec<(NodeId, usize)>>,
    visible: Vec<HashSet<usize>>,
    externals: HashMap<String, NodeId>,
    impl_only: HashMap<(usize, String), usize>,
    memo: RefCell<HashMap<(usize, String), Option<Item>>>,
    stats: ExtractStats,
}

struct FnCtx<'f> {
    pkg: usize,
    self_ty: Option<usize>,
    f: &'f RsFn,
    caller: NodeId,
    scope: Scope,
    module: usize,
    imported: &'f HashSet<usize>,
}

impl<'a> Resolver<'a> {
    fn new(d: &'a Discovery, module_nodes: &'a [NodeId], b: &'a mut GraphBuilder, files: &'a [ParsedFile]) -> Self {
        let visible = (0..d.modules.len())
            .map(|start| {
                let mut set = HashSet::from([start]);
                let mut q = VecDeque::from([start]);
                while let Some(m) = q.pop_front() {
                    for l in d.links.iter().filter(|l| l.from == m) {
                        if set.insert(l.to) {
                            q.push_back(l.to);
                        }
                    }
                }
                set
            })
            .collect();
        Resolver {
            d,
            module_nodes,
            b,
            files,
            file_ix: files.iter().enumerate().map(|(i, f)| (f.path.clone(), i)).collect(),
            file_pkg: vec![None; files.len()],
            crates: HashMap::new(),
            pkgs: Vec::new(),
            types: Vec::new(),
            fns: Vec::new(),
            occs: (0..files.len()).map(|_| Vec::new()).collect(),
            type_occs: (0..files.len()).map(|_| Vec::new()).collect(),
            method_names: HashMap::new(),
            visible,
            externals: HashMap::new(),
            impl_only: HashMap::new(),
            memo: RefCell::new(HashMap::new()),
            stats: ExtractStats::default(),
        }
    }

    fn new_pkg(&mut self, module: usize, display: &str, root: Option<usize>, parent: Option<usize>, segs: Vec<String>, file: usize, body: usize, test: bool) -> usize {
        let mn = self.module_nodes[module];
        let node = self.b.child(mn, display).unwrap_or_else(|| self.b.add_node(mn, NodeKind::Package, SubKind::None, display, Lang::Rust));
        if test {
            self.b.add_flags(node, NodeFlags::TEST);
        }
        let pi = self.pkgs.len();
        self.pkgs.push(Pkg {
            node,
            module,
            root: root.unwrap_or(pi),
            parent,
            segs,
            file,
            body,
            test,
            main_root: false,
            children: HashMap::new(),
            types: HashMap::new(),
            fns: HashMap::new(),
        });
        if body == 0 && self.file_pkg[file].is_none() {
            self.file_pkg[file] = Some(pi);
        }
        pi
    }

    fn display(&self, root: usize, segs: &[String]) -> String {
        let r = &self.pkgs[root];
        let name = self.b.name(r.node);
        if name == LIB_ROOT { segs.join("::") } else { format!("{name}::{}", segs.join("::")) }
    }

    fn build_packages(&mut self) {
        for mi in 0..self.d.modules.len() {
            if self.d.modules[mi].lang != Lang::Rust {
                continue;
            }
            let cc = cargo_crate(self.d, mi);
            let main_rs = self.d.modules[mi].dir.join("src/main.rs");
            let mut targets: Vec<(String, PathBuf, bool, bool)> = Vec::new();
            if let Some(lib) = &cc.lib {
                targets.push((LIB_ROOT.to_string(), lib.clone(), false, false));
            }
            for (name, path) in &cc.bins {
                let display = if *path == main_rs { name.clone() } else { format!("bin/{name}") };
                targets.push((display, path.clone(), false, true));
            }
            for (dir, list, test) in [("examples", &cc.examples, false), ("tests", &cc.tests, true), ("benches", &cc.benches, false)] {
                for (name, path) in list {
                    targets.push((format!("{dir}/{name}"), path.clone(), test, true));
                }
            }
            if let Some(p) = &cc.build_script {
                targets.push((BUILD_ROOT.to_string(), p.clone(), false, true));
            }
            let mut info = CrateInfo { lib_name: cc.lib_name.clone(), lib_root: None, externs: cc.deps.iter().cloned().collect(), ext_names: cc.externs.iter().cloned().collect() };
            for (display, path, test, main_root) in targets {
                let Some(&fi) = self.file_ix.get(&path) else { continue };
                if self.file_pkg[fi].is_some() {
                    continue;
                }
                let root = self.new_pkg(mi, &display, None, None, Vec::new(), fi, 0, test || self.files[fi].is_test);
                self.pkgs[root].main_root = main_root;
                if display == LIB_ROOT {
                    info.lib_root = Some(root);
                }
                let dir = path.parent().unwrap().to_path_buf();
                self.walk(root, fi, 0, &dir);
            }
            let dir = self.d.modules[mi].dir.clone();
            let lib_root = info.lib_root;
            self.crates.insert(mi, info);
            for fi in 0..self.files.len() {
                if self.files[fi].module != mi || self.file_pkg[fi].is_some() {
                    continue;
                }
                let rel = rel_path(&dir, &self.files[fi].path);
                let stem = rel.trim_end_matches(".rs").trim_end_matches("/mod").to_string();
                let test = self.files[fi].is_test;
                let pi = match (stem.strip_prefix("src/"), lib_root) {
                    (Some(inner), Some(root)) => {
                        let segs = segments(&inner.replace('/', "::"));
                        let display = self.display(root, &segs);
                        self.new_pkg(mi, &display, Some(root), None, segs, fi, 0, test)
                    }
                    _ => self.new_pkg(mi, &stem, None, None, Vec::new(), fi, 0, test),
                };
                let mod_dir = if self.files[fi].path.file_name().is_some_and(|n| n == "mod.rs") {
                    self.files[fi].path.parent().unwrap().to_path_buf()
                } else {
                    self.files[fi].path.with_extension("")
                };
                self.walk(pi, fi, 0, &mod_dir);
            }
        }
    }

    fn walk(&mut self, pkg: usize, file: usize, body: usize, mod_dir: &Path) {
        let files = self.files;
        for m in &files[file].rf.bodies[body].mods {
            let test = self.pkgs[pkg].test || has_cfg_test(&m.attrs);
            let mut segs = self.pkgs[pkg].segs.clone();
            segs.push(m.name.clone());
            let root = self.pkgs[pkg].root;
            let module = self.pkgs[pkg].module;
            let child = match m.inline {
                Some(bi) => {
                    let display = self.display(root, &segs);
                    let c = self.new_pkg(module, &display, Some(root), Some(pkg), segs, file, bi, test);
                    self.walk(c, file, bi, &mod_dir.join(&m.name));
                    Some(c)
                }
                None => {
                    let path_attr = m.attrs.iter().find(|a| a.name == "path").and_then(|a| a.args.first());
                    let candidates: Vec<PathBuf> = match path_attr {
                        Some(p) if body == 0 => vec![files[file].path.parent().unwrap().join(p)],
                        Some(p) => vec![mod_dir.join(p)],
                        None => vec![mod_dir.join(format!("{}.rs", m.name)), mod_dir.join(&m.name).join("mod.rs")],
                    };
                    match candidates.iter().find_map(|c| self.file_ix.get(c).map(|&fi| (fi, c))) {
                        Some((fi, _)) if self.file_pkg[fi].is_some() => self.file_pkg[fi],
                        Some((fi, path)) => {
                            let display = self.display(root, &segs);
                            let c = self.new_pkg(module, &display, Some(root), Some(pkg), segs, fi, 0, test || files[fi].is_test);
                            let child_dir = if path_attr.is_some() || path.file_name().is_some_and(|n| n == "mod.rs") {
                                path.parent().unwrap().to_path_buf()
                            } else {
                                path.with_extension("")
                            };
                            self.walk(c, fi, 0, &child_dir);
                            Some(c)
                        }
                        None => None,
                    }
                }
            };
            if let Some(c) = child {
                self.pkgs[pkg].children.entry(m.name.clone()).or_insert(c);
            }
        }
    }

    fn fqn(&self, pkg: usize, name: &str) -> String {
        let p = &self.pkgs[pkg];
        let mut parts: Vec<String> = vec![self.crates.get(&p.module).map(|c| c.lib_name.clone()).unwrap_or_default()];
        let root_name = self.b.name(self.pkgs[p.root].node);
        if root_name != LIB_ROOT {
            parts.push(root_name.to_string());
        }
        parts.extend(p.segs.iter().cloned());
        parts.push(name.to_string());
        parts.join(".")
    }

    fn flag_test(&mut self, node: NodeId, test: bool) {
        if test {
            self.b.add_flags(node, NodeFlags::TEST);
        }
    }

    fn add_fn(&mut self, parent: NodeId, sub: SubKind, f: &RsFn, rel: &str, test: bool) -> NodeId {
        let id = self.b.add_node(parent, NodeKind::Member, sub, &f.name, Lang::Rust);
        self.b.set_span(id, rel, f.start_line, f.end_line);
        self.b.set_lines(id, f.end_line - f.start_line + 1);
        self.flag_test(id, test);
        id
    }

    fn declare(&mut self) {
        let files = self.files;
        for pi in 0..self.pkgs.len() {
            let (file, body, test, pnode) = (self.pkgs[pi].file, self.pkgs[pi].body, self.pkgs[pi].test, self.pkgs[pi].node);
            let f = &files[file];
            let bd = &f.rf.bodies[body];
            for (i, t) in bd.types.iter().enumerate() {
                let ttest = test || has_cfg_test(&t.attrs);
                let ti = match self.pkgs[pi].types.get(&t.name) {
                    Some(&ti) => ti,
                    None => self.new_type(pi, &t.name, t.kind, &f.rel, (t.start_line, t.end_line), ttest, t.fields.iter().cloned().collect()),
                };
                self.type_occs[file].push((pi, body, i));
                for (j, m) in t.methods.iter().enumerate() {
                    let info = match self.types[ti].methods.get(&m.name) {
                        Some(&info) => info,
                        None => {
                            let node = self.add_fn(self.types[ti].node, SubKind::Method, m, &f.rel, ttest);
                            self.push_fn(FnInfo { node, pkg: pi, owner: Some(ti), ret: m.ret.clone(), generics: m.generics.clone() }, m.has_self && !ttest, f.module, &m.name)
                        }
                    };
                    self.types[ti].methods.insert(m.name.clone(), info);
                    self.occs[file].push(FnOcc { pkg: pi, at: FnRef::TraitMethod(body, i, j), info, overrides_external: None });
                }
            }
            for (i, func) in bd.fns.iter().enumerate() {
                let info = match self.pkgs[pi].fns.get(&func.name) {
                    Some(&info) => info,
                    None => {
                        let node = self.add_fn(pnode, SubKind::Function, func, &f.rel, test || is_test_attr(&func.attrs));
                        let info = self.push_fn(FnInfo { node, pkg: pi, owner: None, ret: func.ret.clone(), generics: func.generics.clone() }, false, f.module, &func.name);
                        self.pkgs[pi].fns.insert(func.name.clone(), info);
                        info
                    }
                };
                self.occs[file].push(FnOcc { pkg: pi, at: FnRef::Free(body, i), info, overrides_external: None });
            }
        }
    }

    fn new_type(&mut self, pi: usize, name: &str, kind: SubKind, rel: &str, (start, end): (u32, u32), test: bool, fields: HashMap<String, String>) -> usize {
        let node = self.b.add_node(self.pkgs[pi].node, NodeKind::Type, kind, name, Lang::Rust);
        self.b.set_span(node, rel, start, end);
        self.b.set_lines(node, end - start + 1);
        self.flag_test(node, test);
        let ti = self.types.len();
        let fqn = self.fqn(pi, name);
        self.types.push(TypeInfo { node, pkg: pi, kind, fqn, fields, methods: HashMap::new(), traits: Vec::new(), implementors: Vec::new(), supers: Vec::new() });
        self.pkgs[pi].types.insert(name.to_string(), ti);
        ti
    }

    fn declare_impl_only_types(&mut self) {
        let files = self.files;
        for pi in 0..self.pkgs.len() {
            let (file, body, test) = (self.pkgs[pi].file, self.pkgs[pi].body, self.pkgs[pi].test);
            for imp in &files[file].rf.bodies[body].impls {
                let simple = !imp.self_ty.is_empty() && !imp.self_ty.contains("::") && imp.self_ty != "Self";
                let generic = imp.generics.iter().any(|(g, _)| *g == imp.self_ty);
                if imp.trait_.is_some() || !simple || generic || PRELUDE.contains(&imp.self_ty.as_str()) || self.resolve_path(pi, &segments(&imp.self_ty), 0).is_some() {
                    continue;
                }
                self.memo.borrow_mut().remove(&(pi, imp.self_ty.clone()));
                let ti = self.new_type(pi, &imp.self_ty, SubKind::Struct, &files[file].rel, (imp.start_line, imp.end_line), test || has_cfg_test(&imp.attrs), HashMap::new());
                self.impl_only.insert((self.pkgs[pi].module, imp.self_ty.clone()), ti);
            }
        }
        self.memo.borrow_mut().clear();
    }

    fn push_fn(&mut self, info: FnInfo, method: bool, module: usize, name: &str) -> usize {
        if method {
            self.method_names.entry(name.to_string()).or_default().push((info.node, module));
        }
        self.fns.push(info);
        self.fns.len() - 1
    }

    fn external_node(&mut self, name: &str) -> NodeId {
        if let Some(&n) = self.externals.get(name) {
            return n;
        }
        let root = self.b.root();
        let module = self.b.child(root, EXTERNAL_MODULE).unwrap_or_else(|| self.b.add_node(root, NodeKind::Module, SubKind::None, EXTERNAL_MODULE, Lang::None));
        self.b.add_flags(module, NodeFlags::EXTERNAL);
        let n = self.b.child(module, name).unwrap_or_else(|| self.b.add_node(module, NodeKind::External, SubKind::None, name, Lang::Rust));
        self.b.add_flags(n, NodeFlags::EXTERNAL);
        self.externals.insert(name.to_string(), n);
        n
    }

    fn attach_impls(&mut self) {
        let files = self.files;
        for pi in 0..self.pkgs.len() {
            let (file, body, test) = (self.pkgs[pi].file, self.pkgs[pi].body, self.pkgs[pi].test);
            let f = &files[file];
            for (i, imp) in f.rf.bodies[body].impls.iter().enumerate() {
                let itest = test || has_cfg_test(&imp.attrs);
                let generic = imp.generics.iter().any(|(g, _)| *g == imp.self_ty);
                let owner = match self.resolve_path(pi, &segments(&imp.self_ty), 0) {
                    Some(Item::Type(t)) if !generic => Some(t),
                    _ => None,
                };
                let mut overrides = None;
                if let (Some(tr), Some(t)) = (&imp.trait_, owner) {
                    let scope = if itest { Scope::Test } else { Scope::Main };
                    match self.resolve_path(pi, &segments(tr), 0) {
                        Some(Item::Type(trait_ti)) => {
                            self.b.add_edge(self.types[t].node, self.types[trait_ti].node, EdgeKind::Implements, scope, Confidence::Resolved, EdgeFlags::NONE);
                            self.types[t].traits.push(trait_ti);
                            self.types[trait_ti].implementors.push(t);
                            let fqn = self.types[trait_ti].fqn.clone();
                            self.types[t].supers.push(fqn);
                        }
                        Some(Item::Ext(path)) => {
                            if !is_std(&path) {
                                let ext = self.external_node(&path);
                                self.b.add_edge(self.types[t].node, ext, EdgeKind::Implements, scope, Confidence::Resolved, EdgeFlags::NONE);
                                overrides = Some(path.clone());
                            }
                            self.types[t].supers.push(path);
                        }
                        _ => self.types[t].supers.push(tr.replace("::", ".")),
                    }
                }
                for (j, func) in imp.fns.iter().enumerate() {
                    let ftest = itest || is_test_attr(&func.attrs);
                    let info = match owner {
                        Some(t) => match self.types[t].methods.get(&func.name).copied() {
                            Some(m) => m,
                            None => {
                                let tname = self.b.name(self.types[t].node).to_string();
                                let ctor = !func.has_self && func.ret.as_deref().is_some_and(|r| r == "Self" || r.rsplit("::").next() == Some(tname.as_str()));
                                let sub = if ctor { SubKind::Constructor } else { SubKind::Method };
                                let node = self.add_fn(self.types[t].node, sub, func, &f.rel, ftest);
                                let m = self.push_fn(FnInfo { node, pkg: pi, owner: Some(t), ret: func.ret.clone(), generics: func.generics.clone() }, func.has_self && !ftest, f.module, &func.name);
                                self.types[t].methods.insert(func.name.clone(), m);
                                m
                            }
                        },
                        None => {
                            let last = imp.self_ty.rsplit("::").next().unwrap_or("");
                            let name = if last.is_empty() { func.name.clone() } else { format!("{last}::{}", func.name) };
                            let mut renamed = func.clone();
                            renamed.name = name;
                            let node = self.add_fn(self.pkgs[pi].node, SubKind::Method, &renamed, &f.rel, ftest);
                            self.push_fn(FnInfo { node, pkg: pi, owner: None, ret: func.ret.clone(), generics: func.generics.clone() }, false, f.module, &func.name)
                        }
                    };
                    self.occs[file].push(FnOcc { pkg: pi, at: FnRef::ImplMethod(body, i, j), info, overrides_external: overrides.clone() });
                }
            }
        }
    }

    fn module_edges(&mut self) {
        for l in &self.d.links {
            if self.d.modules[l.from].lang == Lang::Rust && self.d.modules[l.to].lang == Lang::Rust {
                self.b.add_edge(self.module_nodes[l.from], self.module_nodes[l.to], EdgeKind::DependsOn, l.scope, Confidence::Resolved, EdgeFlags::NONE);
            }
        }
    }

    fn item_pkg(&self, item: &Item) -> Option<usize> {
        match item {
            Item::Pkg(p) => Some(*p),
            Item::Type(t) | Item::Variant(t) | Item::Assoc(t) => Some(self.types[*t].pkg),
            Item::Fn(f) => Some(self.fns[*f].owner.map_or(self.fns[*f].pkg, |t| self.types[t].pkg)),
            Item::Ext(_) => None,
        }
    }

    fn use_targets(&self, pi: usize) -> Vec<usize> {
        let p = &self.pkgs[pi];
        let mut out = Vec::new();
        for u in &self.files[p.file].rf.bodies[p.body].uses {
            if let Some(q) = self.resolve_path(pi, &u.path, 0).and_then(|it| self.item_pkg(&it)) {
                if q != pi && !out.contains(&q) {
                    out.push(q);
                }
            }
        }
        out
    }

    fn package_edges(&mut self) {
        let mut edges = Vec::new();
        for pi in 0..self.pkgs.len() {
            let from_module = self.pkgs[pi].module;
            let scope = if self.pkgs[pi].test || self.files[self.pkgs[pi].file].is_test { Scope::Test } else { Scope::Main };
            for q in self.use_targets(pi) {
                let to_module = self.pkgs[q].module;
                if to_module == from_module {
                    continue;
                }
                let flags = if self.visible[from_module].contains(&to_module) { EdgeFlags::NONE } else { EdgeFlags::UNDECLARED };
                edges.push((self.pkgs[pi].node, self.pkgs[q].node, scope, flags));
            }
        }
        for (from, to, scope, flags) in edges {
            self.b.add_edge(from, to, EdgeKind::DependsOn, scope, Confidence::Resolved, flags);
        }
    }

    fn extern_head(&self, pkg: usize, name: &str) -> Option<Item> {
        let p = &self.pkgs[pkg];
        match name {
            "crate" => return Some(Item::Pkg(p.root)),
            "self" => return Some(Item::Pkg(pkg)),
            "super" => return p.parent.map(Item::Pkg),
            _ => {}
        }
        let c = self.crates.get(&p.module)?;
        if let Some(&m) = c.externs.get(name) {
            return self.crates.get(&m).and_then(|x| x.lib_root).map(Item::Pkg);
        }
        if name == c.lib_name {
            if let Some(r) = c.lib_root {
                return Some(Item::Pkg(r));
            }
        }
        (STD_CRATES.contains(&name) || c.ext_names.contains(name)).then(|| Item::Ext(name.to_string()))
    }

    fn lookup(&self, pkg: usize, name: &str, depth: u32) -> Option<Item> {
        if depth > MAX_DEPTH {
            return None;
        }
        let key = (pkg, name.to_string());
        if let Some(hit) = self.memo.borrow().get(&key) {
            return hit.clone();
        }
        let found = self.lookup_uncached(pkg, name, depth);
        if depth == 0 || found.is_some() {
            self.memo.borrow_mut().insert(key, found.clone());
        }
        found
    }

    fn lookup_uncached(&self, pkg: usize, name: &str, depth: u32) -> Option<Item> {
        let p = &self.pkgs[pkg];
        if let Some(&c) = p.children.get(name) {
            return Some(Item::Pkg(c));
        }
        if let Some(&t) = p.types.get(name) {
            return Some(Item::Type(t));
        }
        if let Some(&f) = p.fns.get(name) {
            return Some(Item::Fn(f));
        }
        let uses = &self.files[p.file].rf.bodies[p.body].uses;
        for u in uses.iter().filter(|u| !u.glob) {
            if u.alias.as_deref().or(u.path.last().map(String::as_str)) != Some(name) {
                continue;
            }
            let hit = if u.path.len() == 1 { self.extern_head(pkg, &u.path[0]) } else { self.resolve_path(pkg, &u.path, depth + 1) };
            if hit.is_some() {
                return hit;
            }
        }
        let mut ext_glob = None;
        let mut project_glob = false;
        for u in uses.iter().filter(|u| u.glob) {
            match self.resolve_path(pkg, &u.path, depth + 1) {
                Some(Item::Pkg(q)) if q != pkg => {
                    project_glob = true;
                    if let Some(hit) = self.lookup(q, name, depth + 1) {
                        return Some(hit);
                    }
                }
                Some(Item::Ext(s)) => {
                    ext_glob.get_or_insert(s);
                }
                _ => {}
            }
        }
        if project_glob {
            if let Some(&t) = self.impl_only.get(&(p.module, name.to_string())) {
                return Some(Item::Type(t));
            }
        }
        ext_glob.map(|s| Item::Ext(format!("{s}.{name}")))
    }

    fn resolve_path(&self, pkg: usize, segs: &[String], depth: u32) -> Option<Item> {
        let head = segs.first()?;
        let mut item = match head.as_str() {
            "crate" | "self" | "super" => self.extern_head(pkg, head)?,
            _ => self.lookup(pkg, head, depth).or_else(|| self.extern_head(pkg, head)).or_else(|| PRELUDE.contains(&head.as_str()).then(|| Item::Ext(head.clone())))?,
        };
        for seg in &segs[1..] {
            item = self.step(item, seg, depth)?;
        }
        Some(item)
    }

    fn step(&self, item: Item, seg: &str, depth: u32) -> Option<Item> {
        Some(match item {
            Item::Pkg(p) => match seg {
                "super" => Item::Pkg(self.pkgs[p].parent?),
                "self" => Item::Pkg(p),
                _ => self.lookup(p, seg, depth + 1)?,
            },
            Item::Type(t) => match self.lookup_method(t, seg) {
                Some((_, f)) => Item::Fn(f),
                None if seg.starts_with(char::is_uppercase) => Item::Variant(t),
                None => Item::Assoc(t),
            },
            Item::Ext(s) => Item::Ext(format!("{s}.{seg}")),
            Item::Fn(_) | Item::Variant(_) | Item::Assoc(_) => return None,
        })
    }

    fn lookup_method(&self, t: usize, name: &str) -> Option<(usize, usize)> {
        if let Some(&f) = self.types[t].methods.get(name) {
            return Some((t, f));
        }
        self.types[t].traits.iter().find_map(|&tr| self.types[tr].methods.get(name).map(|&f| (tr, f)))
    }

    fn resolve_ty(&self, pkg: usize, s: &str, self_ty: Option<usize>, generics: &[(String, String)]) -> Option<TypeRef> {
        if s.is_empty() {
            return None;
        }
        if s == "Self" {
            return self_ty.map(TypeRef::Project);
        }
        if let Some((_, bound)) = generics.iter().find(|(g, _)| g == s) {
            return self.resolve_ty(pkg, bound, self_ty, &[]);
        }
        match self.resolve_path(pkg, &segments(s), 0)? {
            Item::Type(t) => Some(TypeRef::Project(t)),
            Item::Ext(x) => Some(TypeRef::External(x)),
            _ => None,
        }
    }

    fn fn_ret(&self, fi: usize) -> Option<TypeRef> {
        let f = &self.fns[fi];
        self.resolve_ty(f.pkg, f.ret.as_deref()?, f.owner, &f.generics)
    }

    fn var_type(&self, cx: &FnCtx, name: &str, upto: usize, depth: u32) -> Option<TypeRef> {
        if name == "self" {
            return cx.self_ty.map(TypeRef::Project);
        }
        if let Some(i) = cx.f.locals[..upto].iter().rposition(|(n, _)| n == name) {
            return self.expr_type(cx, &cx.f.locals[i].1, i, depth + 1);
        }
        let (_, ty) = cx.f.params.iter().find(|(n, _)| n == name)?;
        self.resolve_ty(cx.pkg, ty, cx.self_ty, &cx.f.generics)
    }

    fn expr_type(&self, cx: &FnCtx, e: &Expr, upto: usize, depth: u32) -> Option<TypeRef> {
        if depth > MAX_DEPTH {
            return None;
        }
        match e {
            Expr::Ty(s) => self.resolve_ty(cx.pkg, s, cx.self_ty, &cx.f.generics),
            Expr::Var(head, fields) => {
                let mut cur = self.var_type(cx, head, upto, depth)?;
                for f in fields {
                    cur = match cur {
                        TypeRef::Project(t) => self.field_type(t, f)?,
                        TypeRef::External(_) | TypeRef::Opaque => TypeRef::Opaque,
                    };
                }
                Some(cur)
            }
            Expr::Call(callee) => match &**callee {
                Callee::Path(segs) => match self.resolve_callee(cx, segs)? {
                    Item::Fn(f) => self.fn_ret(f),
                    Item::Type(t) => Some(TypeRef::Project(t)),
                    Item::Assoc(t) if SELF_RETURNING.contains(&segs.last()?.as_str()) => Some(TypeRef::Project(t)),
                    Item::Ext(_) => Some(TypeRef::Opaque),
                    _ => None,
                },
                Callee::Method(recv, name) if PASS_THROUGH.contains(&name.as_str()) => self.expr_type(cx, recv, upto, depth + 1),
                Callee::Method(recv, name) => match self.expr_type(cx, recv, upto, depth + 1)? {
                    TypeRef::Project(t) => self.lookup_method(t, name).and_then(|(_, f)| self.fn_ret(f)),
                    TypeRef::External(_) | TypeRef::Opaque => Some(TypeRef::Opaque),
                },
                _ => None,
            },
            Expr::Unknown => None,
        }
    }

    fn field_type(&self, t: usize, field: &str) -> Option<TypeRef> {
        let ty = self.types[t].fields.get(field)?;
        self.resolve_ty(self.types[t].pkg, ty, Some(t), &[])
    }

    fn resolve_callee(&self, cx: &FnCtx, segs: &[String]) -> Option<Item> {
        if segs.first().is_some_and(|s| s == "Self") {
            let mut item = Item::Type(cx.self_ty?);
            for seg in &segs[1..] {
                item = self.step(item, seg, 0)?;
            }
            return Some(item);
        }
        self.resolve_path(cx.pkg, segs, 0)
    }

    fn call_edge(&mut self, cx: &FnCtx, target: NodeId, confidence: Confidence) {
        self.b.add_edge(cx.caller, target, EdgeKind::Calls, cx.scope, confidence, EdgeFlags::NONE);
    }

    fn resolved_fn(&mut self, cx: &FnCtx, fi: usize, cf: &mut CallFacts) {
        let node = self.fns[fi].node;
        self.call_edge(cx, node, Confidence::Resolved);
        cf.target = Some(node);
        self.stats.resolved += 1;
        let Some(owner) = self.fns[fi].owner else { return };
        if self.types[owner].kind != SubKind::Interface {
            return;
        }
        let name = &cf.method;
        let impls: Vec<NodeId> = self.types[owner]
            .implementors
            .iter()
            .filter(|&&t| self.visible[cx.module].contains(&self.pkgs[self.types[t].pkg].module))
            .filter_map(|&t| self.types[t].methods.get(name).map(|&m| self.fns[m].node))
            .collect();
        if let [single] = impls[..] {
            self.call_edge(cx, single, Confidence::Resolved);
        }
    }

    fn instantiate(&mut self, cx: &FnCtx, item: Option<Item>, cf: &mut CallFacts) {
        match item {
            Some(Item::Type(t) | Item::Variant(t)) => {
                cf.instantiates = Some(self.types[t].fqn.clone());
                let to = self.types[t].node;
                self.b.add_edge(cx.caller, to, EdgeKind::Instantiates, cx.scope, Confidence::Resolved, EdgeFlags::NONE);
            }
            Some(Item::Ext(s)) => cf.instantiates = Some(s),
            _ => {}
        }
    }

    fn unknown_receiver(&mut self, cx: &FnCtx, name: &str, guess: bool, cf: &mut CallFacts) {
        let all: Vec<(NodeId, usize)> = self.method_names.get(name).map(|v| v.iter().filter(|(_, m)| self.visible[cx.module].contains(m)).copied().collect()).unwrap_or_default();
        let imported: Vec<NodeId> = all.iter().filter(|(_, m)| cx.imported.contains(m)).map(|(n, _)| *n).collect();
        match (&all[..], &imported[..]) {
            (_, [target]) if guess && !STD_METHODS.contains(&name) => {
                let target = *target;
                self.call_edge(cx, target, Confidence::Guessed);
                cf.target = Some(target);
                self.stats.guessed += 1;
            }
            _ => self.stats.unresolved += 1,
        }
    }

    fn resolve_call(&mut self, cx: &FnCtx, call: &RsCall) -> CallFacts {
        let mut cf = CallFacts { caller: cx.caller, string_args: call.string_args.clone(), ident_args: call.ident_args.clone(), assigned_to: call.assigned_to.clone(), line: call.line, ..Default::default() };
        match &call.callee {
            Callee::Struct(segs) => {
                cf.method = segs.last().cloned().unwrap_or_default();
                let item = self.resolve_callee(cx, segs);
                self.instantiate(cx, item, &mut cf);
            }
            Callee::Path(segs) => {
                let name = segs.last().cloned().unwrap_or_default();
                cf.method = name.clone();
                if segs.len() > 1 {
                    cf.receiver_var = Some(segs[..segs.len() - 1].join("::"));
                } else if BUILTIN_CALLS.contains(&name.as_str()) || cx.f.params.iter().any(|(n, _)| *n == name) || cx.f.locals.iter().any(|(n, _)| *n == name) {
                    return cf;
                }
                match self.resolve_callee(cx, segs) {
                    Some(Item::Fn(fi)) => {
                        if let Some(o) = self.fns[fi].owner {
                            cf.receiver_type = Some(self.types[o].fqn.clone());
                        }
                        self.resolved_fn(cx, fi, &mut cf);
                    }
                    Some(item @ (Item::Type(_) | Item::Variant(_))) => self.instantiate(cx, Some(item), &mut cf),
                    Some(Item::Ext(s)) => {
                        cf.receiver_type = s.rsplit_once('.').map(|(q, _)| q.to_string());
                        self.stats.resolved += 1;
                    }
                    Some(Item::Assoc(t)) if STD_METHODS.contains(&name.as_str()) => {
                        cf.receiver_type = Some(self.types[t].fqn.clone());
                        self.stats.resolved += 1;
                    }
                    _ => self.stats.unresolved += 1,
                }
            }
            Callee::Method(recv, name) => {
                cf.method = name.clone();
                if let Expr::Var(head, _) = recv {
                    cf.receiver_var = Some(head.clone());
                }
                match self.expr_type(cx, recv, cx.f.locals.len(), 0) {
                    Some(TypeRef::Project(t)) => {
                        cf.receiver_type = Some(self.types[t].fqn.clone());
                        match self.lookup_method(t, name) {
                            Some((_, fi)) => self.resolved_fn(cx, fi, &mut cf),
                            None if STD_METHODS.contains(&name.as_str()) => self.stats.resolved += 1,
                            None => self.unknown_receiver(cx, name, false, &mut cf),
                        }
                    }
                    Some(TypeRef::External(s)) => {
                        cf.receiver_type = Some(s);
                        self.stats.resolved += 1;
                    }
                    Some(TypeRef::Opaque) => self.unknown_receiver(cx, name, false, &mut cf),
                    None => self.unknown_receiver(cx, name, true, &mut cf),
                }
            }
            Callee::Opaque(text) => {
                cf.method = text.clone();
                self.stats.unresolved += 1;
            }
        }
        cf
    }

    fn rs_fn(&self, file: usize, at: FnRef) -> &'a RsFn {
        let files = self.files;
        let bodies = &files[file].rf.bodies;
        match at {
            FnRef::Free(b, i) => &bodies[b].fns[i],
            FnRef::TraitMethod(b, i, j) => &bodies[b].types[i].methods[j],
            FnRef::ImplMethod(b, i, j) => &bodies[b].impls[i].fns[j],
        }
    }

    fn resolve(&mut self, facts: &mut ProjectFacts) {
        let files = self.files;
        for (fi, f) in files.iter().enumerate() {
            self.stats.files += 1;
            self.stats.lines += f.rf.lines as u64;
            self.stats.bytes += f.bytes;
            let Some(own) = self.file_pkg[fi] else { continue };
            let mut import_targets: Vec<NodeId> = Vec::new();
            let mut imports: Vec<Import> = Vec::new();
            let mut imported: HashSet<usize> = HashSet::from([f.module]);
            let file_pkgs: Vec<usize> = (0..self.pkgs.len()).filter(|&p| self.pkgs[p].file == fi).collect();
            for &p in &file_pkgs {
                for q in self.use_targets(p) {
                    imported.insert(self.pkgs[q].module);
                    let n = self.pkgs[q].node;
                    if n != self.pkgs[own].node && !import_targets.contains(&n) {
                        import_targets.push(n);
                    }
                }
                let pk = &self.pkgs[p];
                for u in &f.rf.bodies[pk.body].uses {
                    imports.push(Import { path: u.path.join("."), alias: u.alias.clone(), wildcard: u.glob, is_static: false });
                }
            }
            let is_test = f.is_test || self.pkgs[own].test;
            let mut ff = FileFacts {
                rel_path: f.rel.clone(),
                module: self.module_nodes[f.module],
                package: Some(self.pkgs[own].node),
                import_targets,
                lang: Lang::Rust,
                is_test,
                is_dev: f.is_dev,
                imports,
                types: Vec::new(),
                functions: Vec::new(),
                calls: Vec::new(),
            };
            for &(pi, body, i) in &self.type_occs[fi] {
                let t = &f.rf.bodies[body].types[i];
                let ti = self.pkgs[pi].types[&t.name];
                ff.types.push(TypeFacts { node: self.types[ti].node, fqn: self.types[ti].fqn.clone(), supertypes: self.types[ti].supers.clone(), annotations: t.attrs.clone() });
            }
            let occs = std::mem::take(&mut self.occs[fi]);
            for occ in &occs {
                let func = self.rs_fn(fi, occ.at);
                let info = &self.fns[occ.info];
                let (node, owner) = (info.node, info.owner);
                let pkg = &self.pkgs[occ.pkg];
                let is_main = matches!(occ.at, FnRef::Free(0, _)) && func.name == "main" && pkg.main_root && pkg.root == occ.pkg;
                ff.functions.push(FnFacts {
                    node,
                    owner: owner.map(|t| self.types[t].node),
                    name: func.name.clone(),
                    params: func.params.clone(),
                    annotations: func.attrs.clone(),
                    is_main,
                    overrides_external: occ.overrides_external.clone(),
                });
                let test = is_test || self.b.flags(node).contains(NodeFlags::TEST);
                let cx = FnCtx { pkg: occ.pkg, self_ty: owner, f: func, caller: node, scope: if test { Scope::Test } else { Scope::Main }, module: f.module, imported: &imported };
                for call in &func.calls {
                    let cf = self.resolve_call(&cx, call);
                    ff.calls.push(cf);
                }
            }
            facts.files.push(ff);
        }
    }
}
