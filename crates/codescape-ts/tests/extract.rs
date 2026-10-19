use codescape_build::{default_excludes, discover};
use codescape_facts::{ExtractStats, FileFacts, ProjectFacts};
use codescape_format::*;
use codescape_ts::extract_ts;
use std::path::Path;

struct Run {
    b: GraphBuilder,
    facts: ProjectFacts,
    stats: ExtractStats,
}

fn run() -> Run {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/ts-resolve");
    let d = discover(&root, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("ts-resolve");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    let stats = extract_ts(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    Run { b, facts, stats }
}

fn at(b: &GraphBuilder, path: &[&str]) -> NodeId {
    let mut n = b.root();
    for p in path {
        n = b.child(n, p).unwrap_or_else(|| panic!("missing {p} in {path:?}: {:?}", b.children(n).iter().map(|&c| b.name(c)).collect::<Vec<_>>()));
    }
    n
}

fn edge(b: &GraphBuilder, from: NodeId, to: NodeId, kind: EdgeKind) -> Option<RawEdge> {
    b.edges().iter().find(|e| e.from == from && e.to == to && e.kind == kind).copied()
}

fn file<'a>(r: &'a Run, suffix: &str) -> &'a FileFacts {
    r.facts.files.iter().find(|f| f.rel_path.ends_with(suffix)).unwrap_or_else(|| panic!("no file {suffix}"))
}

const USERS: [&str; 2] = ["web", "src/app/users"];

fn users(name: &str) -> Vec<&str> {
    let mut v = USERS.to_vec();
    v.push(name);
    v
}

#[test]
fn packages_are_directories_and_members_live_in_their_types() {
    let r = run();
    assert_eq!(r.b.node_kind(at(&r.b, &USERS)), NodeKind::Package);
    let svc = at(&r.b, &users("UserService"));
    assert_eq!(r.b.node_kind(svc), NodeKind::Type);
    assert_eq!(r.b.sub_kind(svc), SubKind::Class);
    for m in ["load", "find"] {
        assert_eq!(r.b.sub_kind(at(&r.b, &[USERS[0], USERS[1], "UserService", m])), SubKind::Method);
    }
    assert_eq!(r.b.sub_kind(at(&r.b, &["web", "src/app", "AppComponent", "constructor"])), SubKind::Constructor);
    assert_eq!(r.b.sub_kind(at(&r.b, &["@demo/shared", "src", "Store"])), SubKind::Interface);
    assert_eq!(r.b.sub_kind(at(&r.b, &["web", "src/app/util", "format2"])), SubKind::Function);
    assert_eq!(r.b.sub_kind(at(&r.b, &["web", "src", "main.ts"])), SubKind::Function, "top-level code is a member named after its file");
    assert_eq!(r.b.lang(svc), Lang::TypeScript);
    assert_eq!(r.b.lang(at(&r.b, &["server", "src", "list"])), Lang::JavaScript);
}

#[test]
fn build_output_declarations_and_tooling_are_handled() {
    let r = run();
    assert!(!r.facts.files.iter().any(|f| f.rel_path.contains("/dist/") || f.rel_path.ends_with(".d.ts")));
    assert!(file(&r, "vite.config.ts").is_dev);
    let spec = file(&r, "user.service.spec.ts");
    assert!(spec.is_test);
    let spec_fn = spec.functions[0].node;
    assert!(r.b.flags(spec_fn).contains(NodeFlags::TEST));
    let mock = file(&r, "__mocks__/user.ts");
    assert!(r.b.flags(mock.types[0].node).contains(NodeFlags::MOCK));
    assert_eq!(r.stats.files as usize, r.facts.files.len());
}

#[test]
fn inheritance_and_implementations() {
    let r = run();
    let svc = at(&r.b, &users("UserService"));
    let base = at(&r.b, &users("BaseService"));
    assert!(edge(&r.b, svc, base, EdgeKind::Inherits).is_some());
    let mem = at(&r.b, &["@demo/shared", "src", "MemoryStore"]);
    let store = at(&r.b, &["@demo/shared", "src", "Store"]);
    assert!(edge(&r.b, mem, store, EdgeKind::Implements).is_some());
    let app = at(&r.b, &["web", "src/app", "AppComponent"]);
    let on_init = r.b.edges().iter().find(|e| e.from == app && e.kind == EdgeKind::Implements).map(|e| e.to).unwrap();
    assert_eq!(r.b.node_kind(on_init), NodeKind::External);
    assert_eq!(r.b.name(on_init), "@angular/core.OnInit");
    let facts = file(&r, "user.service.ts");
    let t = &facts.types[0];
    assert!(t.fqn.ends_with("user.service.UserService"), "{}", t.fqn);
    assert_eq!(t.annotations[0].name, "Injectable");
    assert!(t.supertypes[0].ends_with("base.service.BaseService"));
}

