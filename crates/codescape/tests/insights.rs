use codescape::insights::*;
use codescape::review_view::Limits;
use codescape_format::*;
use codescape_index::analysis::{Analysis, Usage};
use codescape_layout::LayoutOptions;
use std::collections::HashMap;

struct Fx {
    _dir: tempfile::TempDir,
    file: IndexFile,
}

fn method(b: &mut GraphBuilder, owner: NodeId, name: &str, file: &str, lines: (u32, u32), vis: Visibility) -> NodeId {
    let m = b.add_node(owner, NodeKind::Member, SubKind::Method, name, Lang::Java);
    b.set_span(m, file, lines.0, lines.1);
    b.set_visibility(m, vis);
    m
}

fn fixture() -> Fx {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "shop", Lang::Java);
    let svc = b.add_node(pkg, NodeKind::Type, SubKind::Class, "Svc", Lang::Java);
    b.set_span(svc, "Svc.java", 1, 200);
    b.add_tag(svc, "layer:service");
    let total = method(&mut b, svc, "total", "Svc.java", (10, 57), Visibility::Public);
    b.set_complexity_detail(total, 14, &[("if", 6), ("for", 2), ("&&", 4), ("catch", 1)], 4);
    let simple = method(&mut b, svc, "simple", "Svc.java", (60, 62), Visibility::Public);
    let helper = method(&mut b, svc, "helper", "Svc.java", (70, 90), Visibility::Private);
    b.set_complexity_detail(helper, 12, &[("if", 11)], 2);
    let repo = b.add_node(pkg, NodeKind::Type, SubKind::Class, "Repo", Lang::Java);
    b.set_span(repo, "Repo.java", 1, 50);
    b.add_tag(repo, "layer:repository");
    let load = method(&mut b, repo, "load", "Repo.java", (5, 40), Visibility::Public);
    b.set_complexity_detail(load, 18, &[("if", 17)], 3);
    b.add_edge(load, simple, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let cart = b.add_node(pkg, NodeKind::Type, SubKind::Class, "Cart", Lang::Java);
    b.set_span(cart, "Cart.java", 1, 100);
    let sum = method(&mut b, cart, "sum", "Cart.java", (40, 72), Visibility::Public);
    let other = b.add_node(pkg, NodeKind::Type, SubKind::Class, "Other", Lang::Java);
    b.set_span(other, "Other.java", 1, 30);
    let copy = method(&mut b, other, "copy", "Other.java", (2, 30), Visibility::Private);
    let tests = b.add_node(pkg, NodeKind::Type, SubKind::Class, "SvcTest", Lang::Java);
    b.set_span(tests, "SvcTest.java", 1, 80);
    b.add_flags(tests, NodeFlags::TEST);
    let check = method(&mut b, tests, "check", "SvcTest.java", (3, 70), Visibility::Public);
    b.set_complexity_detail(check, 30, &[("if", 29)], 1);
    b.add_edge(check, simple, EdgeKind::Calls, Scope::Test, Confidence::Resolved, EdgeFlags::NONE);
    b.add_clone_group(61, &[(sum, 40, 72), (total, 10, 57), (copy, 2, 30)]);
    b.add_clone_group(55, &[(check, 3, 20), (helper, 70, 87)]);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let file = IndexFile::open(&dir.path().join("g")).unwrap();
    Fx { _dir: dir, file }
}

fn node(g: &ArchivedGraph, path: &str) -> NodeId {
    path.split('.').fold(g.root(), |n, p| g.children(n).find(|&c| g.name(c) == p).unwrap_or_else(|| panic!("no {p}")))
}

