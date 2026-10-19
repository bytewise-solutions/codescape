use crate::parse::{CALL_SIGNATURE, CHAIN, Dialect, TsCall, TsFile, TsFunc, TsUse, TypeExpr, parse_ts};
use codescape_build::{Discovery, in_npm_excluded_dir, is_npm_source, is_npm_tooling_file, read_json_loose, rel_path, walk_files_under};
use codescape_facts::{CallFacts, ExtractStats, FileFacts, FnFacts, Import, ProjectFacts, TypeFacts, TypeUse};
use codescape_format::*;
use globset::GlobSet;
use rayon::prelude::*;
use serde_json::Value;
use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};

const EXTERNAL_MODULE: &str = "(external)";
const MAX_BYTES: u64 = 1 << 20;
const EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];
const GLOBAL_OBJECTS: &[&str] = &[
    "console",
    "Math",
    "JSON",
    "Object",
    "Array",
    "Promise",
    "Number",
    "String",
    "Boolean",
    "Symbol",
    "Reflect",
    "Date",
    "window",
    "document",
    "globalThis",
    "localStorage",
    "sessionStorage",
    "navigator",
    "location",
    "history",
    "process",
    "crypto",
    "performance",
    "Intl",
    "URL",
    "URLSearchParams",
    "Buffer",
    "module",
    "exports",
    "Error",
    "Map",
    "Set",
    "WeakMap",
    "RegExp",
    "BigInt",
    "Atomics",
    "self",
];
const GLOBAL_FUNCTIONS: &[&str] = &[
    "setTimeout",
    "setInterval",
    "clearTimeout",
    "clearInterval",
    "setImmediate",
    "queueMicrotask",
    "requestAnimationFrame",
    "cancelAnimationFrame",
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "String",
    "Number",
    "Boolean",
    "Array",
    "Object",
    "Symbol",
    "BigInt",
    "Date",
    "RegExp",
    "Error",
    "Promise",
    "fetch",
    "structuredClone",
    "encodeURIComponent",
    "decodeURIComponent",
    "encodeURI",
    "decodeURI",
    "alert",
    "confirm",
    "prompt",
    "atob",
    "btoa",
    "super",
    "describe",
    "it",
    "test",
    "expect",
    "beforeEach",
    "afterEach",
    "beforeAll",
    "afterAll",
    "fdescribe",
    "xdescribe",
    "fit",
    "xit",
    "spyOn",
    "fail",
    "pending",
];
const NO_TYPE: &[&str] = &["any", "unknown", "void", "never", "object", "null", "undefined", "this", "Function"];
pub(crate) const PRIMITIVES: &[&str] = &["string", "number", "boolean", "bigint", "symbol"];

struct ParsedFile {
    module: usize,
    rel: String,
    stem_path: String,
    dir: String,
    name: String,
    abs: PathBuf,
    lang: Lang,
    is_test: bool,
    is_mock: bool,
    is_dev: bool,
    bytes: u64,
    tf: TsFile,
}

#[derive(Clone, Debug, PartialEq)]
enum TypeRef {
    Project(usize),
    External(String),
}

#[derive(Clone, Debug)]
enum Val {
    Inst(TypeRef),
    Static(usize),
    Ns(usize),
    Func(NodeId),
}

#[derive(Clone, Debug)]
enum Binding {
    Symbol(usize, String),
    Namespace(usize),
    External(String, String),
    ExternalNs(String),
}

#[derive(Clone, Debug)]
enum Sym {
    Type(usize),
    Func(usize, String),
    Var(usize, TypeExpr),
    External(String),
    Namespace(usize),
}

#[derive(Clone, Debug)]
enum Spec {
    File(usize),
    External(String),
    Unresolved,
}

enum Local<'f> {
    Typed(&'f TypeExpr),
    Param(&'f str),
    Missing,
}

struct TypeInfo {
    node: NodeId,
    fqn: String,
    kind: SubKind,
    file: usize,
    fields: HashMap<String, TypeExpr>,
    methods: HashMap<String, NodeId>,
    returns: HashMap<String, String>,
    supers: Vec<TypeRef>,
    is_test: bool,
}

struct FuncInfo {
    node: NodeId,
    ret: Option<String>,
}

#[derive(Default)]
struct TsPaths {
    base: Option<PathBuf>,
    paths: Vec<(String, Vec<String>)>,
    paths_base: Option<PathBuf>,
}

#[derive(Clone, Copy)]
struct Ctx<'f> {
    fi: usize,
    ti: Option<usize>,
    func: Option<&'f TsFunc>,
}