#[test]
fn calls_through_constructor_properties_inject_fields_and_locals() {
    let r = run();
    let init = at(&r.b, &["web", "src/app", "AppComponent", "ngOnInit"]);
    let load = at(&r.b, &[USERS[0], USERS[1], "UserService", "load"]);
    assert!(edge(&r.b, init, load, EdgeKind::Calls).is_some(), "constructor(private users: UserService)");
    let refresh = at(&r.b, &[USERS[0], USERS[1], "UserListComponent", "refresh"]);
    let find = at(&r.b, &[USERS[0], USERS[1], "UserService", "find"]);
    assert!(edge(&r.b, refresh, find, EdgeKind::Calls).is_some(), "svc = inject(UserService)");
    let rename = at(&r.b, &[USERS[0], USERS[1], "User", "rename"]);
    assert!(edge(&r.b, refresh, rename, EdgeKind::Calls).is_some(), "const user = this.svc.find() is typed by find's return type");
    assert!(edge(&r.b, find, rename, EdgeKind::Calls).is_some(), "const u = new User()");
    let touch = at(&r.b, &[USERS[0], USERS[1], "User", "touch"]);
    assert!(edge(&r.b, refresh, touch, EdgeKind::Calls).is_some(), "this.svc.find().touch() follows the chain");
    let count = file(&r, "user-list.component.ts").calls.iter().find(|c| c.method == "count").unwrap();
    assert_eq!(count.receiver_type.as_deref(), Some("@angular/core.signal()"), "calling a signal field");
    assert!(edge(&r.b, find, at(&r.b, &users("User")), EdgeKind::Instantiates).is_some());
    let base = at(&r.b, &[USERS[0], USERS[1], "BaseService", "base"]);
    assert!(edge(&r.b, load, base, EdgeKind::Calls).is_some(), "this.base() through the superclass");
}

#[test]
fn calls_into_other_modules_barrels_aliases_and_namespaces() {
    let r = run();
    let load = at(&r.b, &[USERS[0], USERS[1], "UserService", "load"]);
    let log = at(&r.b, &["@demo/shared", "src", "log"]);
    assert!(edge(&r.b, load, log, EdgeKind::Calls).is_some(), "bare workspace import through an index barrel");
    let store_get = at(&r.b, &["@demo/shared", "src", "Store", "get"]);
    let mem_get = at(&r.b, &["@demo/shared", "src", "MemoryStore", "get"]);
    assert!(edge(&r.b, load, store_get, EdgeKind::Calls).is_some());
    assert!(edge(&r.b, load, mem_get, EdgeKind::Calls).is_some(), "single implementation of the interface");
    let refresh = at(&r.b, &[USERS[0], USERS[1], "UserListComponent", "refresh"]);
    let format = at(&r.b, &["web", "src/app/util", "Helpers", "format"]);
    assert!(edge(&r.b, refresh, format, EdgeKind::Calls).is_some(), "static call through a directory index");
    let format2 = at(&r.b, &["web", "src/app/util", "format2"]);
    assert!(edge(&r.b, refresh, format2, EdgeKind::Calls).is_some(), "namespace import");
    let web = at(&r.b, &["web"]);
    let shared = at(&r.b, &["@demo/shared"]);
    assert!(edge(&r.b, web, shared, EdgeKind::DependsOn).is_some());
    let pkg = at(&r.b, &USERS);
    let shared_src = at(&r.b, &["@demo/shared", "src"]);
    let dep = edge(&r.b, pkg, shared_src, EdgeKind::DependsOn).unwrap();
    assert!(!dep.flags.contains(EdgeFlags::UNDECLARED));
    let facts = file(&r, "users/user.service.ts");
    assert!(facts.import_targets.contains(&shared_src));
    assert!(facts.imports.iter().any(|i| i.path == "@angular/core"));
    let http = facts.calls.iter().find(|c| c.method == "get" && c.receiver_var.as_deref() == Some("this")).unwrap();
    assert_eq!(http.receiver_type.as_deref(), Some("@angular/common/http.HttpClient"));
    assert_eq!(http.string_args, [Some("/api/users".to_string())]);
}