fn analysis(g: &ArchivedGraph) -> Analysis {
    let count = g.node_count();
    let mut a = Analysis { usage: vec![Usage::Used; count], tested: vec![false; count], callers: vec![0; count] };
    a.usage[node(g, "shop.Svc.helper") as usize] = Usage::Unused;
    a.usage[node(g, "shop.Cart.sum") as usize] = Usage::NoCallerInProject;
    a.usage[node(g, "shop.Svc.simple") as usize] = Usage::OnlyTests;
    a.usage[node(g, "shop.SvcTest.check") as usize] = Usage::Unused;
    for t in ["shop.Svc.simple", "shop.Svc"] {
        a.tested[node(g, t) as usize] = true;
    }
    for (n, c) in [("shop.Svc", 9), ("shop.Svc.simple", 7), ("shop.Repo", 6), ("shop.Cart", 2), ("shop.SvcTest", 12)] {
        a.callers[node(g, n) as usize] = c;
    }
    a
}

fn churn() -> HashMap<String, u32> {
    HashMap::from([("Repo.java".into(), 31), ("Svc.java".into(), 3), ("SvcTest.java".into(), 50)])
}

fn names(g: &ArchivedGraph, rows: &[Row]) -> Vec<String> {
    rows.iter().map(|r| g.path(r.node).iter().skip(2).map(|&p| g.name(p)).collect::<Vec<_>>().join(".")).collect()
}

fn all(g: &ArchivedGraph, scope: Option<NodeId>) -> Insights {
    compute(g, &analysis(g), &churn(), &LayoutOptions::default(), &Limits::default(), scope)
}

fn flags_of(g: &ArchivedGraph, path: &str) -> Vec<Flag> {
    flags(g, &analysis(g), &churn(), &LayoutOptions::default(), &Limits::default(), node(g, path))
}

fn flag(g: &ArchivedGraph, path: &str, kind: FlagKind) -> Flag {
    flags_of(g, path).into_iter().find(|f| f.kind == kind).unwrap_or_else(|| panic!("{path} has no {kind:?} flag"))
}

#[test]
fn each_list_is_ranked_by_its_score() {
    let fx = fixture();
    let g = fx.file.graph();
    let i = all(g, None);
    assert_eq!(names(g, &i.complex), ["Repo.load", "Svc.total", "Svc.helper"]);
    assert_eq!(i.complex.iter().map(|r| r.score).collect::<Vec<_>>(), [18, 14, 12]);
    assert_eq!(names(g, &i.hotspots), ["Repo.load", "Svc.total", "Svc.helper"]);
    assert_eq!(i.hotspots.iter().map(|r| r.score).collect::<Vec<_>>(), [31 * 18, 3 * 14, 3 * 12]);
    assert_eq!(i.hotspots[0].reason, "changed 31× · complexity 18");
    assert_eq!(names(g, &i.depended), ["Svc", "Svc.simple", "Repo"]);
    assert_eq!(names(g, &i.upward), ["Repo"]);
    assert!(i.upward[0].reason.contains("Svc"), "{}", i.upward[0].reason);
    assert_eq!(names(g, &i.dead), ["Cart.sum", "Svc.helper", "Svc.simple"], "larger dead code first");
    assert_eq!(names(g, &i.untested), ["Repo.load", "Svc.total", "Cart.sum"], "riskier public code first");
    assert_eq!(names(g, &i.duplicates), ["Cart.sum", "Svc.total", "Other.copy"]);
    assert_eq!(i.duplicates[0].score, 61);
}

#[test]
fn a_scope_keeps_only_its_subtree() {
    let fx = fixture();
    let g = fx.file.graph();
    let i = all(g, Some(node(g, "shop.Repo")));
    assert_eq!(names(g, &i.complex), ["Repo.load"]);
    assert_eq!(names(g, &i.hotspots), ["Repo.load"]);
    assert_eq!(names(g, &i.depended), ["Repo"]);
    assert_eq!(names(g, &i.upward), ["Repo"]);
    assert!(i.dead.is_empty() && i.duplicates.is_empty());
    assert!(all(g, Some(node(g, "shop.Cart"))).upward.is_empty());
}

