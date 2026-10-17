use srcrelief_build::{default_excludes, discover};
use srcrelief_facts::{ExtractStats, ProjectFacts};
use srcrelief_format::*;
use srcrelief_go::extract_go;
use std::path::Path;

struct Run {
    b: GraphBuilder,
    facts: ProjectFacts,
    stats: ExtractStats,
}

fn run() -> Run {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/go-resolve");
    let d = discover(&root, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("go-resolve");
    let module_nodes: Vec<NodeId> = d
        .modules
        .iter()
        .map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, Lang::Go))
        .collect();
    let mut facts = ProjectFacts::default();
    let stats = extract_go(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    Run { b, facts, stats }
}

fn at(b: &GraphBuilder, path: &[&str]) -> NodeId {
    let mut n = b.root();
    for p in path {
        n = b.child(n, p).unwrap_or_else(|| panic!("missing {p} in {path:?}"));
    }
    n
}

fn edge(b: &GraphBuilder, from: NodeId, to: NodeId, kind: EdgeKind) -> Option<RawEdge> {
    b.edges().iter().find(|e| e.from == from && e.to == to && e.kind == kind).copied()
}

#[test]
fn packages_are_directories() {
    let r = run();
    let pkg = at(&r.b, &["svc-a", "internal/handler"]);
    assert_eq!(r.b.node_kind(pkg), NodeKind::Package);
    assert_eq!(r.b.node_kind(at(&r.b, &["svc-a", "internal/handler", "H", "Serve"])), NodeKind::Member);
    assert_eq!(r.b.node_kind(at(&r.b, &["svc-a", "internal/handler", "H", "New"])), NodeKind::Member);
    assert_eq!(r.b.node_kind(at(&r.b, &["shared", "(root)"])), NodeKind::Package);
    assert_eq!(r.stats.files, 7);
}

#[test]
fn cross_module_calls_resolve_via_import_path() {
    let r = run();
    let main = at(&r.b, &["svc-a", "cmd/svc-a", "main"]);
    let log = at(&r.b, &["shared", "(root)", "Log"]);
    assert_eq!(edge(&r.b, main, log, EdgeKind::Calls).unwrap().confidence, Confidence::Resolved);
    let from = at(&r.b, &["svc-a", "cmd/svc-a"]);
    let to = at(&r.b, &["shared", "(root)"]);
    assert!(!edge(&r.b, from, to, EdgeKind::DependsOn).unwrap().flags.contains(EdgeFlags::UNDECLARED));
    let new = at(&r.b, &["svc-a", "internal/handler", "H", "New"]);
    assert!(edge(&r.b, main, new, EdgeKind::Calls).is_some());
    let serve = at(&r.b, &["svc-a", "internal/handler", "H", "Serve"]);
    assert!(edge(&r.b, main, serve, EdgeKind::Calls).is_some(), "h := handler.New(r); h.Serve() uses New's result type");
    let svc_a = at(&r.b, &["svc-a"]);
    let shared = at(&r.b, &["shared"]);
    assert!(edge(&r.b, svc_a, shared, EdgeKind::DependsOn).is_some());
}

#[test]
fn method_calls_via_receiver_and_field_types() {
    let r = run();
    let serve = at(&r.b, &["svc-a", "internal/handler", "H", "Serve"]);
    let do_ = at(&r.b, &["svc-a", "internal/handler", "Service", "Do"]);
    assert!(edge(&r.b, serve, do_, EdgeKind::Calls).is_some());
    let new = at(&r.b, &["svc-a", "internal/handler", "H", "New"]);
    let h = at(&r.b, &["svc-a", "internal/handler", "H"]);
    assert!(edge(&r.b, new, h, EdgeKind::Instantiates).is_some());
}

#[test]
fn interface_single_impl_ignores_mocks_and_tests() {
    let r = run();
    let do_ = at(&r.b, &["svc-a", "internal/handler", "Service", "Do"]);
    let iface_get = at(&r.b, &["svc-a", "internal/repo", "Repo", "Get"]);
    let pg_get = at(&r.b, &["svc-a", "internal/repo", "pgRepo", "Get"]);
    let mock_get = at(&r.b, &["svc-a", "internal/repo/mocks", "MockRepo", "Get"]);
    let fake_get = at(&r.b, &["svc-a", "internal/repo", "fakeRepo", "Get"]);
    assert!(edge(&r.b, do_, iface_get, EdgeKind::Calls).is_some());
    assert!(edge(&r.b, do_, pg_get, EdgeKind::Calls).is_some());
    assert!(edge(&r.b, do_, mock_get, EdgeKind::Calls).is_none());
    assert!(edge(&r.b, do_, fake_get, EdgeKind::Calls).is_none());
    assert!(r.b.flags(at(&r.b, &["svc-a", "internal/repo/mocks", "MockRepo"])).contains(NodeFlags::MOCK));
    assert!(r.b.flags(at(&r.b, &["svc-a", "internal/repo/mocks"])).contains(NodeFlags::MOCK));
    assert!(r.b.flags(at(&r.b, &["svc-a", "internal/repo", "fakeRepo"])).contains(NodeFlags::TEST));
}

