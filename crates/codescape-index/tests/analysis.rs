use codescape_format::*;
use codescape_index::analysis::{Usage, analyse};
use codescape_index::{ProjectConfig, index_project};

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
    let cfg = ProjectConfig { index: Some(tmp.path().join("i.codescape")), ..Default::default() };
    let file = IndexFile::open(&index_project(&dir, &cfg, &|_| {}).unwrap().path).unwrap();
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
    let cfg = ProjectConfig { index: Some(tmp.path().join("ts.codescape")), ..Default::default() };
    let file = IndexFile::open(&index_project(&dir, &cfg, &|_| {}).unwrap().path).unwrap();
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
    let cfg = ProjectConfig { index: Some(tmp.path().join("i.codescape")), ..Default::default() };
    let file = IndexFile::open(&index_project(&dir, &cfg, &|_| {}).unwrap().path).unwrap();
    let g = file.graph();
    let main = (0..g.node_count() as NodeId).find(|&n| g.name(n) == "main" && g.kind(n) == NodeKind::Member).unwrap();
    let lines: Vec<(&str, Option<u32>)> =
        g.out_edges(main).iter().filter(|e| EdgeKind::from_u8(e.kind) == EdgeKind::Calls).map(|e| (g.name(e.to.to_native()), g.call_line(main, e))).collect();
    assert!(lines.contains(&("New", Some(6))), "{lines:?}");
    assert!(lines.contains(&("Add", Some(7))), "{lines:?}");
}

#[test]
fn a_class_holding_an_entry_point_is_used() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("proj");
    std::fs::create_dir_all(dir.join("src/main/java/b")).unwrap();
    std::fs::write(dir.join("settings.gradle"), "rootProject.name = 'proj'\n").unwrap();
    std::fs::write(dir.join("build.gradle"), "apply plugin: 'java'\n").unwrap();
    std::fs::write(
        dir.join("src/main/java/b/BotLauncher.java"),
        "package b;\n\npublic class BotLauncher {\n    public static void main(String[] args) {\n        run();\n    }\n\n    static void run() {}\n\n    public void stale() {}\n}\n",
    )
    .unwrap();
    let cfg = ProjectConfig { index: Some(tmp.path().join("i.codescape")), ..Default::default() };
    let file = IndexFile::open(&index_project(&dir, &cfg, &|_| {}).unwrap().path).unwrap();
    let g = file.graph();
    let a = analyse(g, &dir);
    let named = |name: &str| (0..g.node_count() as NodeId).find(|&n| g.name(n) == name).unwrap_or_else(|| panic!("no {name}"));
    assert_eq!(a.usage[named("BotLauncher") as usize], Usage::Used, "it holds the main");
    assert_eq!(a.usage[named("run") as usize], Usage::Used);
    assert_ne!(a.usage[named("stale") as usize], Usage::Used, "other members are still judged on their own");
}

#[test]
fn methods_the_runtime_calls_on_every_object_are_used() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("proj");
    std::fs::create_dir_all(dir.join("src/main/java/b")).unwrap();
    std::fs::write(dir.join("settings.gradle"), "rootProject.name = 'proj'\n").unwrap();
    std::fs::write(dir.join("build.gradle"), "apply plugin: 'java'\n").unwrap();
    std::fs::write(
        dir.join("src/main/java/b/Point.java"),
        "package b;\n\npublic class Point {\n    @Override\n    public boolean equals(Object o) {\n        return o == this;\n    }\n\n    @Override\n    public int hashCode() {\n        return 1;\n    }\n\n    @Override\n    public String toString() {\n        return \"p\";\n    }\n\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main/java/b/Main.java"),
        "package b;\n\npublic class Main {\n    public static void main(String[] args) {\n        new Point();\n    }\n}\n",
    )
    .unwrap();
    let cfg = ProjectConfig { index: Some(tmp.path().join("i.codescape")), ..Default::default() };
    let file = IndexFile::open(&index_project(&dir, &cfg, &|_| {}).unwrap().path).unwrap();
    let g = file.graph();
    let a = analyse(g, &dir);
    let of = |class: &str, name: &str| -> Vec<Usage> {
        let c = (0..g.node_count() as NodeId).find(|&n| g.name(n) == class && g.kind(n) == NodeKind::Type).unwrap();
        g.children(c).filter(|&m| g.name(m) == name).map(|m| a.usage[m as usize]).collect()
    };
    for m in ["equals", "hashCode", "toString"] {
        assert_eq!(of("Point", m), [Usage::Used], "{m}");
    }
}