fn display(spec: &str, name: &str) -> String {
    if name == "default" { spec.to_string() } else { format!("{spec}.{name}") }
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn package_name(spec: &str) -> (&str, &str) {
    let split = if spec.starts_with('@') { spec.match_indices('/').nth(1).map(|(i, _)| i) } else { spec.find('/') };
    match split {
        Some(i) => (&spec[..i], &spec[i + 1..]),
        None => (spec, ""),
    }
}

pub fn extract_ts(d: &Discovery, module_nodes: &[NodeId], excludes: &GlobSet, b: &mut GraphBuilder, facts: &mut ProjectFacts) -> ExtractStats {
    let sources = list_sources(d, excludes);
    if sources.is_empty() {
        return ExtractStats::default();
    }
    let parsed: Vec<ParsedFile> = sources
        .par_iter()
        .filter_map(|(module, path)| {
            let bytes = std::fs::read(path).ok()?;
            let ext = path.extension()?.to_str()?;
            let dialect = Dialect::for_extension(ext)?;
            let m = &d.modules[*module];
            let mrel = rel_path(&m.dir, path);
            let name = path.file_name()?.to_string_lossy().into_owned();
            let segs: Vec<&str> = mrel.split('/').collect();
            let dirs = &segs[..segs.len() - 1];
            Some(ParsedFile {
                module: *module,
                rel: rel_path(&d.root, path),
                stem_path: mrel[..mrel.len() - ext.len() - 1].to_string(),
                dir: dirs.join("/"),
                abs: path.clone(),
                lang: if dialect == Dialect::JavaScript { Lang::JavaScript } else { Lang::TypeScript },
                is_test: name.contains(".spec.") || name.contains(".test.") || dirs.contains(&"__tests__"),
                is_mock: dirs.contains(&"__mocks__"),
                is_dev: is_npm_tooling_file(path),
                bytes: bytes.len() as u64,
                tf: parse_ts(&bytes, dialect),
                name,
            })
        })
        .collect();
    let mut r = Resolver::new(d, module_nodes, b, &parsed);
    r.declare();
    r.bind();
    r.link_types();
    r.module_edges();
    r.resolve(facts);
    r.stats
}

fn list_sources(d: &Discovery, excludes: &GlobSet) -> Vec<(usize, PathBuf)> {
    let dirs: Vec<(usize, &Path)> =
        d.modules.iter().enumerate().filter(|(_, m)| matches!(m.lang, Lang::TypeScript | Lang::JavaScript)).map(|(i, m)| (i, m.dir.as_path())).collect();
    let mut out = Vec::new();
    for &(mi, dir) in &dirs {
        let _ = walk_files_under(&d.root, dir, excludes, |p| {
            if !is_npm_source(p) || in_npm_excluded_dir(dir, p) || std::fs::metadata(p).is_ok_and(|m| m.len() > MAX_BYTES) {
                return;
            }
            let owner = dirs.iter().filter(|(_, md)| p.starts_with(md)).max_by_key(|(_, md)| md.as_os_str().len()).map(|(i, _)| *i);
            if owner == Some(mi) {
                out.push((mi, p.to_path_buf()));
            }
        });
    }
    out.sort();
    out
}

fn load_tsconfig(path: &Path, depth: u32, out: &mut TsPaths) {
    if depth > 5 {
        return;
    }
    let Some(cfg) = read_json_loose(path) else { return };
    let dir = path.parent().unwrap_or(Path::new("."));
    let parents: Vec<&str> = match cfg.get("extends") {
        Some(Value::String(s)) => vec![s.as_str()],
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str()).collect(),
        _ => Vec::new(),
    };
    for p in parents.into_iter().filter(|p| p.starts_with('.')) {
        let mut target = dir.join(p);
        if target.extension().is_none_or(|e| e != "json") {
            target.set_extension("json");
        }
        load_tsconfig(&normalize(&target), depth + 1, out);
    }
    let Some(opts) = cfg.get("compilerOptions") else { return };
    if let Some(base) = opts.get("baseUrl").and_then(|b| b.as_str()) {
        out.base = Some(normalize(&dir.join(base)));
    }
    if let Some(Value::Object(paths)) = opts.get("paths") {
        out.paths = paths
            .iter()
            .map(|(k, v)| (k.clone(), v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()))
            .collect();
        out.paths_base = Some(dir.to_path_buf());
    }
}

struct Resolver<'a> {
    d: &'a Discovery,
    module_nodes: &'a [NodeId],
    b: &'a mut GraphBuilder,
    files: &'a [ParsedFile],
    file_by_rel: HashMap<String, usize>,
    file_pkg: Vec<NodeId>,
    pkg_nodes: HashMap<(usize, String), NodeId>,
    types: Vec<TypeInfo>,
    file_types: Vec<HashMap<String, usize>>,
    file_funcs: Vec<HashMap<String, FuncInfo>>,
    top_nodes: Vec<Option<NodeId>>,
    bindings: Vec<HashMap<String, Binding>>,
    specs: HashMap<(usize, String), Spec>,
    tsconfigs: HashMap<usize, TsPaths>,
    manifests: HashMap<usize, Value>,
    npm_names: HashMap<String, usize>,
    entry_files: HashSet<usize>,
    subtypes: HashMap<usize, Vec<usize>>,
    externals: HashMap<String, NodeId>,
    depth: Cell<u32>,
    stats: ExtractStats,
}

impl<'a> Resolver<'a> {
    fn new(d: &'a Discovery, module_nodes: &'a [NodeId], b: &'a mut GraphBuilder, files: &'a [ParsedFile]) -> Self {
        let mut manifests = HashMap::new();
        let mut tsconfigs = HashMap::new();
        let mut npm_names = HashMap::new();
        for (mi, m) in d.modules.iter().enumerate().filter(|(_, m)| matches!(m.lang, Lang::TypeScript | Lang::JavaScript)) {
            let manifest = read_json_loose(&m.dir.join("package.json")).unwrap_or(Value::Null);
            if let Some(n) = manifest.get("name").and_then(|n| n.as_str()) {
                npm_names.insert(n.to_string(), mi);
            }
            manifests.insert(mi, manifest);
            let mut cfg = TsPaths::default();
            if let Some(path) = ["tsconfig.json", "jsconfig.json"].iter().map(|f| m.dir.join(f)).find(|p| p.is_file()) {
                load_tsconfig(&path, 0, &mut cfg);
            }
            tsconfigs.insert(mi, cfg);
        }
        Resolver {
            d,
            module_nodes,
            b,
            files,
            file_by_rel: files.iter().enumerate().map(|(i, f)| (f.rel.clone(), i)).collect(),
            file_pkg: Vec::new(),
            pkg_nodes: HashMap::new(),
            types: Vec::new(),
            file_types: Vec::new(),
            file_funcs: Vec::new(),
            top_nodes: Vec::new(),
            bindings: Vec::new(),
            specs: HashMap::new(),
            tsconfigs,
            manifests,
            npm_names,
            entry_files: HashSet::new(),
            subtypes: HashMap::new(),
            externals: HashMap::new(),
            depth: Cell::new(0),
            stats: ExtractStats::default(),
        }
    }

