use srcrelief_format::*;
use srcrelief_index::analysis::{Usage, analyse};
use srcrelief_index::{ProjectConfig, index_project};

const MAIN: &str = "package main\n\nimport \"example.com/shop/cart\"\n\nfunc main() {\n\tc := cart.New()\n\tc.Add(1)\n}\n";
const CART: &str = "package cart\n\ntype Store interface {\n\tSave(x int)\n}\n\ntype mem struct{}\n\nfunc (m *mem) Save(x int) {}\n\ntype Line struct{}\n\nfunc double(x int) int { return x * 2 }\n\nvar hook = double\n\ntype Cart struct {\n\ts Store\n\tlines []Line\n}\n\nfunc New() *Cart { return &Cart{s: &mem{}} }\n\nfunc (c *Cart) Add(x int) {\n\tc.s.Save(x)\n\tc.helper()\n}\n\nfunc (c *Cart) helper() {}\n\nfunc (c *Cart) stale() {}\n\nfunc (c *Cart) Export() {}\n\nfunc (c *Cart) Checked() {}\n\ntype orphan struct{}\n\nfunc (o orphan) String() string { return \"\" }\n";
const CART_TEST: &str = "package cart\n\nimport \"testing\"\n\nfunc TestChecked(t *testing.T) {\n\t(&Cart{}).Checked()\n}\n";

fn node(g: &ArchivedGraph, path: &[&str]) -> NodeId {
    let mut n = g.root();
    for p in path {
        n = g.children(n).find(|&c| g.name(c) == *p).unwrap_or_else(|| panic!("no {p}"));
    }
    n
}

#[test]
fn unused_blocks_are_told_apart_from_api_test_only_and_implementations() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("proj");
    std::fs::create_dir_all(dir.join("cart")).unwrap();
    std::fs::write(dir.join("go.mod"), "module example.com/shop\n\ngo 1.22\n").unwrap();
    std::fs::write(dir.join("main.go"), MAIN).unwrap();
    std::fs::write(dir.join("cart/cart.go"), CART).unwrap();
    std::fs::write(dir.join("cart/cart_test.go"), CART_TEST).unwrap();
    let cfg = ProjectConfig { index: Some(tmp.path().join("i.relief")), ..Default::default() };
    let file = ReliefFile::open(&index_project(&dir, &cfg, &|_| {}).unwrap().path).unwrap();
    let g = file.graph();
    let a = analyse(g, &dir);
    let usage = |p: &[&str]| a.usage[node(g, p) as usize];
    fn cart(m: &str) -> [&str; 4] {
        ["proj", "cart", "Cart", m]
    }
    assert_eq!(usage(&cart("Add")), Usage::Used);
    assert_eq!(usage(&cart("helper")), Usage::Used);
    assert_eq!(usage(&cart("stale")), Usage::Unused);
    assert_eq!(usage(&cart("Export")), Usage::NoCallerInProject);
    assert_eq!(usage(&cart("Checked")), Usage::OnlyTests);
    assert_eq!(usage(&["proj", "cart", "mem", "Save"]), Usage::Used, "implements Store.Save");
    assert_eq!(usage(&["proj", "cart", "orphan"]), Usage::Unused);
    assert_eq!(usage(&["proj", "cart", "Line"]), Usage::Used, "a field's type");
    assert_eq!(usage(&["proj", "cart", "double"]), Usage::Used, "passed by name");
    assert_eq!(usage(&["proj", "cart", "orphan", "String"]), Usage::Used, "an orphan's methods are reported with it");
    let main = (0..g.node_count() as NodeId).find(|&n| g.name(n) == "main" && g.kind(n) == NodeKind::Member).unwrap();
    assert_eq!(a.usage[main as usize], Usage::Used, "entry point");
    assert!(a.tested[node(g, &cart("Checked")) as usize]);
    assert!(!a.tested[node(g, &cart("Add")) as usize]);
    assert_eq!(a.callers[node(g, &cart("Add")) as usize], 1);
}

#[test]
fn files_run_by_tools_are_not_unused() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/ts-resolve");
    let tmp = tempfile::tempdir().unwrap();
    let cfg = ProjectConfig { index: Some(tmp.path().join("ts.relief")), ..Default::default() };
    let file = ReliefFile::open(&index_project(&dir, &cfg, &|_| {}).unwrap().path).unwrap();
    let g = file.graph();
    let a = analyse(g, &dir);
    let reported: Vec<&str> = (0..g.node_count() as NodeId).filter(|&n| a.usage[n as usize] != Usage::Used).map(|n| g.name(n)).collect();
    assert_eq!(reported, ["refresh"]);
}

#[test]
fn calls_record_the_line_they_are_made_on() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("proj");
    std::fs::create_dir_all(dir.join("cart")).unwrap();
    std::fs::write(dir.join("go.mod"), "module example.com/shop\n\ngo 1.22\n").unwrap();
    std::fs::write(dir.join("main.go"), MAIN).unwrap();
    std::fs::write(dir.join("cart/cart.go"), CART).unwrap();
    let cfg = ProjectConfig { index: Some(tmp.path().join("i.relief")), ..Default::default() };
    let file = ReliefFile::open(&index_project(&dir, &cfg, &|_| {}).unwrap().path).unwrap();
    let g = file.graph();
    let main = (0..g.node_count() as NodeId).find(|&n| g.name(n) == "main" && g.kind(n) == NodeKind::Member).unwrap();
    let lines: Vec<(&str, Option<u32>)> = g.out_edges(main).iter().filter(|e| EdgeKind::from_u8(e.kind) == EdgeKind::Calls).map(|e| (g.name(e.to.to_native()), g.call_line(main, e))).collect();
    assert!(lines.contains(&("New", Some(6))), "{lines:?}");
    assert!(lines.contains(&("Add", Some(7))), "{lines:?}");
}
