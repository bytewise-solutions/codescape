use codescape_build::{default_excludes, discover};
use codescape_facts::{ExtractStats, FileFacts, ProjectFacts};
use codescape_format::*;
use codescape_rust::extract_rust;
use std::path::Path;

struct Run {
    _tmp: tempfile::TempDir,
    b: GraphBuilder,
    facts: ProjectFacts,
    stats: ExtractStats,
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

const CORE_LIB: &str = r#"
pub mod model;
mod store;
pub use store::Store;

pub mod util {
    pub fn helper() -> u32 { 1 }
}

pub fn top() {
    util::helper();
    mystery();
    println!("{}", util::helper());
}

pub fn run<T: model::Repo>(r: &T) {
    r.find(1);
}

pub fn run2(r: impl model::Repo) {
    r.count();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it() {
        top();
    }
}
"#;

const MODEL: &str = r#"
#[derive(Debug, Clone)]
pub struct User {
    pub name: String,
}

pub enum Kind { A, B(u32) }

pub union Bits { a: u32 }

pub type Users = Vec<User>;

pub trait Repo {
    fn find(&self, id: u32) -> Option<User>;
    fn count(&self) -> usize { 0 }
}
"#;

const STORE: &str = r#"
pub(crate) mod mem;

use crate::model::{User, Repo as R};

pub struct Store {
    repo: Box<dyn R>,
    cache: mem::Cache,
}

impl Store {
    pub fn new(repo: Box<dyn R>) -> Self {
        Store { repo, cache: mem::Cache::default() }
    }

    pub fn get(&self, id: u32) -> Option<User> {
        self.cache.peek();
        self.repo.find(id)
    }
}
"#;

const MEM: &str = r#"
use super::super::model::*;

#[derive(Default)]
pub struct Cache;

impl Cache {
    pub fn peek(&self) {}
}

pub struct MemRepo;

impl Repo for MemRepo {
    fn find(&self, _id: u32) -> Option<User> { None }
}

impl std::fmt::Display for MemRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { Ok(()) }
}

impl crate::store::Store {
    pub fn clear(&mut self) {
        self.cache.peek();
    }
}

impl serde_like::Ser for Cache {
    fn ser(&self) {}
}
"#;

const APP_MAIN: &str = r#"
use corelib::{Store, model::User};

mod cli;

#[tokio::main]
async fn main() {
    let s = Store::new(cli::repo());
    s.get(1);
    corelib::top();
    let u = User { name: String::new() };
}
"#;

const APP_CLI: &str = r#"
use corelib::model::Repo;

pub fn repo() -> Box<dyn Repo> {
    todo!()
}
"#;

fn run() -> Run {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    write(r, "Cargo.toml", "[workspace]\nmembers = [\"core\", \"app\"]\n");
    write(r, "core/Cargo.toml", "[package]\nname = \"core-lib\"\n\n[dependencies]\nserde-like = \"1\"\n");
    write(r, "core/src/lib.rs", CORE_LIB);
    write(r, "core/src/model.rs", MODEL);
    write(r, "core/src/store/mod.rs", STORE);
    write(r, "core/src/store/mem.rs", MEM);
    write(r, "core/src/orphan.rs", "pub fn lost() {}\n");
    write(r, "app/Cargo.toml", "[package]\nname = \"app\"\n\n[dependencies]\ncorelib = { package = \"core-lib\", path = \"../core\" }\n");
    write(r, "app/src/main.rs", APP_MAIN);
    write(r, "app/src/cli.rs", APP_CLI);
    write(r, "app/src/bin/tool.rs", "fn main() { corelib::top(); }\n");
    write(r, "app/examples/demo.rs", "fn main() {}\n");
    write(r, "app/tests/it.rs", "#[test]\nfn works() { corelib::top(); }\n");
    let d = discover(r, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("fixture");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    let stats = extract_rust(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    Run { _tmp: tmp, b, facts, stats }
}

fn at(b: &GraphBuilder, path: &[&str]) -> NodeId {
    let mut n = b.root();
    for p in path {
        n = b.child(n, p).unwrap_or_else(|| panic!("missing {p} in {path:?}; children: {:?}", b.children(n).iter().map(|&c| b.name(c)).collect::<Vec<_>>()));
    }
    n
}

fn edge(b: &GraphBuilder, from: NodeId, to: NodeId, kind: EdgeKind) -> Option<RawEdge> {
    b.edges().iter().find(|e| e.from == from && e.to == to && e.kind == kind).copied()
}

fn children(b: &GraphBuilder, n: NodeId) -> Vec<&str> {
    let mut v: Vec<&str> = b.children(n).iter().map(|&c| b.name(c)).collect();
    v.sort();
    v
}

fn file<'a>(r: &'a Run, rel: &str) -> &'a FileFacts {
    r.facts.files.iter().find(|f| f.rel_path == rel).unwrap_or_else(|| panic!("no facts for {rel}"))
}