#[test]
fn tests_hidden_by_default_are_never_listed_or_flagged() {
    let fx = fixture();
    let g = fx.file.graph();
    let i = all(g, None);
    for list in [&i.complex, &i.hotspots, &i.depended, &i.upward, &i.dead, &i.untested, &i.duplicates] {
        assert!(!names(g, list).iter().any(|n| n.starts_with("SvcTest")), "{:?}", names(g, list));
    }
    assert!(!names(g, &i.duplicates).contains(&"Svc.helper".to_string()), "its only copy is in hidden test code");
    assert!(flags_of(g, "shop.SvcTest.check").is_empty());
    let shown = LayoutOptions { show_tests: true, ..Default::default() };
    let with = compute(g, &analysis(g), &churn(), &shown, &Limits::default(), None);
    assert_eq!(names(g, &with.complex)[0], "SvcTest.check");
}

#[test]
fn a_complex_flag_names_its_decisions_nesting_length_and_threshold() {
    let fx = fixture();
    let g = fx.file.graph();
    let f = flag(g, "shop.Svc.total", FlagKind::Complex);
    assert_eq!(f.explanation, "Complexity 14 (flagged from 10): 13 decision points — if ×6, for ×2, && ×4, catch ×1; nesting 4; 48 lines.");
    assert_eq!(f.short, "complexity 14");
    assert!(!flags_of(g, "shop.Svc.simple").iter().any(|f| f.kind == FlagKind::Complex), "complexity 1");
}

#[test]
fn a_hotspot_flag_names_its_commits_period_and_complexity() {
    let fx = fixture();
    let g = fx.file.graph();
    let f = flag(g, "shop.Repo.load", FlagKind::Hotspot);
    assert_eq!(f.explanation, "Changed in 31 commits in the last 365 days and complexity 18: a frequent, risky place to edit.");
    let shorter = Limits { hotspot_days: 90, ..Limits::default() };
    let f = flags(g, &analysis(g), &churn(), &LayoutOptions::default(), &shorter, node(g, "shop.Repo.load"));
    assert!(f.iter().any(|f| f.explanation.contains("last 90 days")));
}

#[test]
fn fewer_than_three_commits_make_no_hotspot() {
    let fx = fixture();
    let g = fx.file.graph();
    let few = HashMap::from([("Repo.java".to_string(), 2), ("Svc.java".to_string(), 3)]);
    let i = compute(g, &analysis(g), &few, &LayoutOptions::default(), &Limits::default(), None);
    assert_eq!(names(g, &i.hotspots), ["Svc.total", "Svc.helper"]);
    let f = flags(g, &analysis(g), &few, &LayoutOptions::default(), &Limits::default(), node(g, "shop.Repo.load"));
    assert!(!f.iter().any(|f| f.kind == FlagKind::Hotspot));
}

#[test]
fn usage_flags_say_why_the_code_looks_dead() {
    let fx = fixture();
    let g = fx.file.graph();
    assert_eq!(
        flag(g, "shop.Svc.helper", FlagKind::Unused).explanation,
        "Unused: its name appears nowhere else in the sources; reflection or a framework could still use it."
    );
    assert_eq!(
        flag(g, "shop.Cart.sum", FlagKind::NoCaller).explanation,
        "No caller in the project: its name appears nowhere else in the sources; reflection or a framework could still use it."
    );
    assert_eq!(flag(g, "shop.Svc.simple", FlagKind::OnlyTests).explanation, "Only tests use it: no main code calls it or names it.");
}

#[test]
fn an_untested_flag_is_for_public_code_no_test_reaches() {
    let fx = fixture();
    let g = fx.file.graph();
    assert_eq!(flag(g, "shop.Svc.total", FlagKind::Untested).explanation, "Public, and no test reaches it.");
    assert!(!flags_of(g, "shop.Svc.helper").iter().any(|f| f.kind == FlagKind::Untested), "private");
    assert!(!flags_of(g, "shop.Svc.simple").iter().any(|f| f.kind == FlagKind::Untested), "tested");
}

