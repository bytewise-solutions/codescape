use codescape::inspector::*;
use codescape_format::*;

struct Fx {
    _dir: tempfile::TempDir,
    file: IndexFile,
}

fn fixture() -> Fx {
    let mut b = GraphBuilder::new("p");
    let m = b.add_node(b.root(), NodeKind::Module, SubKind::None, "job-engine", Lang::Go);
    let prod = b.add_node(m, NodeKind::Package, SubKind::None, "internal/jobs/producer", Lang::Go);
    let job = b.add_node(m, NodeKind::Package, SubKind::None, "internal/jobs", Lang::Go);
    let msg = b.add_node(m, NodeKind::Package, SubKind::None, "messaging", Lang::Go);
    let t = b.add_node(prod, NodeKind::Type, SubKind::Struct, "jobProducer", Lang::Go);
    b.set_span(t, "job-engine/internal/jobs/producer/job_producer.go", 34, 42);
    b.add_tag(t, "layer:service");
    let ctor = b.add_node(t, NodeKind::Member, SubKind::Constructor, "NewJobProducer", Lang::Go);
    let run = b.add_node(t, NodeKind::Member, SubKind::Method, "processJobBatch", Lang::Go);
    b.set_span(run, "job-engine/internal/jobs/producer/job_producer.go", 187, 259);
    let fail = b.add_node(t, NodeKind::Member, SubKind::Method, "handleFailure", Lang::Go);
    let dto = b.add_node(job, NodeKind::Member, SubKind::Function, "NewJobDTO", Lang::Go);
    let ent = b.add_node(job, NodeKind::Member, SubKind::Function, "NewJobEntity", Lang::Go);
    let client = b.add_node(msg, NodeKind::Type, SubKind::Interface, "QueueClient", Lang::Go);
    let publish = b.add_node(client, NodeKind::Member, SubKind::Method, "PublishRetrying", Lang::Go);
    let wire = b.add_node(job, NodeKind::Member, SubKind::Function, "provideJobs", Lang::Go);
    b.add_flags(wire, NodeFlags::WIRING);
    let _ = (ctor, fail, dto, ent, publish);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let file = IndexFile::open(&dir.path().join("g")).unwrap();
    Fx { _dir: dir, file }
}

fn named(g: &ArchivedGraph, name: &str) -> NodeId {
    (0..g.node_count() as NodeId).find(|&n| g.name(n) == name).unwrap()
}

#[test]
fn header_says_what_and_where() {
    let fx = fixture();
    let g = fx.file.graph();
    let h = header(g, named(g, "processJobBatch"));
    assert_eq!(h.kind, "METHOD · Go");
    assert_eq!(h.name, "processJobBatch");
    let owners: Vec<&str> = h.owners.iter().map(|(_, s)| s.as_str()).collect();
    assert_eq!(owners, ["jobProducer", "internal/jobs/producer"]);
    assert_eq!(h.location.as_deref(), Some("job_producer.go:187-259 · 73 lines · complexity 1"));
    let pkg = header(g, named(g, "messaging"));
    assert_eq!((pkg.kind.as_str(), pkg.location), ("PACKAGE · Go", None));
    assert_eq!(pkg.owners.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), ["job-engine"]);
}

#[test]
fn links_are_grouped_by_package_with_the_own_type_apart() {
    let fx = fixture();
    let g = fx.file.graph();
    let run = named(g, "processJobBatch");
    let links = [named(g, "PublishRetrying"), named(g, "NewJobEntity"), named(g, "handleFailure"), named(g, "NewJobDTO")];
    let groups = group_links(g, run, &links);
    let shape: Vec<(&str, Vec<&str>)> = groups.iter().map(|gr| (gr.label.as_str(), gr.items.iter().map(|(_, s)| s.as_str()).collect())).collect();
    assert_eq!(
        shape,
        [("internal/jobs", vec!["NewJobDTO", "NewJobEntity"]), ("messaging", vec!["QueueClient.PublishRetrying"]), ("this type", vec!["handleFailure"]),]
    );
}

#[test]
fn inside_counts_children_by_kind() {
    let fx = fixture();
    let g = fx.file.graph();
    let t = named(g, "jobProducer");
    let (summary, kids) = inside(g, t, &|_| false);
    assert_eq!(summary, "3 members: 1 constructor, 2 methods");
    assert_eq!(kids.len(), 3);
    let wiring = named(g, "provideJobs");
    let (summary, _) = inside(g, named(g, "internal/jobs"), &|n| n == wiring);
    assert_eq!(summary, "2 members: 2 functions", "hidden children are not counted");
}

#[test]
fn tags_and_flags_become_readable_chips() {
    let fx = fixture();
    let g = fx.file.graph();
    let labels = |n: NodeId, entry: bool| chips(g, n, entry).into_iter().map(|c| c.label).collect::<Vec<_>>();
    assert_eq!(labels(named(g, "jobProducer"), true), ["service", "has entry point"]);
    assert_eq!(labels(named(g, "provideJobs"), false), ["wiring"]);
    assert!(chips(g, named(g, "jobProducer"), false).iter().all(|c| !c.hint.is_empty()));
}

#[test]
fn inside_lists_implementations_folded_into_an_interface() {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "jobs", Lang::Go);
    let port = b.add_node(pkg, NodeKind::Type, SubKind::Interface, "JobService", Lang::Go);
    b.add_node(port, NodeKind::Member, SubKind::Method, "Run", Lang::Go);
    let imp = b.add_node(pkg, NodeKind::Type, SubKind::Struct, "jobService", Lang::Go);
    b.add_edge(imp, port, EdgeKind::Implements, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let (summary, kids) = inside(g, named(g, "JobService"), &|_| false);
    assert_eq!(kids.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(), ["Run", "jobService"]);
    assert_eq!(summary, "2 items: 1 method, 1 struct");
}

#[test]
fn header_names_the_visibility_when_known() {
    let mut b = GraphBuilder::new("p");
    let t = b.add_node(b.root(), NodeKind::Type, SubKind::Class, "Store", Lang::Java);
    let m = b.add_node(t, NodeKind::Member, SubKind::Method, "evict", Lang::Java);
    b.set_visibility(m, Visibility::Private);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    assert_eq!(header(g, named(g, "evict")).kind, "METHOD · Java · private");
    assert_eq!(header(g, named(g, "Store")).kind, "CLASS · Java", "unknown visibility says nothing");
}

#[test]
fn used_by_leaves_out_tests_while_tests_are_hidden() {
    let mut b = GraphBuilder::new("p");
    let m = b.add_node(b.root(), NodeKind::Module, SubKind::None, "m", Lang::Go);
    let t = b.add_node(m, NodeKind::Type, SubKind::Struct, "T", Lang::Go);
    b.add_node(t, NodeKind::Member, SubKind::Method, "target", Lang::Go);
    b.add_node(t, NodeKind::Member, SubKind::Method, "fromMain", Lang::Go);
    let test = b.add_node(t, NodeKind::Member, SubKind::Method, "fromTest", Lang::Go);
    b.add_flags(test, NodeFlags::TEST);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let file = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = file.graph();
    let callers = [named(g, "fromMain"), named(g, "fromTest")];

    let (shown, dropped) = visible_links(g, &callers, &codescape_layout::LayoutOptions::default());
    assert_eq!((shown, dropped), (vec![named(g, "fromMain")], 1));

    let opts = codescape_layout::LayoutOptions { show_tests: true, ..Default::default() };
    let (shown, dropped) = visible_links(g, &callers, &opts);
    assert_eq!((shown, dropped), (callers.to_vec(), 0));
}
