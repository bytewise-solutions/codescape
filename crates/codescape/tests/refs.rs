use codescape::refs::*;
use codescape::source::SourceCache;
use codescape_format::*;

struct Fx {
    _dir: tempfile::TempDir,
    file: IndexFile,
}

const RAW: [&str; 4] = ["class A {", "\tvoid go() {", "\t\tString é = get(); put();", "\t}"];

fn fixture(extra: impl FnOnce(&mut GraphBuilder, [NodeId; 5])) -> Fx {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "p", Lang::Java);
    let a = b.add_node(pkg, NodeKind::Type, SubKind::Class, "A", Lang::Java);
    b.set_span(a, "A.java", 1, 4);
    let go = b.add_node(a, NodeKind::Member, SubKind::Method, "go", Lang::Java);
    b.set_span(go, "A.java", 2, 4);
    let o = b.add_node(pkg, NodeKind::Type, SubKind::Class, "O", Lang::Java);
    b.set_span(o, "O.java", 1, 9);
    let get = b.add_node(o, NodeKind::Member, SubKind::Method, "get", Lang::Java);
    b.set_span(get, "O.java", 2, 3);
    let put = b.add_node(o, NodeKind::Member, SubKind::Method, "put", Lang::Java);
    b.set_span(put, "O.java", 5, 6);
    b.add_ref(go, get, 3, 14, 3);
    b.add_ref(go, put, 3, 21, 3);
    b.add_ref(a, o, 1, 0, 5);
    extra(&mut b, [a, go, o, get, put]);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let file = IndexFile::open(&dir.path().join("g")).unwrap();
    Fx { _dir: dir, file }
}

fn node(g: &ArchivedGraph, path: &[&str]) -> NodeId {
    path.iter().fold(g.root(), |n, p| g.children(n).find(|&c| g.name(c) == *p).unwrap_or_else(|| panic!("no {p}")))
}

fn raw() -> Vec<String> {
    RAW.iter().map(|l| l.to_string()).collect()
}

#[test]
fn a_span_is_measured_in_drawn_characters_with_tabs_expanded() {
    assert_eq!(display_span("\tString é = foo();", 13, 3), (15, 3));
    assert_eq!(display_span("a\tb", 0, 3), (0, 6));
    assert_eq!(display_span("short", 40, 3), (5, 0), "past the end of the line");
}

#[test]
fn the_token_under_a_column_names_its_target() {
    let fx = fixture(|_, _| {});
    let g = fx.file.graph();
    let a = node(g, &["p", "A"]);
    let refs = FileRefs::new(g, a, &raw());
    let (get, put, o) = (node(g, &["p", "O", "get"]), node(g, &["p", "O", "put"]), node(g, &["p", "O"]));
    assert_eq!(refs.at(3, 19), Some((19, 3, get)), "tabs and é before the name");
    assert_eq!(refs.at(3, 21), Some((19, 3, get)));
    assert_eq!(refs.at(3, 22), None, "one column after the name");
    assert_eq!(refs.at(3, 18), None);
    assert_eq!(refs.at(3, 26), Some((26, 3, put)), "second name on the line");
    assert_eq!(refs.at(1, 2), Some((0, 5, o)), "refs from the type itself");
    assert_eq!(refs.at(2, 5), None);
    assert_eq!(refs.at(9, 0), None);
}

#[test]
fn a_token_with_two_targets_is_not_a_link() {
    let fx = fixture(|b, [_, go, o, _, _]| b.add_ref(go, o, 3, 14, 3));
    let g = fx.file.graph();
    let refs = FileRefs::new(g, node(g, &["p", "A"]), &raw());
    assert_eq!(refs.at(3, 20), None);
    assert!(refs.at(3, 26).is_some());
}

#[test]
fn a_target_without_code_is_not_a_link() {
    let fx = fixture(|b, [_, go, ..]| {
        let ext = b.add_node(b.root(), NodeKind::External, SubKind::None, "java.lang.String", Lang::Java);
        b.add_ref(go, ext, 3, 2, 6);
    });
    let g = fx.file.graph();
    let refs = FileRefs::new(g, node(g, &["p", "A"]), &raw());
    assert_eq!(refs.at(3, 9), None);
}

#[test]
fn raw_lines_keep_their_tabs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.go"), "func A() {\n\treturn \n}\n").unwrap();
    let mut cache = SourceCache::new(dir.path());
    assert_eq!(cache.raw("a.go").unwrap()[1], "\treturn ");
    assert_eq!(cache.file("a.go").unwrap()[1], "    return");
    assert!(cache.raw("missing.go").is_none());
}

#[test]
fn columns_are_as_wide_as_the_drawn_monospace_glyphs() {
    let ctx = egui::Context::default();
    let _ = ctx.run_ui(Default::default(), |ui| {
        for size in [7.0, 13.0, 16.25, 32.0] {
            let line = ui.painter().layout_no_wrap("é".repeat(120), egui::FontId::monospace(size), egui::Color32::WHITE);
            let drift = line.size().x - 120.0 * codescape::source::advance(size);
            assert!(drift.abs() < 0.5, "{size} pt drifts {drift} px over 120 columns");
        }
    });
}