#[test]
fn a_duplicate_flag_names_the_other_copies_and_links_them() {
    let fx = fixture();
    let g = fx.file.graph();
    let f = flag(g, "shop.Cart.sum", FlagKind::Duplicate);
    assert_eq!(f.explanation, "Same code as `Svc.total` (lines 10–57) and 1 other place: 61 tokens.");
    assert_eq!(f.links, [node(g, "shop.Svc.total"), node(g, "shop.Other.copy")]);
    assert!(!flags_of(g, "shop.Svc.helper").iter().any(|f| f.kind == FlagKind::Duplicate), "the only copy is hidden test code");
}

#[test]
fn an_upward_flag_names_the_higher_layer_and_links_it() {
    let fx = fixture();
    let g = fx.file.graph();
    let f = flag(g, "shop.Repo", FlagKind::Upward);
    assert_eq!(f.explanation, "Depends on `Svc`, which sits in a higher layer: this arrow points up and breaks the layering.");
    assert_eq!(f.links, [node(g, "shop.Svc")]);
    assert!(!flags_of(g, "shop.Svc").iter().any(|f| f.kind == FlagKind::Upward));
}

#[test]
fn insights_are_recomputed_only_when_an_input_changes() {
    let fx = fixture();
    let g = fx.file.graph();
    let key = || InsightsKey { analysis: 1, churn: 0, opts: LayoutOptions::default(), limits: Limits::default(), scope: None };
    let mut cache = InsightsCache::default();
    let runs = std::cell::Cell::new(0);
    let get = |cache: &mut InsightsCache, key: InsightsKey| {
        cache.get(key, || {
            runs.set(runs.get() + 1);
            all(g, None)
        });
    };
    get(&mut cache, key());
    get(&mut cache, key());
    assert_eq!(runs.get(), 1);
    get(&mut cache, InsightsKey { churn: 7, ..key() });
    assert_eq!(runs.get(), 2, "churn arrived");
    get(&mut cache, InsightsKey { churn: 7, scope: Some(3), ..key() });
    assert_eq!(runs.get(), 3, "scope changed");
    get(&mut cache, InsightsKey { churn: 7, scope: Some(3), limits: Limits { many_callers: 9, ..Limits::default() }, ..key() });
    assert_eq!(runs.get(), 4, "threshold changed");
    let opts = LayoutOptions { hidden: NodeFlags::NONE, ..LayoutOptions::default() };
    get(&mut cache, InsightsKey { churn: 7, scope: Some(3), limits: Limits { many_callers: 9, ..Limits::default() }, opts, ..key() });
    assert_eq!(runs.get(), 5, "layout options changed");
}

#[test]
fn a_blocks_flags_are_recomputed_only_when_the_block_or_an_input_changes() {
    let key = |scope| InsightsKey { analysis: 1, churn: 0, opts: LayoutOptions::default(), limits: Limits::default(), scope };
    let mut cache = FlagsCache::default();
    let runs = std::cell::Cell::new(0);
    let mut get = |k: InsightsKey| {
        let flag = Flag { kind: FlagKind::Untested, short: "s".into(), explanation: "e".into(), links: Vec::new() };
        cache
            .get(k, || {
                runs.set(runs.get() + 1);
                vec![flag]
            })
            .len()
    };
    assert_eq!(get(key(Some(1))), 1);
    get(key(Some(1)));
    assert_eq!(runs.get(), 1);
    get(key(Some(2)));
    assert_eq!(runs.get(), 2, "another block");
    get(InsightsKey { churn: 5, ..key(Some(2)) });
    assert_eq!(runs.get(), 3, "churn arrived");
}

fn marks_of(g: &ArchivedGraph, path: &str) -> Vec<(u32, Vec<FlagKind>)> {
    let a = analysis(g);
    file_marks(g, &a, &churn(), &LayoutOptions::default(), &Limits::default(), node(g, path)).iter().map(|m| (m.line, m.kinds())).collect()
}