#[test]
fn packages_follow_the_module_tree() {
    let r = run();
    assert_eq!(children(&r.b, at(&r.b, &["core-lib"])), ["(lib)", "model", "orphan", "store", "store::mem", "tests", "util"]);
    assert_eq!(children(&r.b, at(&r.b, &["app"])), ["app", "app::cli", "bin/tool", "examples/demo", "tests/it"]);
    assert!(r.b.node_kind(at(&r.b, &["core-lib", "store::mem"])) == NodeKind::Package);
    assert_eq!(file(&r, "core/src/store/mem.rs").package, Some(at(&r.b, &["core-lib", "store::mem"])));
    assert_eq!(r.stats.files, 10);
}

#[test]
fn types_and_their_kinds() {
    let r = run();
    let kind = |path: &[&str]| r.b.sub_kind(at(&r.b, path));
    assert_eq!(kind(&["core-lib", "model", "User"]), SubKind::Struct);
    assert_eq!(kind(&["core-lib", "model", "Kind"]), SubKind::Enum);
    assert_eq!(kind(&["core-lib", "model", "Bits"]), SubKind::Struct);
    assert_eq!(kind(&["core-lib", "model", "Users"]), SubKind::NamedType);
    assert_eq!(kind(&["core-lib", "model", "Repo"]), SubKind::Interface);
    assert_eq!(children(&r.b, at(&r.b, &["core-lib", "model", "Repo"])), ["count", "find"]);
    assert_eq!(kind(&["core-lib", "util", "helper"]), SubKind::Function);
    assert_eq!(kind(&["core-lib", "orphan", "lost"]), SubKind::Function);
}

#[test]
fn impl_methods_attach_to_their_type_across_files() {
    let r = run();
    let store = at(&r.b, &["core-lib", "store", "Store"]);
    let methods: Vec<&str> =
        children(&r.b, store).into_iter().filter(|n| r.b.sub_kind(at(&r.b, &["core-lib", "store", "Store", n])) != SubKind::Field).collect();
    assert_eq!(methods, ["clear", "get", "new"]);
    assert_eq!(r.b.sub_kind(at(&r.b, &["core-lib", "store", "Store", "new"])), SubKind::Constructor);
    assert_eq!(r.b.sub_kind(at(&r.b, &["core-lib", "store", "Store", "get"])), SubKind::Method);
    assert_eq!(r.b.file(at(&r.b, &["core-lib", "store", "Store", "clear"])), Some("core/src/store/mem.rs"));
    assert_eq!(children(&r.b, at(&r.b, &["core-lib", "store::mem", "MemRepo"])), ["find", "fmt"]);
    let get = file(&r, "core/src/store/mod.rs").functions.iter().find(|f| f.name == "get").unwrap();
    assert_eq!(get.owner, Some(store));
}

