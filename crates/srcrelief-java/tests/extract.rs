use srcrelief_build::{default_excludes, discover};
use srcrelief_facts::{ExtractStats, ProjectFacts};
use srcrelief_format::*;
use srcrelief_java::extract_java;
use std::path::Path;

struct Run {
    b: GraphBuilder,
    facts: ProjectFacts,
    stats: ExtractStats,
}

fn run() -> Run {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/java-resolve");
    let d = discover(&root, &default_excludes()).unwrap();
    let mut b = GraphBuilder::new("java-resolve");
    let module_nodes: Vec<NodeId> = d
        .modules
        .iter()
        .map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, Lang::Java))
        .collect();
    let mut facts = ProjectFacts::default();
    let stats = extract_java(&d, &module_nodes, &default_excludes(), &mut b, &mut facts);
    Run { b, facts, stats }
}

fn at(b: &GraphBuilder, path: &[&str]) -> NodeId {
    let mut n = b.root();
    for p in path {
        n = b.child(n, p).unwrap_or_else(|| panic!("missing {p} in {path:?}"));
    }
    n
}

fn has_edge(b: &GraphBuilder, from: NodeId, to: NodeId, kind: EdgeKind) -> Option<RawEdge> {
    b.edges().iter().find(|e| e.from == from && e.to == to && e.kind == kind).copied()
}

#[test]
fn packages_types_members_nested_under_modules() {
    let r = run();
    let main = at(&r.b, &["app", "com.example.app", "Main", "main"]);
    assert_eq!(r.b.node_kind(main), NodeKind::Member);
    assert_eq!(r.b.file(main), Some("app/src/main/java/com/example/app/Main.java"));
    let fx = r.facts.files.iter().find(|f| f.rel_path.ends_with("Main.java")).unwrap();
    assert!(fx.functions.iter().any(|f| f.name == "main" && f.is_main && f.node == main));
    assert_eq!(r.stats.files, 15);
}

#[test]
fn field_and_local_typed_calls_resolve() {
    let r = run();
    let go = at(&r.b, &["app", "com.example.app", "ViewUser", "go"]);
    let target = at(&r.b, &["core", "com.example.core", "ViewManager", "setCurrentView"]);
    let e = has_edge(&r.b, go, target, EdgeKind::Calls).expect("field call");
    assert_eq!(e.confidence, Confidence::Resolved);
    let update = at(&r.b, &["core", "com.example.core", "MessageQueue", "update"]);
    assert!(has_edge(&r.b, go, update, EdgeKind::Calls).is_some());
    let mq = at(&r.b, &["core", "com.example.core", "MessageQueue"]);
    assert!(has_edge(&r.b, go, mq, EdgeKind::Instantiates).is_some());
    let trim = at(&r.b, &["util", "com.example.util", "Strings", "trim"]);
    assert!(has_edge(&r.b, go, trim, EdgeKind::Calls).is_some());
}

#[test]
fn single_impl_followed() {
    let r = run();
    let go = at(&r.b, &["app", "com.example.app", "ViewUser", "go"]);
    let imp = at(&r.b, &["core", "com.example.core", "ViewManagerImpl", "setCurrentView"]);
    assert!(has_edge(&r.b, go, imp, EdgeKind::Calls).is_some());
    let fake = at(&r.b, &["core", "com.example.core", "FakeViewManager", "setCurrentView"]);
    assert!(has_edge(&r.b, go, fake, EdgeKind::Calls).is_none());
    let fake_ty = at(&r.b, &["core", "com.example.core", "FakeViewManager"]);
    assert!(r.b.flags(fake_ty).contains(NodeFlags::TEST));
    let iface = at(&r.b, &["core", "com.example.core", "ViewManager"]);
    let impl_ty = at(&r.b, &["core", "com.example.core", "ViewManagerImpl"]);
    assert!(has_edge(&r.b, impl_ty, iface, EdgeKind::Implements).is_some());
}

#[test]
fn cross_module_import_creates_package_edge() {
    let r = run();
    let app_pkg = at(&r.b, &["app", "com.example.app"]);
    let core_pkg = at(&r.b, &["core", "com.example.core"]);
    let e = has_edge(&r.b, app_pkg, core_pkg, EdgeKind::DependsOn).expect("app->core package edge");
    assert!(!e.flags.contains(EdgeFlags::UNDECLARED));
    let util_pkg = at(&r.b, &["util", "com.example.util"]);
    let transitive = has_edge(&r.b, app_pkg, util_pkg, EdgeKind::DependsOn).expect("app->util via api");
    assert!(!transitive.flags.contains(EdgeFlags::UNDECLARED));
    let leak = has_edge(&r.b, util_pkg, core_pkg, EdgeKind::DependsOn).expect("util->core edge");
    assert!(leak.flags.contains(EdgeFlags::UNDECLARED));
    let app = at(&r.b, &["app"]);
    let core = at(&r.b, &["core"]);
    assert_eq!(has_edge(&r.b, app, core, EdgeKind::DependsOn).unwrap().scope, Scope::Main);
}

