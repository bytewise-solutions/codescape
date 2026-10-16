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
    assert_eq!(r.b.node_kind(at(&r.b, &["svc-a", "internal/handler", "New"])), NodeKind::Member);
    assert_eq!(r.b.node_kind(at(&r.b, &["shared", "(root)"])), NodeKind::Package);
    assert_eq!(r.stats.files, 6);
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
    let new = at(&r.b, &["svc-a", "internal/handler", "New"]);
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
    let new = at(&r.b, &["svc-a", "internal/handler", "New"]);
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
    assert_eq!(r.stats.unresolved, 0, "{:?}", r.stats);
}