#[test]
fn helpers_named_like_object_methods_but_taking_other_arguments_are_still_unused() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("proj");
    std::fs::create_dir_all(dir.join("src/main/java/b")).unwrap();
    std::fs::write(dir.join("settings.gradle"), "rootProject.name = 'proj'\n").unwrap();
    std::fs::write(dir.join("build.gradle"), "apply plugin: 'java'\n").unwrap();
    std::fs::write(
        dir.join("src/main/java/b/Util.java"),
        "package b;\n\npublic class Util {\n    public static boolean equals(Object a,\n            Object b) {\n        return a == b;\n    }\n}\n\nclass Text {\n    public String toString(int depth) {\n        return \"\";\n    }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main/java/b/Main.java"),
        "package b;\n\npublic class Main {\n    public static void main(String[] args) {\n        new Util();\n        new Text();\n    }\n}\n",
    )
    .unwrap();
    let cfg = ProjectConfig { index: Some(tmp.path().join("i.codescape")), ..Default::default() };
    let file = IndexFile::open(&index_project(&dir, &cfg, &|_| {}).unwrap().path).unwrap();
    let g = file.graph();
    let a = analyse(g, &dir);
    for name in ["equals", "toString"] {
        let n = (0..g.node_count() as NodeId).find(|&n| g.name(n) == name).unwrap();
        assert_ne!(a.usage[n as usize], Usage::Used, "{name}");
    }
}

#[test]
fn a_method_marked_as_an_override_is_used_even_without_a_declared_supertype() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("proj");
    std::fs::create_dir_all(dir.join("src/main/java/b")).unwrap();
    std::fs::write(dir.join("settings.gradle"), "rootProject.name = 'proj'\n").unwrap();
    std::fs::write(dir.join("build.gradle"), "apply plugin: 'java'\n").unwrap();
    std::fs::write(
        dir.join("src/main/java/b/Handler.java"),
        "package b;\n\npublic class Handler {\n    @Override\n    @Deprecated\n    protected Object handle(String event) {\n        return null;\n    }\n\n    protected void stale() {\n    }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main/java/b/Main.java"),
        "package b;\n\npublic class Main {\n    public static void main(String[] args) {\n        new Handler();\n    }\n}\n",
    )
    .unwrap();
    let cfg = ProjectConfig { index: Some(tmp.path().join("i.codescape")), ..Default::default() };
    let file = IndexFile::open(&index_project(&dir, &cfg, &|_| {}).unwrap().path).unwrap();
    let g = file.graph();
    let a = analyse(g, &dir);
    let named = |name: &str| (0..g.node_count() as NodeId).find(|&n| g.name(n) == name).unwrap_or_else(|| panic!("no {name}"));
    assert_eq!(a.usage[named("handle") as usize], Usage::Used);
    assert_ne!(a.usage[named("stale") as usize], Usage::Used);
}

#[test]
fn methods_javascript_calls_on_every_object_are_used() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("proj");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("package.json"), "{\"name\": \"proj\", \"main\": \"src/index.ts\"}\n").unwrap();
    std::fs::write(
        dir.join("src/money.ts"),
        "export class Money {\n  constructor(private v: number) {}\n  toJSON() {\n    return this.v;\n  }\n  toString() {\n    return String(this.v);\n  }\n  stale() {\n    return 1;\n  }\n}\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/index.ts"), "import { Money } from \"./money\";\nconsole.log(JSON.stringify(new Money(1)));\n").unwrap();
    let cfg = ProjectConfig { index: Some(tmp.path().join("i.codescape")), ..Default::default() };
    let file = IndexFile::open(&index_project(&dir, &cfg, &|_| {}).unwrap().path).unwrap();
    let g = file.graph();
    let a = analyse(g, &dir);
    let named = |name: &str| (0..g.node_count() as NodeId).find(|&n| g.name(n) == name).unwrap_or_else(|| panic!("no {name}"));
    assert_eq!(a.usage[named("toJSON") as usize], Usage::Used);
    assert_eq!(a.usage[named("toString") as usize], Usage::Used);
    assert_ne!(a.usage[named("stale") as usize], Usage::Used);
}
