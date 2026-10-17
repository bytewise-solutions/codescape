use srcrelief::sequence::*;
use srcrelief_format::*;
use srcrelief_index::{ProjectConfig, index_project};

const MAIN: &str = "package main\n\nimport \"example.com/shop/shop\"\n\nfunc main() {\n\ts := shop.NewService()\n\ts.Checkout()\n}\n";
const SHOP: &str = "package shop\n\ntype Store interface {\n\tSave(x int)\n}\n\ntype mem struct{}\n\nfunc (m *mem) Save(x int) {\n\tlogIt(x)\n}\n\nfunc logIt(x int) {}\n\ntype Service struct {\n\tst Store\n}\n\nfunc NewService() *Service { return &Service{st: &mem{}} }\n\nfunc (s *Service) Checkout() {\n\ts.validate()\n\ts.st.Save(1)\n\ts.total()\n}\n\nfunc (s *Service) validate() {}\n\nfunc (s *Service) total() int {\n\treturn s.sum()\n}\n\nfunc (s *Service) sum() int {\n\treturn s.total()\n}\n";

fn project() -> (tempfile::TempDir, ReliefFile) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("proj");
    std::fs::create_dir_all(dir.join("shop")).unwrap();
    std::fs::write(dir.join("go.mod"), "module example.com/shop\n\ngo 1.22\n").unwrap();
    std::fs::write(dir.join("main.go"), MAIN).unwrap();
    std::fs::write(dir.join("shop/shop.go"), SHOP).unwrap();
    let cfg = ProjectConfig { index: Some(tmp.path().join("i.relief")), ..Default::default() };
    let file = ReliefFile::open(&index_project(&dir, &cfg, &|_| {}).unwrap().path).unwrap();
    (tmp, file)
}

fn named(g: &ArchivedGraph, name: &str) -> NodeId {
    (0..g.node_count() as NodeId).find(|&n| g.name(n) == name && g.kind(n) == NodeKind::Member).unwrap_or_else(|| panic!("no {name}"))
}

#[test]
fn a_method_is_told_from_an_entry_point_down_two_levels_in_call_order() {
    let (_t, file) = project();
    let g = file.graph();
    let calls = CallIndex::new(g);
    let checkout = named(g, "Checkout");
    let chain = calls.default_chain(g, checkout);
    assert_eq!(chain, [named(g, "main"), checkout]);
    let d = diagram(g, &calls, &Query::new(checkout, chain));
    let shown: Vec<(&str, &str, u32)> = d.messages.iter().map(|m| (g.name(m.caller), g.name(m.callee), m.depth)).collect();
    assert_eq!(shown, [("main", "Checkout", 0), ("Checkout", "validate", 1), ("Checkout", "Save", 1), ("Save", "logIt", 2), ("Checkout", "total", 1), ("total", "sum", 2)]);
    let lines: Vec<Option<u32>> = d.messages.iter().map(|m| m.line).collect();
    assert_eq!(&lines[..3], [Some(7), Some(22), Some(23)]);
    assert!(d.messages[0].focus && !d.messages[1].focus);
    let save = &d.messages[2];
    assert_eq!(save.implementations, [named_in(g, "mem", "Save")]);
    assert_eq!(d.lifelines[save.to].label, "mem", "the call lands on the implementation it follows");
    assert!(d.messages[5].more, "sum calls further");
    assert!(!d.messages[1].more);
}

#[test]
fn opening_a_call_shows_one_more_level_and_recursion_stops_there() {
    let (_t, file) = project();
    let g = file.graph();
    let calls = CallIndex::new(g);
    let checkout = named(g, "Checkout");
    let mut q = Query::new(checkout, calls.default_chain(g, checkout));
    q.expanded.insert(named(g, "sum"));
    let d = diagram(g, &calls, &q);
    let last = d.messages.last().unwrap();
    assert_eq!((g.name(last.caller), g.name(last.callee), last.depth), ("sum", "total", 3));
    assert!(last.on_stack && !last.more);
}

#[test]
fn callers_can_be_listed_to_pick_another_chain() {
    let (_t, file) = project();
    let g = file.graph();
    let calls = CallIndex::new(g);
    let total = named(g, "total");
    let names: Vec<&str> = calls.callers(total).iter().map(|&c| g.name(c)).collect();
    assert_eq!(names, ["Checkout", "sum"]);
    assert_eq!(calls.default_chain(g, total), [named(g, "main"), named(g, "Checkout"), total]);
}

fn named_in(g: &ArchivedGraph, owner: &str, name: &str) -> NodeId {
    (0..g.node_count() as NodeId).find(|&n| g.name(n) == name && g.parent(n).is_some_and(|p| g.name(p) == owner)).unwrap()
}