#[test]
fn trait_impls_become_implements_edges_without_std_traits() {
    let r = run();
    let mem = at(&r.b, &["core-lib", "store::mem", "MemRepo"]);
    let repo = at(&r.b, &["core-lib", "model", "Repo"]);
    assert!(edge(&r.b, mem, repo, EdgeKind::Implements).is_some());
    assert_eq!(r.b.edges().iter().filter(|e| e.from == mem && e.kind == EdgeKind::Implements).count(), 1);
    let cache = at(&r.b, &["core-lib", "store::mem", "Cache"]);
    let ser = at(&r.b, &["(external)", "serde_like.Ser"]);
    assert!(edge(&r.b, cache, ser, EdgeKind::Implements).is_some());
    let mem_facts = file(&r, "core/src/store/mem.rs");
    let t = mem_facts.types.iter().find(|t| t.node == mem).unwrap();
    assert_eq!(t.fqn, "core_lib.store.mem.MemRepo");
    assert_eq!(t.supertypes, ["core_lib.model.Repo", "std.fmt.Display"]);
}

#[test]
fn calls_resolve_through_paths_uses_and_field_types() {
    let r = run();
    let n = |p: &[&str]| at(&r.b, p);
    let top = n(&["core-lib", "(lib)", "top"]);
    assert!(edge(&r.b, top, n(&["core-lib", "util", "helper"]), EdgeKind::Calls).is_some());
    let get = n(&["core-lib", "store", "Store", "get"]);
    assert!(edge(&r.b, get, n(&["core-lib", "store::mem", "Cache", "peek"]), EdgeKind::Calls).is_some());
    assert!(edge(&r.b, get, n(&["core-lib", "model", "Repo", "find"]), EdgeKind::Calls).is_some());
    assert!(edge(&r.b, get, n(&["core-lib", "store::mem", "MemRepo", "find"]), EdgeKind::Calls).is_some(), "single implementation");
    let new = n(&["core-lib", "store", "Store", "new"]);
    assert!(edge(&r.b, new, n(&["core-lib", "store", "Store"]), EdgeKind::Instantiates).is_some());
    let clear = n(&["core-lib", "store", "Store", "clear"]);
    assert!(edge(&r.b, clear, n(&["core-lib", "store::mem", "Cache", "peek"]), EdgeKind::Calls).is_some());
    let run_ = n(&["core-lib", "(lib)", "run"]);
    assert!(edge(&r.b, run_, n(&["core-lib", "model", "Repo", "find"]), EdgeKind::Calls).is_some());
    let run2 = n(&["core-lib", "(lib)", "run2"]);
    assert!(edge(&r.b, run2, n(&["core-lib", "model", "Repo", "count"]), EdgeKind::Calls).is_some());
}

#[test]
fn cross_crate_calls_use_the_renamed_extern_name() {
    let r = run();
    let n = |p: &[&str]| at(&r.b, p);
    let main = n(&["app", "app", "main"]);
    assert!(edge(&r.b, main, n(&["core-lib", "store", "Store", "new"]), EdgeKind::Calls).is_some());
    assert!(edge(&r.b, main, n(&["core-lib", "store", "Store", "get"]), EdgeKind::Calls).is_some(), "s = Store::new(..); s.get()");
    assert!(edge(&r.b, main, n(&["core-lib", "(lib)", "top"]), EdgeKind::Calls).is_some());
    assert!(edge(&r.b, main, n(&["app", "app::cli", "repo"]), EdgeKind::Calls).is_some());
    assert!(edge(&r.b, main, n(&["core-lib", "model", "User"]), EdgeKind::Instantiates).is_some());
    assert!(edge(&r.b, n(&["app", "bin/tool", "main"]), n(&["core-lib", "(lib)", "top"]), EdgeKind::Calls).is_some());
    assert!(edge(&r.b, n(&["app"]), n(&["core-lib"]), EdgeKind::DependsOn).is_some());
    let pkg_edge = edge(&r.b, n(&["app", "app"]), n(&["core-lib", "store"]), EdgeKind::DependsOn).expect("use corelib::Store, re-exported from store");
    assert!(!pkg_edge.flags.contains(EdgeFlags::UNDECLARED));
}

#[test]
fn macro_bodies_are_not_counted_and_unknown_calls_are_unresolved() {
    let r = run();
    let top = at(&r.b, &["core-lib", "(lib)", "top"]);
    let calls: Vec<&str> = r.facts.files.iter().flat_map(|f| &f.calls).filter(|c| c.caller == top).map(|c| c.method.as_str()).collect();
    assert_eq!(calls, ["helper", "mystery"]);
    assert!(r.stats.unresolved >= 1);
    assert!(r.stats.resolved >= 10);
}