#[test]
fn flagged_members_mark_their_first_line_with_one_icon_per_kind_in_kind_order() {
    let fx = fixture();
    let g = fx.file.graph();
    let marks = marks_of(g, "shop.Svc");
    let lines: Vec<u32> = marks.iter().map(|m| m.0).collect();
    assert_eq!(lines, [10, 60, 70], "total, simple and helper; the unflagged type itself has no mark");
    assert_eq!(marks[0].1, [FlagKind::Complex, FlagKind::Hotspot, FlagKind::Untested, FlagKind::Duplicate]);
    assert_eq!(marks[1].1, [FlagKind::OnlyTests]);
    assert_eq!(marks[2].1, [FlagKind::Complex, FlagKind::Hotspot, FlagKind::Unused]);
}

#[test]
fn marks_on_one_line_merge_and_keep_every_flag_for_the_tooltip() {
    let flag = |kind, short: &str| Flag { kind, short: short.into(), explanation: format!("why {short}"), links: Vec::new() };
    let marks = gutter_marks([
        (7, vec![flag(FlagKind::Unused, "a"), flag(FlagKind::Complex, "b")]),
        (3, vec![]),
        (7, vec![flag(FlagKind::Complex, "c")]),
        (2, vec![flag(FlagKind::Upward, "d")]),
    ]);
    assert_eq!(marks.iter().map(|m| m.line).collect::<Vec<_>>(), [2, 7]);
    assert_eq!(marks[1].kinds(), [FlagKind::Complex, FlagKind::Unused]);
    assert_eq!(marks[1].flags.iter().map(|f| f.short.as_str()).collect::<Vec<_>>(), ["b", "c", "a"]);
}

#[test]
fn the_worst_complexity_climbs_to_every_shown_ancestor_and_skips_hidden_code() {
    let fx = fixture();
    let g = fx.file.graph();
    let worst = worst_complexity(g, &LayoutOptions::default());
    assert_eq!(worst.get(&node(g, "shop.Svc.total")), Some(&14));
    assert_eq!(worst.get(&node(g, "shop.Svc")), Some(&14));
    assert_eq!(worst.get(&node(g, "shop.Repo")), Some(&18));
    assert_eq!(worst.get(&node(g, "shop")), Some(&18), "the test's 30 is hidden by default");
    assert_eq!(worst.get(&g.root()), Some(&18));
    assert!(!worst.contains_key(&node(g, "shop.SvcTest")));
    let shown = worst_complexity(
        g,
        &LayoutOptions { show_tests: true, hidden: NodeFlags(LayoutOptions::default().hidden.0 & !NodeFlags::TEST.0), ..LayoutOptions::default() },
    );
    assert_eq!(shown.get(&node(g, "shop")), Some(&30));
}

#[test]
fn a_duplicate_with_the_same_name_says_which_file_it_is_in() {
    let mut b = GraphBuilder::new("p");
    let (one, two) =
        (b.add_node(b.root(), NodeKind::Package, SubKind::None, "one", Lang::Java), b.add_node(b.root(), NodeKind::Package, SubKind::None, "two", Lang::Java));
    let mut copies = Vec::new();
    for (pkg, file) in [(one, "one/Res.java"), (two, "two/Res.java")] {
        let t = b.add_node(pkg, NodeKind::Type, SubKind::Class, "Res", Lang::Java);
        b.set_span(t, file, 1, 40);
        copies.push((method(&mut b, t, "node", file, (5, 30), Visibility::Public), 5, 30));
    }
    b.add_clone_group(80, &copies);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let file = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = file.graph();
    let a = Analysis { usage: vec![Usage::Used; g.node_count()], tested: vec![false; g.node_count()], callers: vec![0; g.node_count()] };
    let all = flags(g, &a, &HashMap::new(), &LayoutOptions::default(), &Limits::default(), node(g, "one.Res.node"));
    let f = all.iter().find(|f| f.kind == FlagKind::Duplicate).unwrap();
    assert_eq!(f.explanation, "Same code as `Res.node` in two/Res.java (lines 5–30): 80 tokens.");
}