#[test]
fn decorator_references_instantiate_project_types() {
    let r = run();
    let app = at(&r.b, &["web", "src/app", "AppComponent"]);
    let list = at(&r.b, &users("UserListComponent"));
    assert!(edge(&r.b, app, list, EdgeKind::Instantiates).is_some());
}

#[test]
fn package_main_top_level_code_is_main_and_handlers_resolve() {
    let r = run();
    let server = file(&r, "server/src/server.js");
    let top = server.functions.iter().find(|f| f.name == "server.js").unwrap();
    assert!(top.is_main);
    assert_eq!(server.lang, Lang::JavaScript);
    let get = server.calls.iter().find(|c| c.method == "get").unwrap();
    assert_eq!(get.receiver_type.as_deref(), Some("express()"));
    assert_eq!(get.receiver_var.as_deref(), Some("app"));
    assert_eq!(get.string_args, [Some("/users".to_string()), None]);
    let list = at(&r.b, &["server", "src", "list"]);
    assert_eq!(get.arg_targets.last().copied().flatten(), Some(list));
    let web_main = file(&r, "web/src/main.ts");
    assert!(!web_main.functions[0].is_main, "main.ts is not named by package.json");
}

#[test]
fn unresolvable_calls_are_counted_not_guessed() {
    let r = run();
    assert!(r.stats.resolved >= 10, "{:?}", r.stats);
    assert!(r.stats.unresolved >= 1, "{:?}", r.stats);
    assert_eq!(r.stats.guessed, 0);
    assert!(r.b.edges().iter().all(|e| e.confidence == Confidence::Resolved));
}