#[test]
fn external_supertype_recorded() {
    let r = run();
    let fx = r.facts.files.iter().find(|f| f.rel_path.ends_with("ClientApp.java")).unwrap();
    let ty = &fx.types[0];
    assert_eq!(ty.fqn, "com.example.app.ClientApp");
    assert_eq!(ty.supertypes, ["com.badlogic.gdx.Game"]);
    let ext = at(&r.b, &["(external)", "com.badlogic.gdx.Game"]);
    assert!(r.b.flags(ext).contains(NodeFlags::EXTERNAL));
    let create = fx.functions.iter().find(|f| f.name == "create").unwrap();
    assert_eq!(create.overrides_external.as_deref(), Some("com.badlogic.gdx.Game"));
    let helper = fx.functions.iter().find(|f| f.name == "helper").unwrap();
    assert_eq!(helper.overrides_external, None);
    let main_fx = r.facts.files.iter().find(|f| f.rel_path.ends_with("Main.java")).unwrap();
    assert!(main_fx.calls.iter().any(|c| c.instantiates.as_deref() == Some("com.example.app.ClientApp")));
}

#[test]
fn unresolved_counted_and_unique_name_guessed() {
    let r = run();
    let go = at(&r.b, &["app", "com.example.app", "ViewUser", "go"]);
    let widget = at(&r.b, &["core", "com.example.core", "Widgets", "buildWidgetOnce"]);
    assert_eq!(has_edge(&r.b, go, widget, EdgeKind::Calls).unwrap().confidence, Confidence::Guessed);
    assert!(r.stats.unresolved >= 1);
    assert!(r.stats.guessed >= 1);
    assert!(r.stats.resolved >= 5);
}

#[test]
fn test_files_flagged() {
    let r = run();
    let fx = r.facts.files.iter().find(|f| f.rel_path.ends_with("FakeViewManager.java")).unwrap();
    assert!(fx.is_test);
}

#[test]
fn guesses_never_cross_into_invisible_modules() {
    let r = run();
    let leak = at(&r.b, &["util", "com.example.util", "Leak", "use"]);
    let only = at(&r.b, &["app", "com.example.app", "ViewUser", "onlyInApp"]);
    assert!(has_edge(&r.b, leak, only, EdgeKind::Calls).is_none(), "util cannot see app");
}

#[test]
fn same_package_names_resolve_only_into_visible_modules() {
    let r = run();
    let leak = at(&r.b, &["util", "com.example.util", "Leak", "use"]);
    let shadow = at(&r.b, &["app", "com.example.util", "Shadow"]);
    assert!(has_edge(&r.b, leak, shadow, EdgeKind::Instantiates).is_none(), "util cannot see app's com.example.util.Shadow");
}

#[test]
fn single_impl_not_followed_into_invisible_module() {
    let r = run();
    let fire = at(&r.b, &["core", "com.example.core", "Bus", "fire"]);
    let iface = at(&r.b, &["core", "com.example.core", "Listener", "onEvent"]);
    let imp = at(&r.b, &["app", "com.example.app", "AppListener", "onEvent"]);
    assert!(has_edge(&r.b, fire, iface, EdgeKind::Calls).is_some());
    assert!(has_edge(&r.b, fire, imp, EdgeKind::Calls).is_none(), "core cannot see app");
}

#[test]
fn main_code_does_not_see_test_scope_dependencies() {
    let r = run();
    let fire = at(&r.b, &["core", "com.example.core", "Bus", "fire"]);
    let only = at(&r.b, &["app", "com.example.app", "ViewUser", "onlyInApp"]);
    assert!(has_edge(&r.b, fire, only, EdgeKind::Calls).is_none(), "core -> app is a test-only dependency");
}

#[test]
fn java_files_record_package_and_import_targets() {
    let r = run();
    let fx = r.facts.files.iter().find(|f| f.rel_path.ends_with("ViewUser.java")).unwrap();
    assert_eq!(fx.package, Some(at(&r.b, &["app", "com.example.app"])));
    assert!(fx.import_targets.contains(&at(&r.b, &["core", "com.example.core"])));
    assert!(fx.import_targets.contains(&at(&r.b, &["util", "com.example.util"])));
}