#[test]
fn test_code_is_flagged() {
    let r = run();
    let n = |p: &[&str]| at(&r.b, p);
    let it = n(&["core-lib", "tests", "it"]);
    assert!(r.b.flags(it).contains(NodeFlags::TEST));
    assert!(r.b.flags(n(&["core-lib", "tests"])).contains(NodeFlags::TEST));
    assert!(!r.b.flags(n(&["core-lib", "(lib)", "top"])).contains(NodeFlags::TEST));
    let e = edge(&r.b, it, n(&["core-lib", "(lib)", "top"]), EdgeKind::Calls).unwrap();
    assert_eq!(e.scope, Scope::Test);
    assert!(file(&r, "app/tests/it.rs").is_test);
    assert!(r.b.flags(n(&["app", "tests/it", "works"])).contains(NodeFlags::TEST));
    assert!(file(&r, "app/examples/demo.rs").is_dev);
    assert!(!file(&r, "app/src/main.rs").is_dev && !file(&r, "app/src/main.rs").is_test);
}

#[test]
fn attributes_are_annotations_and_mains_are_marked() {
    let r = run();
    let model = file(&r, "core/src/model.rs");
    let user = model.types.iter().find(|t| t.fqn == "core_lib.model.User").unwrap();
    assert_eq!(user.annotations[0].name, "derive");
    assert_eq!(user.annotations[0].args, ["Debug", "Clone"]);
    let lib = file(&r, "core/src/lib.rs");
    let it = lib.functions.iter().find(|f| f.name == "it").unwrap();
    assert_eq!(it.annotations[0].name, "test");
    let main = file(&r, "app/src/main.rs").functions.iter().find(|f| f.name == "main").unwrap();
    assert!(main.is_main);
    assert_eq!(main.annotations[0].name, "tokio::main");
    assert!(file(&r, "app/src/bin/tool.rs").functions[0].is_main);
    assert!(file(&r, "app/examples/demo.rs").functions[0].is_main);
    assert!(!lib.functions.iter().any(|f| f.is_main));
}

#[test]
fn external_trait_methods_are_marked_as_overrides() {
    let r = run();
    let mem = file(&r, "core/src/store/mem.rs");
    let ser = mem.functions.iter().find(|f| f.name == "ser").unwrap();
    assert_eq!(ser.overrides_external.as_deref(), Some("serde_like.Ser"));
    let fmt = mem.functions.iter().find(|f| f.name == "fmt").unwrap();
    assert_eq!(fmt.overrides_external, None);
}

const PATHS_LIB: &str = r#"
#[path = "weird_name.rs"]
mod renamed;

mod outer {
    pub mod inner;
}

mod shapes;
pub use shapes::*;
mod shape_impls;

pub fn area_of(s: &Square) -> u32 {
    s.area()
}

use self::outer::inner::{self, Engine as E};
use std::sync::Arc;

macro_rules! define {
    ($n:ident) => {
        pub struct $n;
    };
}

define!(Generated);

impl Generated {
    pub fn tick(&self) {}
}

pub fn use_generated(g: &Generated) {
    g.tick();
}

pub struct Holder {
    engine: Arc<E>,
}

impl Holder {
    pub fn try_new() -> Result<Self, String> {
        Ok(Holder { engine: Arc::new(E::build()) })
    }

    pub fn make() -> Holder {
        Self::try_new().unwrap()
    }

    pub fn go(&self) {
        self.engine.start();
        Self::make();
        inner::free();
        renamed::hidden();
        E::build().start();
        let e: E = E::build();
        e.stop();
        let h = Holder::make();
        h.go();
    }
}
"#;

const INNER: &str = r#"
pub struct Engine;

impl Engine {
    pub fn build() -> Engine { Engine }
    pub fn start(&self) { super::super::renamed::hidden(); }
    pub fn stop(&self) {}
}

