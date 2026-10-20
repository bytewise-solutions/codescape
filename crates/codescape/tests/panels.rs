use codescape::panels::group_entries;
use codescape_format::*;
use codescape_rules::{PackSelection, load_packs};

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
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let vis = load_packs(None, None, &PackSelection::default()).unwrap().visual();
    let groups = group_entries(g, &vis);
    let labels: Vec<&str> = groups.iter().map(|gr| gr.label.as_str()).collect();
    assert_eq!(labels, ["Applications", "Dev tools"]);
    assert!(!groups[0].collapsed && groups[1].collapsed);
    let desktop = &groups[0].items[0];
    assert_eq!(g.entry_label(desktop.entry as usize), "Desktop");
    let kids: Vec<&str> = desktop.children.iter().map(|&c| g.entry_label(c as usize)).collect();
    assert_eq!(kids, ["create()", "render()"]);
}

#[test]
fn entries_skip_tests() {
    let mut b = GraphBuilder::new("p");
    let m = b.add_node(b.root(), NodeKind::Module, SubKind::None, "m", Lang::Java);
    let n = b.add_node(m, NodeKind::Member, SubKind::Method, "main", Lang::Java);
    for (label, category) in [("App", Category::Application), ("LoadTest", Category::Test), ("Tool", Category::DevTool)] {
        b.add_entry(EntryPointSpec {
            node: n,
            kind: "main".into(),
            category,
            label: label.into(),
            parent: None,
            lifecycle: Lifecycle::Once,
            order: 0,
            rule: "r".into(),
            handoff: None,
        });
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let vis = load_packs(None, None, &PackSelection::default()).unwrap().visual();
    let groups = group_entries(g, &vis);
    let all: Vec<&str> = groups.iter().flat_map(|gr| gr.items.iter().map(|i| g.entry_label(i.entry as usize))).collect();
    assert_eq!(all, ["App", "Tool"]);
}

#[test]
fn panel_text_size_is_remembered_and_kept_in_range() {
    use codescape::panels::Prefs;
    assert_eq!(Prefs::parse("").panel_text, 1.0, "a missing file keeps the usual size");
    let p = Prefs { panel_text: 1.4, ..Prefs::default() };
    assert_eq!(Prefs::parse(&p.to_toml()).panel_text, 1.4);
    assert_eq!(Prefs::parse("panel_text = 9.0").panel_text, 2.0);
    assert_eq!(Prefs::parse("panel_text = 0.1").panel_text, 0.8);
    assert_eq!(Prefs::parse("panel_text = nan").panel_text, 1.0);
}

#[test]
fn lists_open_by_default_only_up_to_the_chosen_size() {
    use codescape::panels::Prefs;
    let p = Prefs::parse("");
    assert_eq!(p.open_lists_up_to, 0, "everything starts collapsed");
    assert!(!p.opens(1));
    let p = Prefs::parse("open_lists_up_to = 12");
    assert!(p.opens(12) && !p.opens(13));
    assert!(!p.opens(0), "an empty list has nothing to show");
    assert_eq!(Prefs::parse("open_lists_up_to = 100000").open_lists_up_to, 500);
    assert_eq!(Prefs::parse(&p.to_toml()).open_lists_up_to, 12);
}