#[test]
fn mains_marked() {
    let r = run();
    let fx = r.facts.files.iter().find(|f| f.rel_path == "svc-a/cmd/svc-a/main.go").unwrap();
    assert!(fx.functions.iter().any(|f| f.name == "main" && f.is_main));
    let other = r.facts.files.iter().find(|f| f.rel_path.ends_with("handler.go")).unwrap();
    assert!(other.functions.iter().all(|f| !f.is_main));
}

#[test]
fn receiver_type_from_param() {
    let r = run();
    let fx = r.facts.files.iter().find(|f| f.rel_path.ends_with("handler.go")).unwrap();
    let get = fx.calls.iter().find(|c| c.method == "Get" && c.string_args.first() == Some(&Some("/x".into()))).unwrap();
    assert_eq!(get.receiver_type.as_deref(), Some("fiber.Router"));
    assert_eq!(get.receiver_var.as_deref(), Some("r"));
    let serve = at(&r.b, &["svc-a", "internal/handler", "H", "Serve"]);
    assert_eq!(get.arg_targets[1], Some(serve));
    assert!(fx.imports.iter().any(|i| i.path == "github.com/gofiber/fiber/v2"));
}

#[test]
fn builtins_not_counted_unresolved() {
    let r = run();
    assert_eq!(r.stats.unresolved, 1, "only shared's chained call stays unresolved: {:?}", r.stats);
}

#[test]
fn guesses_never_cross_into_invisible_modules() {
    let r = run();
    let log = at(&r.b, &["shared", "(root)", "Log"]);
    let only = at(&r.b, &["svc-a", "internal/handler", "H", "OnlyInSvc"]);
    assert!(edge(&r.b, log, only, EdgeKind::Calls).is_none(), "shared cannot see svc-a");
}

#[test]
fn single_impl_not_followed_into_invisible_module() {
    let r = run();
    let run_fn = at(&r.b, &["shared", "(root)", "Bus", "Run"]);
    let iface = at(&r.b, &["shared", "(root)", "Hook", "Fire"]);
    let imp = at(&r.b, &["svc-a", "internal/handler", "myHook", "Fire"]);
    assert!(edge(&r.b, run_fn, iface, EdgeKind::Calls).is_some());
    assert!(edge(&r.b, run_fn, imp, EdgeKind::Calls).is_none(), "shared cannot see svc-a");
}

#[test]
fn constructors_fold_into_their_type() {
    let r = run();
    let new = at(&r.b, &["svc-a", "internal/handler", "H", "New"]);
    assert_eq!(r.b.sub_kind(new), SubKind::Constructor);
    assert!(r.b.child(at(&r.b, &["svc-a", "internal/handler"]), "New").is_none());
    let pg = at(&r.b, &["svc-a", "internal/repo", "pgRepo", "NewPgRepo"]);
    assert_eq!(r.b.sub_kind(pg), SubKind::Constructor);
    let fx = r.facts.files.iter().find(|f| f.rel_path.ends_with("handler.go")).unwrap();
    let f = fx.functions.iter().find(|f| f.name == "New").unwrap();
    assert_eq!(f.owner, Some(at(&r.b, &["svc-a", "internal/handler", "H"])));
    let main = at(&r.b, &["svc-a", "cmd/svc-a", "main"]);
    assert!(edge(&r.b, main, pg, EdgeKind::Calls).is_some(), "calls still reach a folded constructor");
}

#[test]
fn implements_edges_inferred() {
    let r = run();
    let pg = at(&r.b, &["svc-a", "internal/repo", "pgRepo"]);
    let repo = at(&r.b, &["svc-a", "internal/repo", "Repo"]);
    assert!(edge(&r.b, pg, repo, EdgeKind::Implements).is_some());
    let mock = at(&r.b, &["svc-a", "internal/repo/mocks", "MockRepo"]);
    assert!(edge(&r.b, mock, repo, EdgeKind::Implements).is_none());
    let hook = at(&r.b, &["shared", "(root)", "Hook"]);
    let my = at(&r.b, &["svc-a", "internal/handler", "myHook"]);
    assert!(edge(&r.b, my, hook, EdgeKind::Implements).is_some(), "svc-a can see shared");
}

