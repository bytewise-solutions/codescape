use srcrelief::panels::group_entries;
use srcrelief_format::*;
use srcrelief_rules::{PackSelection, load_packs};

#[test]
fn entries_grouped_in_visual_order_with_children_nested() {
    let mut b = GraphBuilder::new("p");
    let m = b.add_node(b.root(), NodeKind::Module, SubKind::None, "m", Lang::Java);
    let n = b.add_node(m, NodeKind::Member, SubKind::Method, "f", Lang::Java);
    let spec = |label: &str, kind: &str, category: Category, parent: Option<u32>, order: u32| EntryPointSpec {
        node: n,
        kind: kind.into(),
        category,
        label: label.into(),
        parent,
        lifecycle: Lifecycle::Once,
        order,
        rule: "r".into(),
        handoff: None,
    };
    b.add_entry(spec("Tool", "main", Category::DevTool, None, 0));
    let app = b.add_entry(spec("Desktop", "main", Category::Application, None, 0));
    b.add_entry(spec("render()", "framework-callback", Category::Application, Some(app), 3));
    b.add_entry(spec("create()", "framework-callback", Category::Application, Some(app), 1));
    b.add_entry(spec("LoadTest", "main", Category::Test, None, 0));
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let vis = load_packs(None, None, &PackSelection::default()).unwrap().visual();
    let groups = group_entries(g, &vis);
    let labels: Vec<&str> = groups.iter().map(|gr| gr.label.as_str()).collect();
    assert_eq!(labels, ["Applications", "Dev tools", "Tests"]);
    assert!(!groups[0].collapsed && groups[1].collapsed);
    let desktop = &groups[0].items[0];
    assert_eq!(g.entry_label(desktop.entry as usize), "Desktop");
    let kids: Vec<&str> = desktop.children.iter().map(|&c| g.entry_label(c as usize)).collect();
    assert_eq!(kids, ["create()", "render()"]);
}
