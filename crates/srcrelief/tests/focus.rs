use srcrelief::focus::*;
use srcrelief_format::*;
use std::collections::HashMap;

struct Fx {
    _dir: tempfile::TempDir,
    file: ReliefFile,
}

fn fixture() -> Fx {
    let mut b = GraphBuilder::new("p");
    let m = b.add_node(b.root(), NodeKind::Module, SubKind::None, "job-engine", Lang::Go);
    let mut ids: HashMap<&str, NodeId> = HashMap::new();
    for (pkg, funcs) in [("cmd", &["main"][..]), ("consumer", &["setupConsumer", "runWorker", "handle"][..]), ("jobs", &["send", "retry"][..]), ("store", &["save"][..])] {
        let p = b.add_node(m, NodeKind::Package, SubKind::None, pkg, Lang::Go);
        for f in funcs {
            let n = b.add_node(p, NodeKind::Member, SubKind::Function, f, Lang::Go);
            b.set_span(n, "f", 1, 10);
            ids.insert(f, n);
        }
    }
    for (a, c) in [("main", "setupConsumer"), ("setupConsumer", "runWorker"), ("runWorker", "handle"), ("handle", "send"), ("send", "save"), ("retry", "send")] {
        b.add_edge(ids[a], ids[c], EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let main = b.add_entry(EntryPointSpec { node: ids["main"], kind: "main".into(), category: Category::Application, label: "job-engine".into(), parent: None, lifecycle: Lifecycle::Once, order: 0, rule: "r".into(), handoff: None });
    b.add_entry(EntryPointSpec { node: ids["runWorker"], kind: "message-handler".into(), category: Category::Application, label: "consume in runWorker".into(), parent: Some(main), lifecycle: Lifecycle::OnEvent, order: 0, rule: "r".into(), handoff: None });
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let file = ReliefFile::open(&dir.path().join("g")).unwrap();
    Fx { _dir: dir, file }
}

fn named(g: &ArchivedGraph, name: &str) -> NodeId {
    (0..g.node_count() as NodeId).find(|&n| g.name(n) == name).unwrap()
}

fn names(g: &ArchivedGraph, v: &[NodeId]) -> Vec<String> {
    let mut out: Vec<String> = v.iter().map(|&n| g.name(n).to_string()).collect();
    out.sort();
    out
}

#[test]
fn neighbours_show_one_step_down_and_up() {
    let fx = fixture();
    let g = fx.file.graph();
    let idx = UsageIndex::new(g);
    let entry = (0..g.entries().len()).find(|&i| g.entry_label(i).starts_with("consume")).unwrap();
    let f = focus(g, &idx, named(g, "runWorker"), Some(entry), FocusMode::Neighbours);
    assert_eq!(names(g, &f.calls), ["handle"]);
    assert_eq!(names(g, &f.callers), ["main", "setupConsumer"], "caller plus the main that sets the consumer up");
    assert_eq!(f.marks.mark(named(g, "runWorker")), Mark::Focus);
    assert_eq!(f.marks.mark(named(g, "handle")), Mark::Down);
    assert_eq!(f.marks.mark(named(g, "setupConsumer")), Mark::Up);
    assert_eq!(f.marks.mark(named(g, "send")), Mark::None, "two steps down is not highlighted");
    assert_ne!(f.marks.mark(named(g, "consumer")), Mark::None, "containers of highlighted boxes are marked");
    assert_eq!(f.marks.mark(named(g, "jobs")), Mark::None);
}

#[test]
fn containers_list_links_leaving_them() {
    let fx = fixture();
    let g = fx.file.graph();
    let idx = UsageIndex::new(g);
    let f = focus(g, &idx, named(g, "consumer"), None, FocusMode::Neighbours);
    assert_eq!(names(g, &f.calls), ["send"]);
    assert_eq!(names(g, &f.callers), ["main"]);
}

#[test]
fn reaches_and_used_by_are_transitive() {
    let fx = fixture();
    let g = fx.file.graph();
    let idx = UsageIndex::new(g);
    let down = focus(g, &idx, named(g, "runWorker"), None, FocusMode::Reaches);
    for n in ["handle", "send", "save"] {
        assert_eq!(down.marks.mark(named(g, n)), Mark::Down, "{n}");
    }
    assert_eq!(down.marks.mark(named(g, "retry")), Mark::None);
    let up = focus(g, &idx, named(g, "send"), None, FocusMode::UsedBy);
    for n in ["handle", "runWorker", "setupConsumer", "main", "retry"] {
        assert_eq!(up.marks.mark(named(g, n)), Mark::Up, "{n}");
    }
    assert_eq!(up.marks.mark(named(g, "save")), Mark::None);
}

#[test]
fn history_goes_back() {
    let mut h = History::default();
    assert_eq!(h.back(), None);
    h.push(1);
    h.push(2);
    h.push(2);
    h.push(3);
    assert_eq!(h.back(), Some(2));
    assert_eq!(h.back(), Some(1));
    assert_eq!(h.back(), None);
}