pub fn free() {}
"#;

fn run_paths() -> Run {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    write(r, "Cargo.toml", "[package]\nname = \"paths\"\n");
    write(r, "src/lib.rs", PATHS_LIB);
    write(r, "src/weird_name.rs", "pub fn hidden() {}\n");
    write(r, "src/outer/inner.rs", INNER);
    write(r, "src/shapes.rs", "macro_rules! def { ($n:ident) => { pub struct $n; } }\ndef!(Square);\n");
    write(r, "src/shape_impls.rs", "use crate::*;\nimpl Square { pub fn area(&self) -> u32 { 4 } }\n");
    let d = discover(r, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("paths");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    let stats = extract_rust(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    Run { _tmp: tmp, b, facts, stats }
}

#[test]
fn path_attributes_and_modules_nested_in_inline_blocks() {
    let r = run_paths();
    assert_eq!(children(&r.b, at(&r.b, &["paths"])), ["(lib)", "outer", "outer::inner", "renamed", "shape_impls", "shapes"]);
    assert_eq!(r.b.file(at(&r.b, &["paths", "renamed", "hidden"])), Some("src/weird_name.rs"));
    assert_eq!(r.b.file(at(&r.b, &["paths", "outer::inner", "Engine"])), Some("src/outer/inner.rs"));
}

#[test]
fn constructors_return_self_or_their_type() {
    let r = run_paths();
    let kind = |p: &[&str]| r.b.sub_kind(at(&r.b, p));
    assert_eq!(kind(&["paths", "(lib)", "Holder", "try_new"]), SubKind::Constructor);
    assert_eq!(kind(&["paths", "(lib)", "Holder", "make"]), SubKind::Constructor);
    assert_eq!(kind(&["paths", "(lib)", "Holder", "go"]), SubKind::Method);
    assert_eq!(kind(&["paths", "outer::inner", "Engine", "build"]), SubKind::Constructor);
}

#[test]
fn self_paths_aliases_wrappers_and_call_results() {
    let r = run_paths();
    let n = |p: &[&str]| at(&r.b, p);
    let go = n(&["paths", "(lib)", "Holder", "go"]);
    let calls = |to: &[&str]| edge(&r.b, go, n(to), EdgeKind::Calls).is_some_and(|e| e.confidence == Confidence::Resolved);
    assert!(calls(&["paths", "outer::inner", "Engine", "start"]), "self.engine: Arc<E>");
    assert!(calls(&["paths", "(lib)", "Holder", "make"]), "Self::make");
    assert!(calls(&["paths", "outer::inner", "free"]), "use self::outer::inner::{{self}}");
    assert!(calls(&["paths", "renamed", "hidden"]));
    assert!(calls(&["paths", "outer::inner", "Engine", "build"]));
    assert!(calls(&["paths", "outer::inner", "Engine", "stop"]), "let e: E");
    assert!(calls(&["paths", "(lib)", "Holder", "go"]), "let h = Holder::make(); h.go()");
    let make = n(&["paths", "(lib)", "Holder", "make"]);
    assert!(edge(&r.b, make, n(&["paths", "(lib)", "Holder", "try_new"]), EdgeKind::Calls).is_some());
    let start = n(&["paths", "outer::inner", "Engine", "start"]);
    assert!(edge(&r.b, start, n(&["paths", "renamed", "hidden"]), EdgeKind::Calls).is_some(), "super::super");
    let try_new = n(&["paths", "(lib)", "Holder", "try_new"]);
    assert!(edge(&r.b, try_new, n(&["paths", "(lib)", "Holder"]), EdgeKind::Instantiates).is_some());
    assert_eq!(r.stats.unresolved, 0, "{:?}", r.stats);
    assert_eq!(r.stats.guessed, 0);
}

#[test]
fn inherent_impls_of_macro_generated_types_still_get_a_type() {
    let r = run_paths();
    let g = at(&r.b, &["paths", "(lib)", "Generated"]);
    assert_eq!(r.b.node_kind(g), NodeKind::Type);
    assert_eq!(children(&r.b, g), ["tick"]);
    let user = at(&r.b, &["paths", "(lib)", "use_generated"]);
    assert!(edge(&r.b, user, at(&r.b, &["paths", "(lib)", "Generated", "tick"]), EdgeKind::Calls).is_some());
}

#[test]
fn macro_generated_types_resolve_through_glob_reexports() {
    let r = run_paths();
    let area_of = at(&r.b, &["paths", "(lib)", "area_of"]);
    let e = edge(&r.b, area_of, at(&r.b, &["paths", "shape_impls", "Square", "area"]), EdgeKind::Calls).unwrap();
    assert_eq!(e.confidence, Confidence::Resolved);
}

#[test]
fn unknown_receivers_guess_only_into_imported_crates() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    write(r, "Cargo.toml", "[workspace]\nmembers = [\"a\", \"b\"]\n");
    write(r, "a/Cargo.toml", "[package]\nname = \"a\"\n");
    write(r, "a/src/lib.rs", "pub struct Thing;\nimpl Thing { pub fn unique_name(&self) {} }\n");
    write(r, "b/Cargo.toml", "[package]\nname = \"b\"\n\n[dependencies]\na = { path = \"../a\" }\n");
    write(
        r,
        "b/src/lib.rs",
        "mod other;\npub fn elim(x: Unknown) { x.never_defined_anywhere(); }\npub fn h(items: Vec<u8>) { for t in items { t.unique_name(); } }\n",
    );
    write(
        r,
        "b/src/other.rs",
        "use a::Thing;\npub fn k(xs: Vec<Thing>) { for t in xs { t.unique_name(); } }\npub fn o() { std::env::args().unique_name(); }\n",
    );
    let d = discover(r, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("guess");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    let stats = extract_rust(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    assert_eq!(
        (stats.resolved, stats.guessed, stats.unresolved),
        (4, 0, 1),
        "only the call on an unknown type is unresolved: nothing is resolved by elimination"
    );
    let target = at(&b, &["a", "(lib)", "Thing", "unique_name"]);
    let callers: Vec<&str> = b.edges().iter().filter(|e| e.to == target).map(|e| b.name(e.from)).collect();
    assert_eq!(callers, ["k"], "u8 elements have no unique_name, so h is not guessed into a");
    assert_eq!(b.edges().iter().find(|e| e.to == target).unwrap().confidence, Confidence::Resolved, "Vec<Thing> elements are Things");
}

#[test]
fn pub_modifiers_become_visibility() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    write(r, "Cargo.toml", "[package]\nname = \"vis\"\n");
    write(
        r,
        "src/lib.rs",
        "pub struct Store;\nstruct Cache;\npub trait Port { fn run(&self); }\nimpl Store {\n    pub fn get(&self) {}\n    pub(crate) fn load(&self) {}\n    fn evict(&self) {}\n}\nimpl Port for Store { fn run(&self) {} }\npub fn open() {}\nfn helper() {}\n",
    );
    let d = discover(r, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("vis");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    extract_rust(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    let vis = |path: &[&str]| b.visibility(at(&b, path));
    assert_eq!(vis(&["vis", "(lib)", "Store"]), Visibility::Public);
    assert_eq!(vis(&["vis", "(lib)", "Cache"]), Visibility::Private);
    assert_eq!(vis(&["vis", "(lib)", "Store", "get"]), Visibility::Public);
    assert_eq!(vis(&["vis", "(lib)", "Store", "load"]), Visibility::Package);
    assert_eq!(vis(&["vis", "(lib)", "Store", "evict"]), Visibility::Private);
    assert_eq!(vis(&["vis", "(lib)", "Store", "run"]), Visibility::Public, "trait methods are as visible as the trait");
    assert_eq!(vis(&["vis", "(lib)", "Port", "run"]), Visibility::Public);
    assert_eq!(vis(&["vis", "(lib)", "open"]), Visibility::Public);
    assert_eq!(vis(&["vis", "(lib)", "helper"]), Visibility::Private);
}

#[test]
fn struct_fields_become_members_after_methods() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    write(r, "Cargo.toml", "[package]\nname = \"fld\"\n");
    write(
        r,
        "src/lib.rs",
        "pub struct Buf {\n    pub len: usize,\n    pub(crate) data: Vec<u8>,\n    cap: usize,\n}\nimpl Buf { pub fn len(&self) -> usize { self.len } }\npub struct Pair(pub u8, String);\n",
    );
    let d = discover(r, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("fld");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    extract_rust(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    let fields = |t: NodeId| -> Vec<(String, Option<String>, Visibility)> {
        b.children(t)
            .iter()
            .filter(|&&c| b.sub_kind(c) == SubKind::Field)
            .map(|&c| (b.name(c).to_string(), b.detail(c).map(String::from), b.visibility(c)))
            .collect()
    };
    let buf = at(&b, &["fld", "(lib)", "Buf"]);
    assert_eq!(
        fields(buf),
        [
            ("len".to_string(), Some("usize".to_string()), Visibility::Public),
            ("data".to_string(), Some("Vec<u8>".to_string()), Visibility::Package),
            ("cap".to_string(), Some("usize".to_string()), Visibility::Private)
        ]
    );
    assert_eq!(b.sub_kind(b.child(buf, "len").unwrap()), SubKind::Method, "the method keeps the name lookup");
    let pair = at(&b, &["fld", "(lib)", "Pair"]);
    assert_eq!(
        fields(pair),
        [("0".to_string(), Some("u8".to_string()), Visibility::Public), ("1".to_string(), Some("String".to_string()), Visibility::Private)]
    );
}

#[test]
fn functions_carry_their_cyclomatic_complexity() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    write(r, "Cargo.toml", "[package]\nname = \"cx\"\n");
    write(
        r,
        "src/lib.rs",
        "pub fn easy() -> u8 { 1 }\npub fn hard(x: u8, a: bool, b: bool) -> u8 {\n    if x > 0 && a { return 1; }\n    for _ in 0..x { if b || a { continue; } }\n    match x { 1 => 2, 2 => 3, _ => 4 }\n}\n",
    );
    let d = discover(r, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("cx");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    extract_rust(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    assert_eq!(b.complexity(at(&b, &["cx", "(lib)", "easy"])), 1);
    assert_eq!(b.complexity(at(&b, &["cx", "(lib)", "hard"])), 8, "if && for if || and two extra match arms");
}

#[test]
fn derive_generated_types_resolve_from_another_crate() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    write(r, "Cargo.toml", "[workspace]\nmembers = [\"fmt\", \"app\"]\n");
    write(r, "fmt/Cargo.toml", "[package]\nname = \"fmt\"\n");
    write(r, "fmt/src/lib.rs", "mod model;\nmod file;\npub use model::*;\n");
    write(r, "fmt/src/model.rs", "#[derive(Archive)]\npub struct Graph;\n");
    write(r, "fmt/src/file.rs", "use crate::model::*;\nimpl ArchivedGraph {\n    pub fn path(&self) -> u32 { 1 }\n}\n");
    write(r, "app/Cargo.toml", "[package]\nname = \"app\"\n\n[dependencies]\nfmt = { path = \"../fmt\" }\n");
    write(r, "app/src/lib.rs", "use fmt::ArchivedGraph;\npub fn f(g: &ArchivedGraph) -> u32 {\n    g.path()\n}\n");
    let d = discover(r, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("derive");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    let stats = extract_rust(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    let f = at(&b, &["app", "(lib)", "f"]);
    let path = at(&b, &["fmt", "file", "ArchivedGraph", "path"]);
    assert!(edge(&b, f, path, EdgeKind::Calls).is_some_and(|e| e.confidence == Confidence::Resolved), "{stats:?}");
}

const BAG: &str = r#"
pub struct Item {
    pub name: String,
}

impl Item {
    pub fn weight(&self) -> u32 { 1 }
}

pub struct Bag {
    pub items: Vec<Item>,
}

impl Bag {
    pub fn open(p: &str) -> Result<Bag, String> {
        Ok(Bag { items: Vec::new() })
    }

    pub fn looped(&self) -> u32 {
        let mut t = 0;
        for i in &self.items {
            t += i.weight();
        }
        t
    }

    pub fn mapped(&self) -> u32 {
        self.items.iter().filter(|i| i.weight() > 0).map(|i| i.weight()).sum()
    }

    pub fn picked(&self) -> u32 {
        let first = self.items.first().unwrap();
        first.weight() + self.items.get(1).map(|i| i.weight()).unwrap_or(0)
    }

    pub fn shadowed(bag: &Bag) -> u32 {
        let b = Bag::open("x").unwrap();
        let n = bag.items.iter().filter(|b| b.weight() > 0).count() as u32;
        b.looped() + n
    }

    pub fn load() -> Result<u32, String> {
        let b = Bag::open("x").map_err(|e| e.to_string())?;
        Ok(b.looped())
    }
}
"#;

#[test]
fn loop_and_closure_variables_take_the_element_type_and_adapters_keep_it() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    write(r, "Cargo.toml", "[package]\nname = \"bag\"\n");
    write(r, "src/lib.rs", BAG);
    let d = discover(r, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("bag");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    extract_rust(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    let n = |p: &[&str]| at(&b, p);
    let weight = n(&["bag", "(lib)", "Item", "weight"]);
    for caller in ["looped", "mapped", "picked"] {
        let e = edge(&b, n(&["bag", "(lib)", "Bag", caller]), weight, EdgeKind::Calls);
        assert!(e.is_some_and(|e| e.confidence == Confidence::Resolved), "{caller} calls Item::weight");
    }
    let shadowed = n(&["bag", "(lib)", "Bag", "shadowed"]);
    assert!(edge(&b, shadowed, weight, EdgeKind::Calls).is_some(), "the closure's b is an Item");
    assert!(edge(&b, shadowed, n(&["bag", "(lib)", "Bag", "looped"]), EdgeKind::Calls).is_some(), "after the closure, b is the Bag again");
    let load = n(&["bag", "(lib)", "Bag", "load"]);
    assert!(
        edge(&b, load, n(&["bag", "(lib)", "Bag", "looped"]), EdgeKind::Calls).is_some_and(|e| e.confidence == Confidence::Resolved),
        "map_err(...)? keeps Bag"
    );
}

#[test]
fn integration_tests_resolve_calls_into_their_crate_dependencies() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    write(r, "Cargo.toml", "[workspace]\nmembers = [\"fmt\", \"app\"]\n\n[workspace.dependencies]\nmy-fmt = { path = \"fmt\" }\n");
    write(r, "fmt/Cargo.toml", "[package]\nname = \"my-fmt\"\n");
    write(r, "fmt/src/lib.rs", "mod builder;\npub use builder::{GraphBuilder, Kind};\n");
    write(
        r,
        "fmt/src/builder.rs",
        "pub enum Kind { A }\npub struct GraphBuilder;\nimpl GraphBuilder {\n    pub fn new(name: &str) -> GraphBuilder { GraphBuilder }\n    pub fn root(&self) -> u32 { 0 }\n    pub fn add_node(&mut self, parent: u32, kind: Kind) -> u32 { parent }\n}\n",
    );
    write(r, "app/Cargo.toml", "[package]\nname = \"app\"\n\n[dependencies]\nmy-fmt.workspace = true\n");
    write(r, "app/src/lib.rs", "pub fn x() {}\n");
    write(r, "app/tests/all.rs", "mod scene;\n");
    write(
        r,
        "app/tests/scene.rs",
        "use my_fmt::*;\n\n#[test]\nfn builds() {\n    let mut b = GraphBuilder::new(\"p\");\n    let m = b.add_node(b.root(), Kind::A);\n}\n",
    );
    let d = discover(r, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("it");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    let stats = extract_rust(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    let targets: Vec<&str> = b.edges().iter().filter(|e| e.kind == EdgeKind::Calls && b.name(e.from) == "builds").map(|e| b.name(e.to)).collect();
    assert!(targets.contains(&"add_node") && targets.contains(&"root"), "{targets:?} {stats:?}");
}