    fn try_file(&self, p: &Path) -> Option<usize> {
        let p = normalize(p);
        let lookup = |c: &Path| self.file_by_rel.get(&rel_path(&self.d.root, c)).copied();
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            if let Some(f) = lookup(&p) {
                return Some(f);
            }
            let swapped: &[&str] = match ext {
                "js" => &["ts", "tsx"],
                "jsx" => &["tsx"],
                "mjs" => &["mts"],
                "cjs" => &["cts"],
                _ => &[],
            };
            if let Some(f) = swapped.iter().find_map(|s| lookup(&p.with_extension(s))) {
                return Some(f);
            }
        }
        let s = p.to_string_lossy();
        EXTENSIONS.iter().find_map(|e| lookup(Path::new(&format!("{s}.{e}")))).or_else(|| EXTENSIONS.iter().find_map(|e| lookup(&p.join(format!("index.{e}")))))
    }

    fn manifest_file(&self, mi: usize, rel: &str) -> Option<usize> {
        let dir = &self.d.modules[mi].dir;
        let rel = rel.trim_start_matches("./");
        let mut candidates = vec![rel.to_string()];
        let stripped = rel.trim_end_matches(".d.ts").trim_end_matches(".js").trim_end_matches(".mjs").trim_end_matches(".cjs");
        for out in ["dist/", "lib/", "build/", "out/"] {
            if let Some(rest) = stripped.strip_prefix(out) {
                candidates.push(format!("src/{rest}"));
            }
        }
        candidates.push(stripped.to_string());
        candidates.iter().find_map(|c| self.try_file(&dir.join(c)))
    }

    fn module_entry(&self, mi: usize) -> Option<usize> {
        let manifest = self.manifests.get(&mi)?;
        ["source", "types", "typings", "module", "main"]
            .iter()
            .filter_map(|k| manifest.get(*k).and_then(|v| v.as_str()))
            .find_map(|v| self.manifest_file(mi, v))
            .or_else(|| self.try_file(&self.d.modules[mi].dir.join("src/index")))
            .or_else(|| self.try_file(&self.d.modules[mi].dir.join("index")))
    }

    fn resolve_spec(&mut self, fi: usize, spec: &str) -> Spec {
        let key = (fi, spec.to_string());
        if let Some(s) = self.specs.get(&key) {
            return s.clone();
        }
        let s = self.compute_spec(fi, spec);
        self.specs.insert(key, s.clone());
        s
    }

    fn compute_spec(&self, fi: usize, spec: &str) -> Spec {
        let f = &self.files[fi];
        if spec.starts_with('.') {
            return self.try_file(&f.abs.parent().unwrap().join(spec)).map_or(Spec::Unresolved, Spec::File);
        }
        if let Some(cfg) = self.tsconfigs.get(&f.module) {
            let base = cfg.base.clone().or_else(|| cfg.paths_base.clone());
            for (pattern, targets) in &cfg.paths {
                let sub = match pattern.split_once('*') {
                    Some((pre, suf)) => spec.strip_prefix(pre).and_then(|r| r.strip_suffix(suf)),
                    None => (pattern == spec).then_some(""),
                };
                let (Some(sub), Some(base)) = (sub, &base) else { continue };
                if let Some(found) = targets.iter().find_map(|t| self.try_file(&base.join(t.replace('*', sub)))) {
                    return Spec::File(found);
                }
            }
            if let Some(found) = cfg.base.as_ref().and_then(|b| self.try_file(&b.join(spec))) {
                return Spec::File(found);
            }
        }
        let (name, sub) = package_name(spec);
        if let Some(&mi) = self.npm_names.get(name) {
            let dir = &self.d.modules[mi].dir;
            let found =
                if sub.is_empty() { self.module_entry(mi) } else { self.try_file(&dir.join(sub)).or_else(|| self.try_file(&dir.join("src").join(sub))) };
            return found.map_or(Spec::Unresolved, Spec::File);
        }
        Spec::External(spec.to_string())
    }

    fn flag(&mut self, node: NodeId, f: &ParsedFile) {
        if f.is_test {
            self.b.add_flags(node, NodeFlags::TEST);
        }
        if f.is_mock {
            self.b.add_flags(node, NodeFlags::MOCK);
        }
    }

    fn entry_files_of(&self, mi: usize) -> Vec<usize> {
        let Some(manifest) = self.manifests.get(&mi) else { return Vec::new() };
        let mut rels: Vec<String> = Vec::new();
        if let Some(m) = manifest.get("main").and_then(|v| v.as_str()) {
            rels.push(m.to_string());
        }
        match manifest.get("bin") {
            Some(Value::String(s)) => rels.push(s.clone()),
            Some(Value::Object(o)) => rels.extend(o.values().filter_map(|v| v.as_str().map(String::from))),
            _ => {}
        }
        if let Some(start) = manifest.get("scripts").and_then(|s| s.get("start")).and_then(|s| s.as_str()) {
            let words: Vec<&str> = start.split_whitespace().collect();
            if words.first().is_some_and(|w| matches!(*w, "node" | "ts-node" | "tsx" | "nodemon" | "bun")) {
                rels.extend(words.iter().skip(1).filter(|w| !w.starts_with('-')).take(1).map(|w| w.to_string()));
            }
        }
        rels.iter().filter_map(|r| self.manifest_file(mi, r)).collect()
    }

    fn declare(&mut self) {
        let modules: Vec<usize> = self.manifests.keys().copied().collect();
        for mi in modules {
            let entries = self.entry_files_of(mi);
            self.entry_files.extend(entries);
        }
        let files = self.files;
        for (fi, f) in files.iter().enumerate() {
            self.stats.files += 1;
            self.stats.lines += f.tf.lines as u64;
            self.stats.bytes += f.bytes;
            let key = (f.module, f.dir.clone());
            let pkg = match self.pkg_nodes.get(&key) {
                Some(&p) => p,
                None => {
                    let name = if f.dir.is_empty() { "(root)" } else { f.dir.as_str() };
                    let p = self.b.add_node(self.module_nodes[f.module], NodeKind::Package, SubKind::None, name, self.d.modules[f.module].lang);
                    if f.is_mock {
                        self.b.add_flags(p, NodeFlags::MOCK);
                    }
                    self.pkg_nodes.insert(key, p);
                    p
                }
            };
            self.file_pkg.push(pkg);
            let stem = f.name.split('.').next().unwrap_or("").to_string();
            let mut types = HashMap::new();
            for t in &f.tf.types {
                let shown = if t.name == "default" { stem.as_str() } else { t.name.as_str() };
                let node = self.b.add_node(pkg, NodeKind::Type, t.kind, shown, f.lang);
                self.b.set_visibility(node, exported(&f.tf, &t.name));
                self.b.set_span(node, &f.rel, t.start_line, t.end_line);
                self.b.set_lines(node, t.end_line - t.start_line + 1);
                self.flag(node, f);
                let mut methods = HashMap::new();
                for m in &t.methods {
                    let id = self.b.add_node(node, NodeKind::Member, m.sub, &m.name, f.lang);
                    self.b.set_visibility(id, if m.visibility == Visibility::Unknown { Visibility::Public } else { m.visibility });
                    self.b.set_complexity_detail(id, m.complexity.total, &m.complexity.decisions, m.complexity.nesting);
                    self.b.set_span(id, &f.rel, m.start_line, m.end_line);
                    self.b.set_lines(id, m.end_line - m.start_line + 1);
                    self.flag(id, f);
                    methods.insert(m.name.clone(), id);
                }
                for fd in &t.field_decls {
                    let id = self.b.add_node(node, NodeKind::Member, SubKind::Field, &fd.name, f.lang);
                    if !fd.ty.is_empty() {
                        self.b.set_detail(id, &fd.ty);
                    }
                    self.b.set_visibility(id, fd.visibility);
                    self.b.set_span(id, &f.rel, fd.line, fd.line);
                    self.flag(id, f);
                }
                let mut fields = HashMap::new();
                for (n, e) in &t.fields {
                    fields.entry(n.clone()).or_insert_with(|| e.clone());
                }
                types.insert(t.name.clone(), self.types.len());
                self.types.push(TypeInfo {
                    node,
                    fqn: format!("{}/{}.{}", self.d.modules[f.module].name, f.stem_path, t.name),
                    kind: t.kind,
                    file: fi,
                    fields,
                    methods,
                    returns: t.methods.iter().filter_map(|m| m.returns.clone().map(|r| (m.name.clone(), r))).collect(),
                    supers: Vec::new(),
                    is_test: f.is_test,
                });
            }
            self.file_types.push(types);
            let mut funcs = HashMap::new();
            for func in &f.tf.funcs {
                let shown = if func.name == "default" { stem.as_str() } else { func.name.as_str() };
                let node = self.b.add_node(pkg, NodeKind::Member, SubKind::Function, shown, f.lang);
                self.b.set_visibility(node, exported(&f.tf, &func.name));
                self.b.set_complexity_detail(node, func.complexity.total, &func.complexity.decisions, func.complexity.nesting);
                self.b.set_span(node, &f.rel, func.start_line, func.end_line);
                self.b.set_lines(node, func.end_line - func.start_line + 1);
                self.flag(node, f);
                funcs.entry(func.name.clone()).or_insert(FuncInfo { node, ret: func.returns.clone() });
            }
            self.file_funcs.push(funcs);
            let top = (!f.tf.top.calls.is_empty() || self.entry_files.contains(&fi)).then(|| {
                let node = self.b.add_node(pkg, NodeKind::Member, SubKind::Function, &f.name, f.lang);
                self.b.set_span(node, &f.rel, 1, f.tf.lines);
                node
            });
            if let Some(node) = top {
                self.flag(node, f);
            }
            self.top_nodes.push(top);
        }
    }

    fn bind(&mut self) {
        for fi in 0..self.files.len() {
            let mut map = HashMap::new();
            for imp in self.files[fi].tf.imports.iter().filter(|i| !i.dynamic) {
                match self.resolve_spec(fi, &imp.spec) {
                    Spec::File(t) => {
                        if let Some(local) = &imp.default {
                            let b = if self.files[t].tf.default_export.is_some() { Binding::Symbol(t, "default".into()) } else { Binding::Namespace(t) };
                            map.insert(local.clone(), b);
                        }
                        if let Some(ns) = &imp.namespace {
                            map.insert(ns.clone(), Binding::Namespace(t));
                        }
                        for (orig, local) in &imp.named {
                            map.insert(local.clone(), Binding::Symbol(t, orig.clone()));
                        }
                    }
                    Spec::External(s) => {
                        if let Some(local) = &imp.default {
                            map.insert(local.clone(), Binding::External(s.clone(), "default".into()));
                        }
                        if let Some(ns) = &imp.namespace {
                            map.insert(ns.clone(), Binding::ExternalNs(s.clone()));
                        }
                        for (orig, local) in &imp.named {
                            map.insert(local.clone(), Binding::External(s.clone(), orig.clone()));
                        }
                    }
                    Spec::Unresolved => {}
                }
            }
            self.bindings.push(map);
        }
    }

    fn reexport_targets(&self, fi: usize) -> Vec<(Spec, Option<Vec<(String, String)>>)> {
        self.files[fi]
            .tf
            .reexports
            .iter()
            .map(|r| (self.specs.get(&(fi, r.spec.clone())).cloned().unwrap_or_else(|| self.compute_spec(fi, &r.spec)), r.names.clone()))
            .collect()
    }

    fn symbol(&self, fi: usize, name: &str, depth: u32) -> Option<Sym> {
        if depth > 8 {
            return None;
        }
        if let Some(&t) = self.file_types[fi].get(name) {
            return Some(Sym::Type(t));
        }
        if self.file_funcs[fi].contains_key(name) {
            return Some(Sym::Func(fi, name.to_string()));
        }
        let tf = &self.files[fi].tf;
        if let Some((_, e)) = tf.vars.iter().find(|(n, _)| n == name) {
            return Some(Sym::Var(fi, e.clone()));
        }
        if name == "default"
            && let Some(de) = tf.default_export.as_deref().filter(|d| *d != "default")
        {
            return self.symbol(fi, de, depth + 1);
        }
        if let Some((local, _)) = tf.exports.iter().find(|(l, e)| e == name && l != name) {
            return self.symbol(fi, local, depth + 1);
        }
        if let Some(b) = self.bindings[fi].get(name) {
            return self.binding_sym(b, depth + 1);
        }
        for (spec, names) in self.reexport_targets(fi) {
            let found = match (&spec, &names) {
                (Spec::File(t), Some(list)) => list.iter().find(|(_, alias)| alias == name).and_then(|(orig, _)| self.symbol(*t, orig, depth + 1)),
                (Spec::External(s), Some(list)) => list.iter().find(|(_, alias)| alias == name).map(|(orig, _)| Sym::External(display(s, orig))),
                (Spec::File(t), None) => self.symbol(*t, name, depth + 1),
                _ => None,
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }

    fn binding_sym(&self, b: &Binding, depth: u32) -> Option<Sym> {
        match b {
            Binding::Symbol(f, n) => self.symbol(*f, n, depth),
            Binding::Namespace(f) => Some(Sym::Namespace(*f)),
            Binding::External(s, n) => Some(Sym::External(display(s, n))),
            Binding::ExternalNs(s) => Some(Sym::External(s.clone())),
        }
    }

    fn name_sym(&self, fi: usize, name: &str) -> Option<Sym> {
        if let Some(&t) = self.file_types[fi].get(name) {
            return Some(Sym::Type(t));
        }
        if self.file_funcs[fi].contains_key(name) {
            return Some(Sym::Func(fi, name.to_string()));
        }
        if let Some((_, e)) = self.files[fi].tf.vars.iter().find(|(n, _)| n == name) {
            return Some(Sym::Var(fi, e.clone()));
        }
        self.bindings[fi].get(name).and_then(|b| self.binding_sym(b, 0))
    }

    fn sym_val(&self, s: Sym) -> Option<Val> {
        match s {
            Sym::Type(t) => Some(Val::Static(t)),
            Sym::Func(f, n) => Some(Val::Func(self.file_funcs[f][&n].node)),
            Sym::Var(f, e) => self.value_of(Ctx { fi: f, ti: None, func: None }, &e, false),
            Sym::External(s) => Some(Val::Inst(TypeRef::External(s))),
            Sym::Namespace(f) => Some(Val::Ns(f)),
        }
    }

    fn resolve_type(&self, fi: usize, raw: &str, awaited: bool) -> Option<TypeRef> {
        let raw = raw.trim();
        if raw.is_empty() || NO_TYPE.contains(&raw) {
            return None;
        }
        if let Some(inner) = raw.strip_prefix("Promise<").and_then(|r| r.strip_suffix('>')) {
            return if awaited { self.resolve_type(fi, inner, false) } else { Some(TypeRef::External("Promise".into())) };
        }
        if raw == "[]" {
            return Some(TypeRef::External("Array".into()));
        }
        if PRIMITIVES.contains(&raw) {
            return Some(TypeRef::External(raw.to_string()));
        }
        if let Some((ns, name)) = raw.split_once('.') {
            return match self.bindings[fi].get(ns) {
                Some(Binding::Namespace(t)) => match self.symbol(*t, name, 0) {
                    Some(Sym::Type(ti)) => Some(TypeRef::Project(ti)),
                    Some(Sym::External(s)) => Some(TypeRef::External(s)),
                    _ => None,
                },
                Some(Binding::ExternalNs(s)) => Some(TypeRef::External(format!("{s}.{name}"))),
                _ => None,
            };
        }
        match self.name_sym(fi, raw) {
            Some(Sym::Type(t)) => Some(TypeRef::Project(t)),
            Some(Sym::External(s)) => Some(TypeRef::External(s)),
            Some(_) => None,
            None => raw.chars().next().is_some_and(|c| c.is_uppercase()).then(|| TypeRef::External(raw.to_string())),
        }
    }

    fn local<'f>(&self, ctx: Ctx<'f>, name: &str) -> Local<'f> {
        let Some(func) = ctx.func else { return Local::Missing };
        if let Some((_, e)) = func.locals.iter().rev().find(|(n, _)| n == name) {
            return Local::Typed(e);
        }
        match func.params.iter().find(|(n, _)| n == name) {
            Some((_, t)) => Local::Param(t),
            None => Local::Missing,
        }
    }

    fn value_of(&self, ctx: Ctx, e: &TypeExpr, awaited: bool) -> Option<Val> {
        if self.depth.get() > 12 {
            return None;
        }
        self.depth.set(self.depth.get() + 1);
        let out = match e {
            TypeExpr::Named(raw) => self.resolve_type(ctx.fi, raw, awaited).map(Val::Inst),
            TypeExpr::Path(p) => self.resolve_value(ctx, p),
            TypeExpr::CallOf(callee) => self.call_result(ctx, callee, awaited),
            TypeExpr::Await(inner) => self.value_of(ctx, inner, true),
        };
        self.depth.set(self.depth.get() - 1);
        out
    }

    fn head_value(&self, ctx: Ctx, head: &str) -> Option<Val> {
        match head {
            "this" => return ctx.ti.map(|t| Val::Inst(TypeRef::Project(t))),
            "super" => return ctx.ti.and_then(|t| self.types[t].supers.first().cloned()).map(Val::Inst),
            _ => {}
        }
        match self.local(ctx, head) {
            Local::Typed(e) => return self.value_of(ctx, e, false),
            Local::Param(t) => return if t.is_empty() { None } else { self.resolve_type(ctx.fi, t, false).map(Val::Inst) },
            Local::Missing => {}
        }
        match self.name_sym(ctx.fi, head) {
            Some(s) => self.sym_val(s),
            None => GLOBAL_OBJECTS.contains(&head).then(|| Val::Inst(TypeRef::External(head.to_string()))),
        }
    }

    fn resolve_value(&self, ctx: Ctx, path: &str) -> Option<Val> {
        let mut segs = path.split('.');
        let first = segs.next()?;
        let mut cur = match first.strip_suffix("()") {
            Some(name) => self.call_result(ctx, name, false)?,
            None => self.head_value(ctx, first)?,
        };
        for seg in segs {
            cur = match seg.strip_suffix("()") {
                Some(name) => self.call_member(cur, name, false)?,
                None => self.member(cur, seg)?,
            };
        }
        Some(cur)
    }

    fn call_member(&self, v: Val, m: &str, awaited: bool) -> Option<Val> {
        match v {
            Val::Inst(TypeRef::Project(t)) | Val::Static(t) => match self.lookup_method(t, m) {
                Some((owner, _)) => {
                    let ret = self.types[owner].returns.get(m)?;
                    self.resolve_type(self.types[owner].file, ret, awaited).map(Val::Inst)
                }
                None => match self.field_type(t, m)? {
                    Val::Inst(TypeRef::External(s)) => Some(Val::Inst(TypeRef::External(format!("{s}()")))),
                    _ => None,
                },
            },
            Val::Inst(TypeRef::External(s)) => Some(Val::Inst(TypeRef::External(format!("{s}.{m}()")))),
            Val::Ns(f) => match self.symbol(f, m, 0)? {
                Sym::Func(f, n) => self.func_ret(f, &n, awaited),
                Sym::External(s) => Some(Val::Inst(TypeRef::External(format!("{s}()")))),
                _ => None,
            },
            Val::Func(_) => None,
        }
    }

    fn member(&self, v: Val, seg: &str) -> Option<Val> {
        match v {
            Val::Inst(TypeRef::Project(t)) | Val::Static(t) => self.field_type(t, seg).or_else(|| self.lookup_method(t, seg).map(|(_, m)| Val::Func(m))),
            Val::Inst(TypeRef::External(s)) => Some(Val::Inst(TypeRef::External(s))),
            Val::Ns(f) => self.symbol(f, seg, 0).and_then(|s| self.sym_val(s)),
            Val::Func(_) => None,
        }
    }

    fn supers_bfs(&self, t: usize) -> Vec<usize> {
        let mut seen = HashSet::new();
        let mut order = Vec::new();
        let mut q = VecDeque::from([t]);
        while let Some(cur) = q.pop_front() {
            if !seen.insert(cur) {
                continue;
            }
            order.push(cur);
            for s in &self.types[cur].supers {
                if let TypeRef::Project(p) = s {
                    q.push_back(*p);
                }
            }
        }
        order
    }

    fn field_type(&self, t: usize, name: &str) -> Option<Val> {
        for cur in self.supers_bfs(t) {
            let info = &self.types[cur];
            if let Some(e) = info.fields.get(name) {
                return self.value_of(Ctx { fi: info.file, ti: Some(cur), func: None }, e, false);
            }
            if info.methods.contains_key(name)
                && let Some(ret) = info.returns.get(name)
            {
                return self.resolve_type(info.file, ret, false).map(Val::Inst);
            }
        }
        None
    }

    fn lookup_method(&self, t: usize, name: &str) -> Option<(usize, NodeId)> {
        self.supers_bfs(t).into_iter().find_map(|cur| self.types[cur].methods.get(name).map(|&m| (cur, m)))
    }

    fn has_external_super(&self, t: usize) -> bool {
        self.supers_bfs(t).iter().any(|&c| self.types[c].supers.iter().any(|s| matches!(s, TypeRef::External(_))))
    }

    fn func_ret(&self, f: usize, name: &str, awaited: bool) -> Option<Val> {
        let ret = self.file_funcs[f].get(name)?.ret.as_ref()?;
        self.resolve_type(f, ret, awaited).map(Val::Inst)
    }

    fn call_result(&self, ctx: Ctx, callee: &str, awaited: bool) -> Option<Val> {
        let Some((recv, m)) = callee.rsplit_once('.') else {
            if !matches!(self.local(ctx, callee), Local::Missing) {
                return None;
            }
            return match self.name_sym(ctx.fi, callee)? {
                Sym::Func(f, n) => self.func_ret(f, &n, awaited),
                Sym::External(s) => Some(Val::Inst(TypeRef::External(format!("{s}()")))),
                Sym::Var(f, e) => match self.value_of(Ctx { fi: f, ti: None, func: None }, &e, false)? {
                    Val::Inst(TypeRef::Project(t)) => self.call_member(Val::Inst(TypeRef::Project(t)), CALL_SIGNATURE, awaited),
                    _ => None,
                },
                _ => None,
            };
        };
        let v = self.resolve_value(ctx, recv)?;
        self.call_member(v, m, awaited)
    }

    fn external_node(&mut self, name: &str, lang: Lang) -> NodeId {
        if let Some(&n) = self.externals.get(name) {
            return n;
        }
        let root = self.b.root();
        let module = self.b.child(root, EXTERNAL_MODULE).unwrap_or_else(|| self.b.add_node(root, NodeKind::Module, SubKind::None, EXTERNAL_MODULE, Lang::None));
        self.b.add_flags(module, NodeFlags::EXTERNAL);
        let n = self.b.child(module, name).unwrap_or_else(|| self.b.add_node(module, NodeKind::External, SubKind::None, name, lang));
        self.b.add_flags(n, NodeFlags::EXTERNAL);
        self.externals.insert(name.to_string(), n);
        n
    }

    fn type_name(&self, r: &TypeRef) -> String {
        match r {
            TypeRef::Project(t) => self.types[*t].fqn.clone(),
            TypeRef::External(s) => s.clone(),
        }
    }

    fn link_types(&mut self) {
        let files = self.files;
        let mut ti = 0;
        for fi in 0..files.len() {
            let f = &files[fi];
            for t in &f.tf.types {
                let scope = if f.is_test { Scope::Test } else { Scope::Main };
                let raws = t.extends.iter().map(|s| (s, false)).chain(t.implements.iter().map(|s| (s, true)));
                let mut supers = Vec::new();
                for (raw, implements) in raws {
                    let Some(r) = self.resolve_type(fi, raw, false) else { continue };
                    let kind = match &r {
                        TypeRef::Project(s) if self.types[*s].kind == SubKind::Interface => EdgeKind::Implements,
                        _ if implements || t.kind == SubKind::Interface => EdgeKind::Implements,
                        _ => EdgeKind::Inherits,
                    };
                    let to = match &r {
                        TypeRef::Project(s) => self.types[*s].node,
                        TypeRef::External(s) => self.external_node(s, f.lang),
                    };
                    self.b.add_edge(self.types[ti].node, to, kind, scope, Confidence::Resolved, EdgeFlags::NONE);
                    if let TypeRef::Project(s) = r {
                        self.subtypes.entry(s).or_default().push(ti);
                    }
                    supers.push(r);
                }
                self.types[ti].supers = supers;
                ti += 1;
            }
        }
        let files = self.files;
        let mut ti = 0;
        for fi in 0..files.len() {
            let f = &files[fi];
            for t in &f.tf.types {
                let scope = if f.is_test { Scope::Test } else { Scope::Main };
                for r in &t.decorator_refs {
                    if let Some(Val::Static(target)) = self.resolve_value(Ctx { fi, ti: None, func: None }, r)
                        && target != ti
                    {
                        self.b.add_edge(self.types[ti].node, self.types[target].node, EdgeKind::Instantiates, scope, Confidence::Resolved, EdgeFlags::NONE);
                    }
                }
                ti += 1;
            }
        }
    }

    fn is_ts_js(&self, m: usize) -> bool {
        matches!(self.d.modules[m].lang, Lang::TypeScript | Lang::JavaScript)
    }

    fn module_edges(&mut self) {
        for l in &self.d.links {
            if self.is_ts_js(l.from) && self.is_ts_js(l.to) {
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

    fn single_impl(&self, owner: usize, name: &str) -> Option<NodeId> {
        if self.types[owner].kind != SubKind::Interface {
            return None;
        }
        let mut impls = Vec::new();
        let mut seen = HashSet::new();
        let mut q = VecDeque::from([owner]);
        while let Some(t) = q.pop_front() {
            for &s in self.subtypes.get(&t).into_iter().flatten() {
                if !seen.insert(s) || self.types[s].is_test || self.files[self.types[s].file].is_mock {
                    continue;
                }
                if self.types[s].kind != SubKind::Interface {
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

    fn resolve(&mut self, facts: &mut ProjectFacts) {
        let reach = self.declared_reach();
        let files = self.files;
        for fi in 0..files.len() {
            let f = &files[fi];
            let own = self.file_pkg[fi];
            let scope = if f.is_test { Scope::Test } else { Scope::Main };
            let mut import_targets: Vec<NodeId> = Vec::new();
            let mut imports = Vec::new();
            for imp in &f.tf.imports {
                imports.push(Import {
                    path: imp.spec.clone(),
                    alias: imp.default.clone().or_else(|| imp.namespace.clone()),
                    wildcard: imp.namespace.is_some(),
                    is_static: false,
                });
                let Spec::File(t) = self.resolve_spec(fi, &imp.spec) else { continue };
                let target = self.file_pkg[t];
                if target == own || import_targets.contains(&target) {
                    continue;
                }
                import_targets.push(target);
                let (fm, tm) = (f.module, self.files[t].module);
                if fm != tm {
                    let flags = if reach[fm].contains(&tm) { EdgeFlags::NONE } else { EdgeFlags::UNDECLARED };
                    self.b.add_edge(own, target, EdgeKind::DependsOn, scope, Confidence::Resolved, flags);
                }
            }
            let mut ff = FileFacts {
                rel_path: f.rel.clone(),
                module: self.module_nodes[f.module],
                package: Some(own),
                import_targets,
                lang: f.lang,
                is_test: f.is_test,
                is_dev: f.is_dev,
                imports,
                types: Vec::new(),
                functions: Vec::new(),
                calls: Vec::new(),
                type_uses: Vec::new(),
            };
            for t in &f.tf.types {
                let ti = self.file_types[fi][&t.name];
                let info = &self.types[ti];
                ff.types.push(TypeFacts {
                    node: info.node,
                    fqn: info.fqn.clone(),
                    supertypes: info.supers.iter().map(|s| self.type_name(s)).collect(),
                    annotations: t.decorators.clone(),
                });
                let from = info.node;
                self.record_uses(fi, from, &t.type_uses, &[], &mut ff.type_uses);
                for m in &t.methods {
                    let node = self.types[ti].methods[&m.name];
                    ff.functions.push(FnFacts {
                        node,
                        owner: Some(self.types[ti].node),
                        name: m.name.clone(),
                        params: m.params.clone(),
                        annotations: m.decorators.clone(),
                        is_main: false,
                        overrides_external: None,
                    });
                    self.record_uses(fi, node, &m.type_uses, &m.declared, &mut ff.type_uses);
                    let ctx = Ctx { fi, ti: Some(ti), func: Some(m) };
                    for call in &m.calls {
                        let cf = self.resolve_call(ctx, node, scope, call);
                        ff.calls.push(cf);
                    }
                }
            }
            for func in &f.tf.funcs {
                let node = self.file_funcs[fi][&func.name].node;
                if ff.functions.iter().any(|x| x.node == node) {
                    continue;
                }
                ff.functions.push(FnFacts {
                    node,
                    owner: None,
                    name: self.b.name(node).to_string(),
                    params: func.params.clone(),
                    annotations: Vec::new(),
                    is_main: false,
                    overrides_external: None,
                });
                self.record_uses(fi, node, &func.type_uses, &func.declared, &mut ff.type_uses);
                let ctx = Ctx { fi, ti: None, func: Some(func) };
                for call in &func.calls {
                    let cf = self.resolve_call(ctx, node, scope, call);
                    ff.calls.push(cf);
                }
            }
            if let Some(node) = self.top_nodes[fi] {
                let top = &f.tf.top;
                ff.functions.push(FnFacts {
                    node,
                    owner: None,
                    name: f.name.clone(),
                    params: Vec::new(),
                    annotations: Vec::new(),
                    is_main: self.entry_files.contains(&fi),
                    overrides_external: None,
                });
                self.record_uses(fi, node, &top.type_uses, &top.declared, &mut ff.type_uses);
                let ctx = Ctx { fi, ti: None, func: Some(top) };
                for call in &top.calls {
                    let cf = self.resolve_call(ctx, node, scope, call);
                    ff.calls.push(cf);
                }
            }
            facts.files.push(ff);
        }
    }

    fn record_uses(&mut self, fi: usize, from: NodeId, uses: &[TsUse], declared: &[String], out: &mut Vec<TypeUse>) {
        for u in uses {
            out.push(TypeUse { owner: from, name: u.name.clone(), line: u.line, col: u.col, len: u.len });
            let head = u.name.split('.').next().unwrap_or("");
            if declared.iter().any(|d| d == head) {
                continue;
            }
            if let Some(TypeRef::Project(t)) = self.resolve_type(fi, &u.name, false) {
                self.b.add_ref(from, self.types[t].node, u.line, u.col, u.len);
            }
        }
    }

    fn head_is_clear(&self, ctx: Ctx, path: &str, strict: bool) -> bool {
        let head = path.split('.').next().unwrap_or("").trim_end_matches("()");
        let times = ctx.func.map_or(0, |f| f.declared.iter().filter(|d| *d == head).count());
        match self.local(ctx, head) {
            Local::Missing => times == 0,
            _ => !strict && times <= 1,
        }
    }

    fn edge(&mut self, from: NodeId, to: NodeId, kind: EdgeKind, scope: Scope, line: u32) {
        self.b.add_edge_at(from, to, kind, scope, Confidence::Resolved, EdgeFlags::NONE, line);
    }

    fn resolve_call(&mut self, ctx: Ctx, caller: NodeId, scope: Scope, call: &TsCall) -> CallFacts {
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
        let ref_from = if call.in_field { ctx.ti.map_or(caller, |t| self.types[t].node) } else { caller };
        cf.arg_targets = call
            .ident_args
            .iter()
            .map(|a| match a.as_deref().and_then(|a| self.resolve_value(ctx, a)) {
                Some(Val::Func(n)) => Some(n),
                _ => None,
            })
            .collect();
        if call.is_new {
            match self.resolve_value(ctx, &call.method) {
                Some(Val::Static(t)) => {
                    let to = self.types[t].node;
                    self.edge(caller, to, EdgeKind::Instantiates, scope, call.line);
                    if self.head_is_clear(ctx, &call.method, true) {
                        self.b.add_ref(ref_from, to, call.line, call.col, call.len);
                    }
                    cf.instantiates = Some(self.types[t].fqn.clone());
                }
                Some(Val::Inst(TypeRef::External(s))) => cf.instantiates = Some(s),
                _ => {}
            }
            cf.receiver_type = cf.instantiates.clone();
            return cf;
        }
        let Some(recv) = call.receiver.as_deref() else {
            if !matches!(self.local(ctx, &call.method), Local::Missing) {
                return cf;
            }
            match self.resolve_value(ctx, &call.method) {
                Some(Val::Func(target)) => {
                    self.edge(caller, target, EdgeKind::Calls, scope, call.line);
                    if self.head_is_clear(ctx, &call.method, true) {
                        self.b.add_ref(ref_from, target, call.line, call.col, call.len);
                    }
                    cf.target = Some(target);
                    self.stats.resolved += 1;
                }
                Some(Val::Inst(TypeRef::External(_))) => self.stats.external += 1,
                Some(Val::Inst(TypeRef::Project(t))) => match self.lookup_method(t, CALL_SIGNATURE) {
                    Some((_, target)) => {
                        self.edge(caller, target, EdgeKind::Calls, scope, call.line);
                        cf.target = Some(target);
                        self.stats.resolved += 1;
                    }
                    None if self.has_external_super(t) => self.stats.external += 1,
                    None => self.stats.unresolved += 1,
                },
                Some(Val::Static(_)) => {}
                _ if GLOBAL_FUNCTIONS.contains(&call.method.as_str()) => {}
                _ => self.stats.unresolved += 1,
            }
            return cf;
        };
        if recv == CHAIN {
            self.stats.unresolved += 1;
            return cf;
        }
        cf.receiver_var = recv.split('.').next().map(String::from);
        let value = self.resolve_value(ctx, recv);
        let strict = matches!(value, Some(Val::Static(_) | Val::Ns(_)));
        let clear = self.head_is_clear(ctx, recv, strict);
        match value {
            Some(Val::Inst(TypeRef::Project(t))) | Some(Val::Static(t)) => {
                cf.receiver_type = Some(self.types[t].fqn.clone());
                match self.lookup_method(t, &call.method) {
                    Some((owner, target)) => {
                        self.edge(caller, target, EdgeKind::Calls, scope, call.line);
                        if clear {
                            self.b.add_ref(ref_from, target, call.line, call.col, call.len);
                        }
                        cf.target = Some(target);
                        if let Some(imp) = self.single_impl(owner, &call.method) {
                            self.edge(caller, imp, EdgeKind::Calls, scope, call.line);
                        }
                        self.stats.resolved += 1;
                    }
                    None => match self.field_type(t, &call.method) {
                        Some(Val::Inst(TypeRef::External(s))) => {
                            cf.receiver_type = Some(s);
                            self.stats.external += 1;
                        }
                        _ if self.has_external_super(t) => self.stats.external += 1,
                        _ => self.stats.unresolved += 1,
                    },
                }
            }
            Some(Val::Inst(TypeRef::External(s))) => {
                cf.receiver_type = Some(s);
                self.stats.external += 1;
            }
            Some(Val::Ns(f)) => match self.symbol(f, &call.method, 0) {
                Some(Sym::Func(f, n)) => {
                    let target = self.file_funcs[f][&n].node;
                    self.edge(caller, target, EdgeKind::Calls, scope, call.line);
                    if clear {
                        self.b.add_ref(ref_from, target, call.line, call.col, call.len);
                    }
                    cf.target = Some(target);
                    self.stats.resolved += 1;
                }
                Some(Sym::External(_)) => self.stats.external += 1,
                _ => self.stats.unresolved += 1,
            },
            _ => self.stats.unresolved += 1,
        }
        cf
    }
}

fn exported(tf: &TsFile, name: &str) -> Visibility {
    if name == "default" || tf.default_export.as_deref() == Some(name) || tf.exports.iter().any(|(local, _)| local == name) {
        Visibility::Public
    } else {
        Visibility::Package
    }
}