#[test]
fn generic_interfaces_get_no_implements_edges() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("go.mod"), "module example.com/g\n\ngo 1.22\n").unwrap();
    let mut src = String::from("package g\n\ntype Closer interface {\n\tClose() error\n}\n\ntype Pair interface {\n\tClose() error\n\tName() string\n}\n");
    for i in 0..9 {
        src.push_str(&format!("\ntype s{i} struct{{}}\n\nfunc (s *s{i}) Close() error {{ return nil }}\n"));
    }
    src.push_str("\nfunc (s *s0) Name() string { return \"\" }\n");
    std::fs::write(tmp.path().join("g.go"), src).unwrap();
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("g");
    let nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, Lang::Go)).collect();
    let mut facts = ProjectFacts::default();
    extract_go(&d, &nodes, &default_excludes(), &mut b, &mut facts);
    let pkg = b.children(nodes[0])[0];
    let closer = b.child(pkg, "Closer").unwrap();
    let pair = b.child(pkg, "Pair").unwrap();
    assert!(b.edges().iter().all(|e| e.to != closer || e.kind != EdgeKind::Implements), "9 structs match Closer");
    assert_eq!(b.edges().iter().filter(|e| e.to == pair && e.kind == EdgeKind::Implements).count(), 1);
}

#[test]
fn files_record_package_and_import_targets() {
    let r = run();
    let fx = r.facts.files.iter().find(|f| f.rel_path == "svc-a/cmd/svc-a/main.go").unwrap();
    assert_eq!(fx.package, Some(at(&r.b, &["svc-a", "cmd/svc-a"])));
    let mut targets = fx.import_targets.clone();
    targets.sort();
    let mut expected = vec![at(&r.b, &["shared", "(root)"]), at(&r.b, &["svc-a", "internal/handler"]), at(&r.b, &["svc-a", "internal/repo"])];
    expected.sort();
    assert_eq!(targets, expected);
}

#[test]
fn implements_needs_matching_parameter_counts() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("go.mod"), "module example.com/g\n\ngo 1.22\n").unwrap();
    let src = "package g\n\ntype EventProducer interface {\n\tPublish(e *Event) error\n}\n\ntype Event struct{}\n\n\
type eventProducer struct{}\n\nfunc (p *eventProducer) Publish(e *Event) error { return nil }\n\n\
type batchProducer struct{}\n\nfunc (p *batchProducer) Publish(tenant string, b *Event) error { return nil }\n";
    std::fs::write(tmp.path().join("g.go"), src).unwrap();
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("g");
    let nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, Lang::Go)).collect();
    let mut facts = ProjectFacts::default();
    extract_go(&d, &nodes, &default_excludes(), &mut b, &mut facts);
    let pkg = b.children(nodes[0])[0];
    let iface = b.child(pkg, "EventProducer").unwrap();
    let from: Vec<&str> = b.edges().iter().filter(|e| e.to == iface && e.kind == EdgeKind::Implements).map(|e| b.name(e.from)).collect();
    assert_eq!(from, ["eventProducer"]);
}

#[test]
fn exported_names_are_public_and_the_rest_package() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("go.mod"), "module example.com/v\n\ngo 1.22\n").unwrap();
    std::fs::write(tmp.path().join("v.go"), "package v\n\ntype Store struct{}\n\nfunc (s *Store) Get() {}\n\nfunc (s *Store) evict() {}\n\ntype cache struct{}\n\nfunc Open() {}\n\nfunc helper() {}\n").unwrap();
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("v");
    let nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, Lang::Go)).collect();
    let mut facts = ProjectFacts::default();
    extract_go(&d, &nodes, &default_excludes(), &mut b, &mut facts);
    let pkg = b.children(nodes[0])[0];
    let store = b.child(pkg, "Store").unwrap();
    for (n, v) in [(store, Visibility::Public), (b.child(store, "Get").unwrap(), Visibility::Public), (b.child(store, "evict").unwrap(), Visibility::Package), (b.child(pkg, "cache").unwrap(), Visibility::Package), (b.child(pkg, "Open").unwrap(), Visibility::Public), (b.child(pkg, "helper").unwrap(), Visibility::Package)] {
        assert_eq!(b.visibility(n), v, "{}", b.name(n));
    }
}