#[test]
fn class_members_and_exports_carry_their_visibility() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    std::fs::write(r.join("package.json"), "{\"name\": \"vis\"}").unwrap();
    std::fs::write(r.join("tsconfig.json"), "{}").unwrap();
    std::fs::create_dir_all(r.join("src")).unwrap();
    std::fs::write(
        r.join("src/store.ts"),
        "export class Store {\n  constructor(private db: string) {}\n  get(): void {}\n  protected load(): void {}\n  private evict(): void {}\n  #secret(): void {}\n}\nclass Cache {}\nexport function open() {}\nfunction helper() {}\n",
    )
    .unwrap();
    let d = discover(r, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("vis");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    extract_ts(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    let vis = |path: &[&str]| b.visibility(at(&b, path));
    assert_eq!(vis(&["vis", "src", "Store"]), Visibility::Public);
    assert_eq!(vis(&["vis", "src", "Cache"]), Visibility::Package, "not exported");
    for (m, v) in [
        ("constructor", Visibility::Public),
        ("get", Visibility::Public),
        ("load", Visibility::Protected),
        ("evict", Visibility::Private),
        ("#secret", Visibility::Private),
    ] {
        assert_eq!(vis(&["vis", "src", "Store", m]), v, "{m}");
    }
    assert_eq!(vis(&["vis", "src", "open"]), Visibility::Public);
    assert_eq!(vis(&["vis", "src", "helper"]), Visibility::Package);
}

#[test]
fn class_and_interface_fields_become_members() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    std::fs::write(r.join("package.json"), "{\"name\": \"fld\"}").unwrap();
    std::fs::write(r.join("tsconfig.json"), "{}").unwrap();
    std::fs::create_dir_all(r.join("src")).unwrap();
    std::fs::write(
        r.join("src/store.ts"),
        "export class Store {\n  private cache: Map<string, number> = new Map();\n  readonly name = 'x';\n  constructor(private db: Db, public log: Logger) {}\n}\nexport interface Config {\n  url: string;\n  retries?: number;\n}\n",
    )
    .unwrap();
    let d = discover(r, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("fld");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    extract_ts(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    let fields = |t: NodeId| -> Vec<(String, Option<String>, Visibility)> {
        b.children(t)
            .iter()
            .filter(|&&c| b.sub_kind(c) == SubKind::Field)
            .map(|&c| (b.name(c).to_string(), b.detail(c).map(String::from), b.visibility(c)))
            .collect()
    };
    let store = at(&b, &["fld", "src", "Store"]);
    assert_eq!(
        fields(store),
        [
            ("cache".to_string(), Some("Map<string, number>".to_string()), Visibility::Private),
            ("name".to_string(), None, Visibility::Public),
            ("db".to_string(), Some("Db".to_string()), Visibility::Private),
            ("log".to_string(), Some("Logger".to_string()), Visibility::Public),
        ]
    );
    let cfg = at(&b, &["fld", "src", "Config"]);
    assert_eq!(
        fields(cfg),
        [("url".to_string(), Some("string".to_string()), Visibility::Public), ("retries".to_string(), Some("number".to_string()), Visibility::Public)]
    );
}

#[test]
fn functions_and_methods_carry_their_cyclomatic_complexity() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    std::fs::write(r.join("package.json"), "{\"name\": \"cx\"}").unwrap();
    std::fs::write(r.join("tsconfig.json"), "{}").unwrap();
    std::fs::create_dir_all(r.join("src")).unwrap();
    std::fs::write(
        r.join("src/c.ts"),
        "export function easy() { return 1; }\nexport class C {\n  hard(x: number, a: boolean, b: boolean) {\n    if (x > 0 && a) { return 1; }\n    for (let i = 0; i < x; i++) { if (b || a) x--; }\n    switch (x) { case 1: return 2; case 2: return 3; default: return a ? 4 : (x ?? 5); }\n  }\n}\n",
    )
    .unwrap();
    let d = discover(r, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("cx");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    extract_ts(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    assert_eq!(b.complexity(at(&b, &["cx", "src", "easy"])), 1);
    assert_eq!(b.complexity(at(&b, &["cx", "src", "C", "hard"])), 10, "if && for if || case case ?: ??");
}

fn refs_from(b: &GraphBuilder, from: NodeId) -> Vec<(NodeId, u32, u32, u32)> {
    b.refs().iter().filter(|r| r.from == from).map(|r| (r.to, r.line, r.col as u32, r.len as u32)).collect()
}

fn project(files: &[(&str, &str)]) -> (GraphBuilder, ProjectFacts) {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    std::fs::write(r.join("package.json"), "{\"name\": \"u\"}").unwrap();
    std::fs::write(r.join("tsconfig.json"), "{}").unwrap();
    std::fs::create_dir_all(r.join("src")).unwrap();
    for (name, src) in files {
        std::fs::write(r.join("src").join(name), src).unwrap();
    }
    let d = discover(r, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("u");
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = ProjectFacts::default();
    extract_ts(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    (b, facts)
}

#[test]
fn call_and_type_use_positions_are_recorded() {
    let r = run();
    let user = at(&r.b, &users("User"));
    let rename = at(&r.b, &[USERS[0], USERS[1], "User", "rename"]);
    let find = at(&r.b, &[USERS[0], USERS[1], "UserService", "find"]);
    let refs = refs_from(&r.b, find);
    assert!(refs.contains(&(user, 20, 20, 4)), "return type: {refs:?}");
    assert!(refs.contains(&(user, 21, 18, 4)), "new User(): {refs:?}");
    assert!(refs.contains(&(rename, 22, 6, 6)), "u.rename(): {refs:?}");
    let service = at(&r.b, &users("UserService"));
    let load = at(&r.b, &[USERS[0], USERS[1], "UserService", "load"]);
    assert!(refs_from(&r.b, load).contains(&(user, 12, 18, 4)), "generic argument of the return type");
    let store = at(&r.b, &["@demo/shared", "src", "Store"]);
    let store_get = at(&r.b, &["@demo/shared", "src", "Store", "get"]);
    let log = at(&r.b, &["@demo/shared", "src", "log"]);
    assert!(refs_from(&r.b, load).contains(&(store_get, 15, 15, 3)));
    assert!(refs_from(&r.b, load).contains(&(log, 13, 4, 3)));
    let service_refs = refs_from(&r.b, service);
    assert!(service_refs.contains(&(store, 10, 18, 5)), "field type is referenced from the owning type: {service_refs:?}");
    let base = at(&r.b, &users("BaseService"));
    assert!(service_refs.contains(&(base, 8, 33, 11)), "extends: {service_refs:?}");
    let ctor = at(&r.b, &["web", "src/app", "AppComponent", "constructor"]);
    assert!(refs_from(&r.b, ctor).contains(&(service, 7, 29, 11)), "constructor parameter type");
    let on_init = at(&r.b, &["web", "src/app", "AppComponent", "ngOnInit"]);
    let users_load = at(&r.b, &[USERS[0], USERS[1], "UserService", "load"]);
    assert!(refs_from(&r.b, on_init).contains(&(users_load, 10, 15, 4)));
    let refresh = at(&r.b, &[USERS[0], USERS[1], "UserListComponent", "refresh"]);
    let format = at(&r.b, &["web", "src/app/util", "Helpers", "format"]);
    let format2 = at(&r.b, &["web", "src/app/util", "format2"]);
    assert!(refs_from(&r.b, refresh).contains(&(format, 16, 12, 6)), "static call");
    assert!(refs_from(&r.b, refresh).contains(&(format2, 17, 6, 7)), "namespace import call");
    let f = file(&r, "user.service.ts");
    assert!(f.type_uses.iter().any(|u| u.name == "User" && u.line == 20 && u.col == 20 && u.len == 4));
    assert!(f.calls.iter().any(|c| c.method == "rename" && c.line == 22 && c.col == 6 && c.len == 6));
}

#[test]
fn aliased_default_and_namespace_imports_resolve_in_js_and_ts() {
    let (b, _) = project(&[
        ("a.ts", "export class A { go() {} }\nexport default class D { run() {} }\n"),
        (
            "use.js",
            "import Dflt from './a';\nimport { A as Z } from './a';\nimport * as ns from './a';\nexport function f() {\n  new Dflt().run();\n  const z = new Z();\n  z.go();\n  new ns.A();\n}\n",
        ),
    ]);
    let f = at(&b, &["u", "src", "f"]);
    let (a, d) = (at(&b, &["u", "src", "A"]), at(&b, &["u", "src", "D"]));
    let go = at(&b, &["u", "src", "A", "go"]);
    let refs = refs_from(&b, f);
    for want in [(d, 5, 6, 4), (a, 6, 16, 1), (go, 7, 4, 2), (a, 8, 9, 1)] {
        assert!(refs.contains(&want), "{want:?} in {refs:?}");
    }
}

#[test]
fn field_initializers_and_property_types_are_referenced_from_the_owning_type() {
    let (b, _) = project(&[
        ("a.ts", "export class A {}\nexport interface I {}\n"),
        ("c.ts", "import { A, I } from './a';\nexport class C implements I {\n  x: A[] = [];\n  y = new A();\n  readonly z: Map<string, A> = new Map();\n}\n"),
    ]);
    let c = at(&b, &["u", "src", "C"]);
    let (a, i) = (at(&b, &["u", "src", "A"]), at(&b, &["u", "src", "I"]));
    let refs = refs_from(&b, c);
    for want in [(i, 2, 26, 1), (a, 3, 5, 1), (a, 4, 10, 1), (a, 5, 26, 1)] {
        assert!(refs.contains(&want), "{want:?} in {refs:?}");
    }
}

#[test]
fn shadowed_guessed_and_type_parameter_names_get_no_ref() {
    let (b, _) = project(&[
        ("a.ts", "export class A { go() {} }\nexport class B { go() {} }\n"),
        (
            "c.ts",
            "import { A } from './a';\nexport function g<A>(x: A) {}\nexport function h(A: number) { return new A(); }\nexport function k() {\n  const A = make();\n  new A();\n  A.go();\n}\nexport function m(x) {\n  x.go();\n  unknown.go();\n}\nexport function n() {\n  type A = number;\n  let y: A;\n}\nexport class W<A> {\n  f: A;\n}\n",
        ),
        ("d.ts", "import { A } from './a';\nexport function ok(x: A) { new A(); }\n"),
    ]);
    let a = at(&b, &["u", "src", "A"]);
    for f in ["g", "h", "k", "m", "n"] {
        let refs = refs_from(&b, at(&b, &["u", "src", f]));
        assert!(refs.is_empty(), "{f}: {refs:?}");
    }
    assert!(refs_from(&b, at(&b, &["u", "src", "W"])).iter().all(|r| r.0 != a), "class type parameter");
    let ok = refs_from(&b, at(&b, &["u", "src", "ok"]));
    assert!(ok.contains(&(a, 2, 22, 1)) && ok.contains(&(a, 2, 31, 1)), "control: {ok:?}");
}
