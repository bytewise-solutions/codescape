use codescape::find::{Hit, find_in, hit_from, step_hit};
use codescape::search::{file_structure, filter_structure};
use codescape_format::*;

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_string).collect()
}

#[test]
fn finds_every_match_ignoring_case_with_char_columns() {
    let text = lines("let probe = Probe::new();\n// é probe\nnothing");
    let hits = find_in(&text, "probe");
    assert_eq!(hits, vec![Hit { line: 0, col: 4, len: 5 }, Hit { line: 0, col: 12, len: 5 }, Hit { line: 1, col: 5, len: 5 }]);
}

#[test]
fn an_uppercase_letter_makes_the_search_case_sensitive() {
    let text = lines("let probe = Probe::new();");
    assert_eq!(find_in(&text, "Probe"), vec![Hit { line: 0, col: 12, len: 5 }]);
}

#[test]
fn an_empty_query_finds_nothing_and_matches_do_not_overlap() {
    assert!(find_in(&lines("abc"), "").is_empty());
    assert_eq!(find_in(&lines("aaaa"), "aa").len(), 2);
}

#[test]
fn stepping_wraps_around_both_ends() {
    assert_eq!(step_hit(3, Some(2), true), Some(0));
    assert_eq!(step_hit(3, Some(0), false), Some(2));
    assert_eq!(step_hit(3, None, true), Some(0));
    assert_eq!(step_hit(0, None, true), None);
}

#[test]
fn the_first_hit_is_the_one_at_or_below_the_line_on_screen() {
    let hits = vec![Hit { line: 2, col: 0, len: 1 }, Hit { line: 10, col: 0, len: 1 }, Hit { line: 30, col: 0, len: 1 }];
    assert_eq!(hit_from(&hits, 5), Some(1));
    assert_eq!(hit_from(&hits, 10), Some(1));
    assert_eq!(hit_from(&hits, 31), Some(0), "wraps to the top past the last hit");
}

#[test]
fn the_structure_lists_the_file_members_in_source_order_and_filters_fuzzily() {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "tools", Lang::Java);
    let t = b.add_node(pkg, NodeKind::Type, SubKind::Class, "Probe", Lang::Java);
    b.set_span(t, "Probe.java", 1, 100);
    for (name, line) in [("render", 50), ("main", 10), ("renderScene", 70)] {
        let m = b.add_node(t, NodeKind::Member, SubKind::Method, name, Lang::Java);
        b.set_span(m, "Probe.java", line, line + 5);
    }
    let inner = b.add_node(t, NodeKind::Type, SubKind::Class, "Scene", Lang::Java);
    b.set_span(inner, "Probe.java", 80, 95);
    let other = b.add_node(pkg, NodeKind::Type, SubKind::Class, "Other", Lang::Java);
    b.set_span(other, "Other.java", 1, 9);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let t = g.children(g.children(g.root()).start).start;
    let all = file_structure(g, t);
    let names: Vec<&str> = all.iter().map(|&n| g.name(n)).collect();
    assert_eq!(names, ["main", "render", "renderScene", "Scene"]);
    let picked: Vec<&str> = filter_structure(g, &all, "rensc").iter().map(|&n| g.name(n)).collect();
    assert_eq!(picked, ["renderScene"]);
    assert_eq!(filter_structure(g, &all, "  ").len(), 4);
}
