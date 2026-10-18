use codescape_format::*;
use codescape_layout::*;
use codescape_rules::FrameRule;
use std::collections::HashMap;

struct G {
    _dir: tempfile::TempDir,
    file: IndexFile,
    ids: HashMap<String, NodeId>,
}

fn graph(modules: &[&str], deps: &[(&str, &str, Scope)]) -> G {
    let mut b = GraphBuilder::new("p");
    let mut ids = HashMap::new();
    for m in modules {
        let id = b.add_node(b.root(), NodeKind::Module, SubKind::None, m, Lang::Java);
        let pkg = b.add_node(id, NodeKind::Package, SubKind::None, "p", Lang::Java);
        let t = b.add_node(pkg, NodeKind::Type, SubKind::Class, "T", Lang::Java);
        b.set_span(t, "f", 1, 100);
        b.set_lines(t, 100);
        ids.insert(m.to_string(), id);
    }
    for (a, c, scope) in deps {
        b.add_edge(ids[*a], ids[*c], EdgeKind::DependsOn, *scope, Confidence::Resolved, EdgeFlags::NONE);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.codescape");
    write_graph(&b.finish(), &path).unwrap();
    let file = IndexFile::open(&path).unwrap();
    let g = file.graph();
    let ids = g.children(g.root()).map(|c| (g.name(c).to_string(), c)).collect();
    G { _dir: dir, file, ids }
}

fn main_deps(pairs: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str, Scope)> {
    pairs.iter().map(|(a, b)| (*a, *b, Scope::Main)).collect()
}

fn layer_of(l: &ContainerLayout, n: NodeId) -> u32 {
    l.children.iter().find(|c| c.node == n).unwrap().layer
}

#[test]
fn layers_follow_longest_path() {
    let g = graph(
        &["android", "bot", "client", "core", "desktop"],
        &main_deps(&[("bot", "desktop"), ("desktop", "client"), ("client", "core"), ("android", "client")]),
    );
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    let at = |n: &str| layer_of(&l, g.ids[n]);
    assert_eq!((at("bot"), at("android"), at("desktop"), at("client"), at("core")), (0, 0, 1, 2, 3));
    assert!(l.edges.iter().all(|e| e.dir == Direction::Down));
    let rect = |n: &str| l.children.iter().find(|c| c.node == g.ids[n]).unwrap().rect;
    assert!(rect("bot").y < rect("desktop").y && rect("client").y < rect("core").y);
}

#[test]
fn test_edges_excluded_by_default() {
    let g = graph(&["core", "navigation"], &[("navigation", "core", Scope::Main), ("core", "navigation", Scope::Test)]);
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    assert_eq!(l.edges.len(), 1);
    assert!(l.edges.iter().all(|e| e.dir == Direction::Down));
    let with = layout_container(gr, gr.root(), &[], &LayoutOptions { include_test_edges: true, ..Default::default() });
    assert_eq!(with.edges.iter().filter(|e| e.dir == Direction::Up).count(), 1);
    assert!(with.edges.iter().any(|e| e.test_only));
}

#[test]
fn cycle_breaks_and_marks_up() {
    let g = graph(&["a", "b", "c"], &main_deps(&[("a", "b"), ("b", "c"), ("c", "a")]));
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    assert_eq!(l.edges.iter().filter(|e| e.dir == Direction::Up).count(), 1);
    assert_eq!(l.children.len(), 3);
}

#[test]
fn hidden_children_are_left_out() {
    let mut b = GraphBuilder::new("p");
    let a = b.add_node(b.root(), NodeKind::Module, SubKind::None, "a", Lang::Java);
    let ext = b.add_node(b.root(), NodeKind::Module, SubKind::None, "(external)", Lang::None);
    b.add_flags(ext, NodeFlags::EXTERNAL);
    b.add_edge(a, ext, EdgeKind::DependsOn, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let l = layout_container(g, g.root(), &[], &LayoutOptions::default());
    assert_eq!(l.children.len(), 1);
    assert!(l.edges.is_empty());
}

#[test]
fn auto_prefix_frames() {
    let g = graph(
        &["client", "core", "server-core", "server-game", "server-world"],
        &main_deps(&[("server-game", "server-core"), ("server-world", "server-core"), ("server-core", "core"), ("client", "core")]),
    );
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[FrameRule::AutoPrefix { sep: "-".into() }], &LayoutOptions::default());
    assert!(!l.frames.is_empty() && l.frames.iter().all(|f| f.label == "server"));
    for m in ["server-core", "server-game", "server-world"] {
        let r = l.children.iter().find(|c| c.node == g.ids[m]).unwrap().rect;
        assert!(l.frames.iter().any(|f| f.rect.contains_rect(&r)), "{m} outside every frame");
    }
    let client = l.children.iter().find(|c| c.node == g.ids["client"]).unwrap().rect;
    assert!(l.frames.iter().all(|f| !f.rect.contains_rect(&client)));
    let pattern = layout_container(gr, gr.root(), &[FrameRule::Pattern { glob: "c*".into(), label: "c-things".into() }], &LayoutOptions::default());
    assert_eq!(pattern.frames[0].label, "c-things");
}

#[test]
fn boxes_do_not_overlap_and_fit_size() {
    let g = graph(&["a", "b", "c", "d", "e", "f"], &main_deps(&[("a", "b"), ("a", "c"), ("b", "d"), ("c", "d"), ("e", "f")]));
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    for (i, x) in l.children.iter().enumerate() {
        assert!(x.rect.x >= 0.0 && x.rect.y >= l.header && x.rect.right() <= l.size[0] && x.rect.bottom() <= l.size[1]);
        for y in &l.children[i + 1..] {
            assert!(!x.rect.intersects(&y.rect), "{:?} overlaps {:?}", x, y);
        }
    }
}

#[test]
fn deterministic() {
    let g = graph(&["a", "b", "c", "d"], &main_deps(&[("a", "b"), ("c", "b"), ("b", "d")]));
    let gr = g.file.graph();
    let one = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    let two = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    assert_eq!(one, two);
}

#[test]
fn cache_reuses_layouts() {
    let g = graph(&["a", "b"], &main_deps(&[("a", "b")]));
    let gr = g.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let first = cache.get_or_compute(gr, gr.root()).clone();
    assert_eq!(cache.computed(), 1);
    assert_eq!(cache.get_or_compute(gr, gr.root()), &first);
    assert_eq!(cache.computed(), 1);
    cache.invalidate_all();
    cache.get_or_compute(gr, gr.root());
    assert_eq!(cache.computed(), 2);
}

#[test]
#[cfg_attr(debug_assertions, ignore)]
fn large_flat_container_under_budget() {
    let mut b = GraphBuilder::new("p");
    let m = b.add_node(b.root(), NodeKind::Module, SubKind::None, "m", Lang::Java);
    let pkg = b.add_node(m, NodeKind::Package, SubKind::None, "big", Lang::Java);
    let types: Vec<NodeId> = (0..2000)
        .map(|i| {
            let t = b.add_node(pkg, NodeKind::Type, SubKind::Class, &format!("T{i}"), Lang::Java);
            b.set_span(t, "f", 1, 10 + (i % 300));
            t
        })
        .collect();
    let mut x: u64 = 7;
    for _ in 0..6000 {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let a = types[(x >> 33) as usize % 2000];
        let c = types[(x >> 13) as usize % 2000];
        b.add_edge(a, c, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let pkg = g.children(g.children(g.root()).start).start;
    let timed = || {
        let t = std::time::Instant::now();
        let l = layout_container(g, pkg, &[], &LayoutOptions::default());
        (t.elapsed(), l)
    };
    let (elapsed, l) = (0..3).map(|_| timed()).min_by_key(|(e, _)| *e).unwrap();
    assert_eq!(l.children.len(), 2000);
    assert!(elapsed.as_millis() < 100, "took {elapsed:?}");
    let aspect = l.size[0] / l.size[1];
    assert!((0.3..4.0).contains(&aspect), "aspect {aspect}");
}

#[test]
fn frames_never_cover_non_members() {
    let g = graph(
        &["android", "client", "core", "desktop", "server-chat", "server-core", "server-game", "server-world"],
        &main_deps(&[
            ("server-game", "server-core"),
            ("server-world", "server-core"),
            ("server-chat", "server-core"),
            ("server-core", "core"),
            ("desktop", "client"),
            ("android", "client"),
            ("client", "core"),
        ]),
    );
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[FrameRule::AutoPrefix { sep: "-".into() }], &LayoutOptions::default());
    assert!(!l.frames.is_empty());
    for f in &l.frames {
        assert_eq!(f.label, "server");
        for c in &l.children {
            let member = gr.name(c.node).starts_with("server-");
            if !member {
                assert!(!f.rect.intersects(&c.rect), "frame covers {}", gr.name(c.node));
            }
        }
    }
    for m in ["server-chat", "server-core", "server-game", "server-world"] {
        let r = l.children.iter().find(|c| c.node == g.ids[m]).unwrap().rect;
        assert!(l.frames.iter().any(|f| f.rect.contains_rect(&r)), "{m} not framed");
    }
}

#[test]
fn edge_endpoints_spread_along_box_sides() {
    let g = graph(&["a", "b", "c", "core"], &main_deps(&[("a", "core"), ("b", "core"), ("c", "core")]));
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    let ends: Vec<f32> = l.edges.iter().map(|e| e.points.last().unwrap()[0]).collect();
    assert_eq!(ends.len(), 3);
    assert!(ends.iter().any(|x| (x - ends[0]).abs() > 5.0), "all arrows end at the same x: {ends:?}");
    let core = l.children.iter().find(|c| c.node == g.ids["core"]).unwrap().rect;
    assert!(ends.iter().all(|&x| x >= core.x && x <= core.right()));
}

fn with_entries(modules: &[&str], deps: &[(&str, &str)], apps: &[&str]) -> G {
    let mut b = GraphBuilder::new("p");
    let mut ids = HashMap::new();
    for m in modules {
        let id = b.add_node(b.root(), NodeKind::Module, SubKind::None, m, Lang::Java);
        let pkg = b.add_node(id, NodeKind::Package, SubKind::None, "p", Lang::Java);
        let t = b.add_node(pkg, NodeKind::Type, SubKind::Class, "T", Lang::Java);
        b.set_span(t, "f", 1, 100);
        let main = b.add_node(t, NodeKind::Member, SubKind::Method, "main", Lang::Java);
        if apps.contains(m) {
            b.add_entry(EntryPointSpec {
                node: main,
                kind: "main".into(),
                category: Category::Application,
                label: m.to_string(),
                parent: None,
                lifecycle: Lifecycle::Once,
                order: 0,
                rule: "r".into(),
                handoff: None,
            });
        }
        ids.insert(m.to_string(), id);
    }
    for (a, c) in deps {
        b.add_edge(ids[*a], ids[*c], EdgeKind::DependsOn, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g.codescape")).unwrap();
    let file = IndexFile::open(&dir.path().join("g.codescape")).unwrap();
    let g = file.graph();
    let ids = g.children(g.root()).map(|c| (g.name(c).to_string(), c)).collect();
    G { _dir: dir, file, ids }
}

#[test]
fn pinned_entry_owners_take_the_top_row_alphabetically() {
    let g = with_entries(
        &["util", "zeta", "core", "bot", "desktop", "client"],
        &[("bot", "desktop"), ("desktop", "client"), ("zeta", "core"), ("client", "core"), ("util", "core")],
        &["zeta", "bot", "desktop"],
    );
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    let row = |n: &str| l.children.iter().find(|c| c.node == g.ids[n]).unwrap();
    for app in ["bot", "desktop", "zeta"] {
        assert_eq!(row(app).layer, 0, "{app}");
        assert!(row(app).pinned);
    }
    assert!(!row("util").pinned);
    let mut top: Vec<&ChildBox> = l.children.iter().filter(|c| c.pinned).collect();
    top.sort_by(|a, b| a.rect.x.total_cmp(&b.rect.x));
    let names: Vec<&str> = top.iter().map(|c| gr.name(c.node)).collect();
    assert_eq!(names, ["bot", "desktop", "zeta"]);
    let bot_desktop = l.edges.iter().find(|e| e.a == g.ids["bot"] && e.b == g.ids["desktop"]).unwrap();
    assert_eq!(bot_desktop.dir, Direction::Sideways);
    assert!(row("client").layer >= 1 && row("core").layer > row("client").layer);
}

#[test]
fn pinned_edges_do_not_hang() {
    let g = with_entries(&["a", "b", "c"], &[("a", "b"), ("b", "a"), ("b", "c"), ("c", "a")], &["a", "b"]);
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    assert_eq!(l.children.len(), 3);
    assert!(l.children.iter().filter(|c| c.pinned).all(|c| c.layer == 0));
}

fn wired_graph() -> G {
    let mut b = GraphBuilder::new("p");
    let mut ids = HashMap::new();
    for m in ["cmd", "billing", "auth", "core"] {
        let id = b.add_node(b.root(), NodeKind::Package, SubKind::None, m, Lang::Go);
        let f = b.add_node(id, NodeKind::Member, SubKind::Function, "f", Lang::Go);
        b.set_span(f, "f", 1, 40);
        ids.insert(m, (id, f));
    }
    let wire = b.add_node(ids["cmd"].0, NodeKind::Member, SubKind::Function, "initializeApp", Lang::Go);
    b.add_flags(wire, NodeFlags::WIRING);
    for m in ["billing", "auth", "core"] {
        b.add_edge(wire, ids[m].1, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    b.add_edge(ids["billing"].1, ids["core"].1, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    b.add_edge(ids["auth"].1, ids["billing"].1, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g.codescape")).unwrap();
    let file = IndexFile::open(&dir.path().join("g.codescape")).unwrap();
    let g = file.graph();
    let ids = g.children(g.root()).map(|c| (g.name(c).to_string(), c)).collect();
    G { _dir: dir, file, ids }
}

#[test]
fn wiring_never_moves_boxes() {
    let g = wired_graph();
    let gr = g.file.graph();
    let hidden = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    assert!(hidden.edges.iter().all(|e| e.wiring == 0), "wiring arrows hidden by default");
    let shown = layout_container(gr, gr.root(), &[], &LayoutOptions { show_wiring: true, ..Default::default() });
    assert_eq!(shown.edges.iter().filter(|e| e.wiring > 0).count(), 3);
    let rects = |l: &ContainerLayout| -> Vec<(NodeId, u32)> {
        let mut v: Vec<(NodeId, u32)> = l.children.iter().map(|c| (c.node, c.layer)).collect();
        v.sort();
        v
    };
    assert_eq!(rects(&hidden), rects(&shown));
    let cmd = hidden.children.iter().find(|c| c.node == g.ids["cmd"]).unwrap();
    assert_eq!(cmd.layer, 0);
    let core = hidden.children.iter().find(|c| c.node == g.ids["core"]).unwrap();
    assert_eq!(core.layer, 2, "auth -> billing -> core, wiring does not stretch it");
}

#[test]
fn constructors_head_their_card() {
    let mut b = GraphBuilder::new("p");
    let t = b.add_node(b.root(), NodeKind::Type, SubKind::Struct, "checkoutService", Lang::Go);
    for (name, sub) in [("Run", SubKind::Method), ("Alpha", SubKind::Method), ("NewCheckoutService", SubKind::Constructor)] {
        let m = b.add_node(t, NodeKind::Member, sub, name, Lang::Go);
        b.set_span(m, "f", 1, 5);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let t = g.children(g.root()).start;
    let rows: Vec<&str> = card_members(g, t, &LayoutOptions::default()).into_iter().map(|m| g.name(m)).collect();
    assert_eq!(rows[0], "NewCheckoutService");
    assert_eq!(rows.len(), 3);
}

#[test]
fn swap_removes_obvious_crossing() {
    let mut b = GraphBuilder::new("p");
    let t = b.add_node(b.root(), NodeKind::Type, SubKind::Class, "TickClock", Lang::Java);
    let mut ids = HashMap::new();
    for name in ["<init>", "tick", "getRate", "getElapsed", "elapsedMillis", "advance", "setRate", "awaitReady"] {
        let m = b.add_node(t, NodeKind::Member, SubKind::Method, name, Lang::Java);
        b.set_span(m, "f", 1, 6);
        ids.insert(name, m);
    }
    for (a, c) in [("<init>", "setRate"), ("tick", "advance"), ("tick", "elapsedMillis"), ("elapsedMillis", "awaitReady")] {
        b.add_edge(ids[a], ids[c], EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let t = g.children(g.root()).start;
    let l = layout_container(g, t, &[], &LayoutOptions::default());
    assert_eq!(count_crossings(&l), 0);
}

#[test]
fn random_graphs_have_30_percent_fewer_crossings() {
    use codescape_layout::order::{Ordering, barycenter_poc, crossings, minimize};
    let mut seed: u64 = 42;
    let mut rnd = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    let (mut before, mut after) = (0usize, 0usize);
    for _ in 0..40 {
        let layers_n = 4 + rnd(5);
        let mut layers: Vec<Vec<usize>> = Vec::new();
        let mut layer_of = Vec::new();
        let mut next = 0;
        for l in 0..layers_n {
            let width = 3 + rnd(8);
            layers.push((next..next + width).collect());
            layer_of.extend(std::iter::repeat_n(l as u32, width));
            next += width;
        }
        let mut edges = Vec::new();
        for l in 0..layers_n - 1 {
            for &u in &layers[l] {
                for _ in 0..1 + rnd(3) {
                    let v = layers[l + 1][rnd(layers[l + 1].len())];
                    edges.push((u, v));
                }
            }
        }
        let poc = barycenter_poc(&layers, &layer_of, &edges);
        let best = minimize(&layers, &layer_of, &edges, &Ordering::default());
        before += crossings(&poc, &layer_of, &edges);
        after += crossings(&best, &layer_of, &edges);
    }
    assert!(after * 10 <= before * 7, "crossings {before} -> {after}");
}

#[test]
fn pinned_row_never_wraps() {
    let apps: Vec<String> = (0..14).map(|i| format!("app{i:02}")).collect();
    let mut names: Vec<&str> = apps.iter().map(|s| s.as_str()).collect();
    names.push("core");
    let deps: Vec<(&str, &str)> = apps.iter().map(|a| (a.as_str(), "core")).collect();
    let g = with_entries(&names, &deps, &names[..14]);
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    let ys: Vec<f32> = l.children.iter().filter(|c| c.pinned).map(|c| c.rect.y).collect();
    assert_eq!(ys.len(), 14);
    assert!(ys.iter().all(|&y| (y - ys[0]).abs() < 0.5), "pinned boxes share one row: {ys:?}");
}

#[test]
fn squeezed_layers_keep_forward_edges_down() {
    let names: Vec<String> = (0..12).map(|i| format!("n{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let deps: Vec<(&str, &str, Scope)> = refs.windows(2).map(|w| (w[0], w[1], Scope::Main)).collect();
    let g = graph(&refs, &deps);
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    assert!(l.children.iter().map(|c| c.layer).max().unwrap() < 11, "a 12-long chain is squeezed");
    assert!(l.edges.iter().all(|e| e.dir == Direction::Down), "{:?}", l.edges.iter().map(|e| e.dir).collect::<Vec<_>>());
}

#[test]
fn frames_skip_non_members_between_pinned_and_unpinned_members() {
    let g = with_entries(
        &["android", "server-chat", "server-game", "server-world", "core"],
        &[("server-game", "core"), ("server-world", "core"), ("android", "core"), ("server-chat", "core")],
        &["server-game", "server-world"],
    );
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[FrameRule::AutoPrefix { sep: "-".into() }], &LayoutOptions::default());
    let android = l.child(g.ids["android"]).unwrap().rect;
    assert!(l.frames.iter().all(|f| !f.rect.intersects(&android)), "android sits between server members but is not one");
    for m in ["server-chat", "server-game", "server-world"] {
        let r = l.child(g.ids[m]).unwrap().rect;
        assert!(l.frames.iter().any(|f| f.rect.contains_rect(&r)), "{m} framed");
    }
}

#[test]
fn wiring_siblings_never_move_other_boxes() {
    let mut b = GraphBuilder::new("p");
    let mut fs = Vec::new();
    for m in ["a", "b", "c", "d", "e", "f", "g"] {
        let id = b.add_node(b.root(), NodeKind::Package, SubKind::None, m, Lang::Go);
        let f = b.add_node(id, NodeKind::Member, SubKind::Function, "f", Lang::Go);
        b.set_span(f, "f", 1, 40);
        fs.push(f);
    }
    for w in fs.windows(2) {
        b.add_edge(w[0], w[1], EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let dagger = b.add_node(b.root(), NodeKind::Package, SubKind::None, "dagger", Lang::Go);
    let wire = b.add_node(dagger, NodeKind::Member, SubKind::Function, "provide", Lang::Go);
    b.set_span(wire, "f", 1, 40);
    b.add_flags(wire, NodeFlags::WIRING);
    for &f in &fs {
        b.add_edge(wire, f, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let file = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = file.graph();
    let off = layout_container(g, g.root(), &[], &LayoutOptions::default());
    let on = layout_container(g, g.root(), &[], &LayoutOptions { show_wiring: true, ..Default::default() });
    assert_eq!(on.children.len(), off.children.len() + 1);
    for c in &off.children {
        assert_eq!(on.child(c.node).unwrap().rect, c.rect, "{} moved", g.name(c.node));
    }
}

#[test]
fn only_pinned_boxes_refuse_to_wrap() {
    let mut names: Vec<String> = (0..39).map(|i| format!("lib{i:02}")).collect();
    names.push("app".into());
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let g = with_entries(&refs, &[], &["app"]);
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    let aspect = l.size[0] / l.size[1];
    assert!(aspect < 4.0, "unpinned sources still wrap: size {:?}", l.size);
    let app = l.child(g.ids["app"]).unwrap().rect;
    assert!(l.children.iter().filter(|c| !c.pinned).all(|c| c.rect.y > app.y), "pinned row sits above the rest");
}

#[test]
fn error_types_hidden_by_default_and_shown_without_moving_others() {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "billing", Lang::Java);
    let mut ids = Vec::new();
    for name in ["Checkout", "Invoice", "PaymentException"] {
        let t = b.add_node(pkg, NodeKind::Type, SubKind::Class, name, Lang::Java);
        b.set_span(t, "f", 1, 60);
        ids.push(t);
    }
    b.add_flags(ids[2], NodeFlags::NOISE);
    b.add_edge(ids[0], ids[1], EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    b.add_edge(ids[0], ids[2], EdgeKind::Instantiates, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let pkg = g.children(g.root()).start;
    let off = layout_container(g, pkg, &[], &LayoutOptions::default());
    assert_eq!(off.children.len(), 2);
    let on = layout_container(g, pkg, &[], &LayoutOptions { show_noise: true, ..Default::default() });
    assert_eq!(on.children.len(), 3);
    for c in &off.children {
        assert_eq!(on.child(c.node).unwrap().rect, c.rect);
    }
}

fn tagged(types: &[(&str, &str)], calls: &[(&str, &str)]) -> G {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "billing", Lang::Go);
    let mut ids = HashMap::new();
    for (name, tag) in types {
        let t = b.add_node(pkg, NodeKind::Type, SubKind::Struct, name, Lang::Go);
        b.set_span(t, "f", 1, 40);
        if !tag.is_empty() {
            b.add_tag(t, tag);
        }
        ids.insert(name.to_string(), t);
    }
    for (a, c) in calls {
        b.add_edge(ids[*a], ids[*c], EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let file = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = file.graph();
    let pkg = g.children(g.root()).start;
    let ids = g.children(pkg).map(|c| (g.name(c).to_string(), c)).collect();
    G { _dir: dir, file, ids }
}

#[test]
fn roles_order_layers_top_down() {
    let g = tagged(
        &[
            ("Invoice", "layer:model"),
            ("invoiceRepo", "layer:repository"),
            ("invoiceService", "layer:service"),
            ("InvoiceHandler", "layer:handler"),
            ("Price", ""),
        ],
        &[("InvoiceHandler", "invoiceService")],
    );
    let gr = g.file.graph();
    let pkg = gr.children(gr.root()).start;
    let l = layout_container(gr, pkg, &[], &LayoutOptions::default());
    let layer = |n: &str| l.child(g.ids[n]).unwrap().layer;
    assert!(layer("InvoiceHandler") < layer("invoiceService"));
    assert!(layer("invoiceService") < layer("invoiceRepo"), "no call links them, the roles still order them");
    assert!(layer("invoiceRepo") < layer("Invoice"));
}

#[test]
fn calls_against_the_role_order_point_up() {
    let g = tagged(&[("orderRepo", "layer:repository"), ("orderService", "layer:service")], &[("orderService", "orderRepo"), ("orderRepo", "orderService")]);
    let gr = g.file.graph();
    let pkg = gr.children(gr.root()).start;
    let l = layout_container(gr, pkg, &[], &LayoutOptions::default());
    assert!(l.child(g.ids["orderService"]).unwrap().layer < l.child(g.ids["orderRepo"]).unwrap().layer);
    let back = l.edges.iter().find(|e| e.a == g.ids["orderRepo"]).unwrap();
    assert_eq!(back.dir, Direction::Up);
    let down = l.edges.iter().find(|e| e.a == g.ids["orderService"]).unwrap();
    assert_eq!(down.dir, Direction::Down);
}

fn ports() -> G {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "billing", Lang::Go);
    let mk = |b: &mut GraphBuilder, name: &str, sub: SubKind| {
        let t = b.add_node(pkg, NodeKind::Type, sub, name, Lang::Go);
        b.set_span(t, "f", 1, 40);
        let m = b.add_node(t, NodeKind::Member, SubKind::Method, "Do", Lang::Go);
        b.set_span(m, "f", 2, 10);
        (t, m)
    };
    let (handler, hm) = mk(&mut b, "Handler", SubKind::Struct);
    let (port, _) = mk(&mut b, "Repository", SubKind::Interface);
    let (adapter, am) = mk(&mut b, "pgRepository", SubKind::Struct);
    let (price, _) = mk(&mut b, "Price", SubKind::Struct);
    b.add_flags(price, NodeFlags::DETAIL);
    b.add_edge(adapter, port, EdgeKind::Implements, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    b.add_edge(hm, am, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let _ = handler;
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let file = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = file.graph();
    let pkg = g.children(g.root()).start;
    let ids = g.children(pkg).map(|c| (g.name(c).to_string(), c)).collect();
    G { _dir: dir, file, ids }
}

#[test]
fn details_hidden_counted_and_shown_without_moving() {
    let g = ports();
    let gr = g.file.graph();
    let pkg = gr.children(gr.root()).start;
    let off = layout_container(gr, pkg, &[], &LayoutOptions::default());
    assert!(off.child(g.ids["Price"]).is_none());
    assert_eq!(off.hidden_details, 1);
    let on = layout_container(gr, pkg, &[], &LayoutOptions { show_details: true, ..Default::default() });
    assert!(on.child(g.ids["Price"]).is_some());
    assert_eq!(on.hidden_details, 0);
    for c in &off.children {
        assert_eq!(on.child(c.node).unwrap().rect, c.rect);
    }
}

#[test]
fn implementations_fold_into_their_interface() {
    let g = ports();
    let gr = g.file.graph();
    let pkg = gr.children(gr.root()).start;
    let l = layout_container(gr, pkg, &[], &LayoutOptions::default());
    assert!(l.child(g.ids["pgRepository"]).is_none(), "the implementation is not a box of its own");
    assert!(l.edges.iter().any(|e| e.a == g.ids["Handler"] && e.b == g.ids["Repository"]), "calls into the implementation point at the interface");
    assert_eq!(folded_into(gr, g.ids["pgRepository"]), Some(g.ids["Repository"]));
    let inside = layout_container(gr, g.ids["Repository"], &[], &LayoutOptions::default());
    assert!(inside.child(g.ids["pgRepository"]).is_some(), "opening the interface shows its implementation");
    assert_eq!(
        display_path(gr, gr.children(g.ids["pgRepository"]).start),
        vec![gr.root(), pkg, g.ids["Repository"], g.ids["pgRepository"], gr.children(g.ids["pgRepository"]).start]
    );
}

fn tagged_graph() -> G {
    let mut b = GraphBuilder::new("p");
    for m in ["app", "gen", "it"] {
        let id = b.add_node(b.root(), NodeKind::Module, SubKind::None, m, Lang::Go);
        let t = b.add_node(id, NodeKind::Type, SubKind::Struct, "T", Lang::Go);
        b.set_span(t, "f", 1, 10);
        b.set_lines(t, 10);
        match m {
            "gen" => {
                b.add_tag(id, "filter:generated");
                b.add_tag(t, "filter:generated");
            }
            "it" => b.add_flags(t, NodeFlags::TEST),
            _ => {}
        }
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g.codescape")).unwrap();
    let file = IndexFile::open(&dir.path().join("g.codescape")).unwrap();
    let g = file.graph();
    let ids = g.children(g.root()).map(|c| (g.name(c).to_string(), c)).collect();
    G { _dir: dir, file, ids }
}

#[test]
fn hidden_tags_take_boxes_out_and_tests_come_back_when_shown() {
    let g = tagged_graph();
    let gr = g.file.graph();
    let shown = |opts: &LayoutOptions| -> Vec<&str> { layout_container(gr, gr.root(), &[], opts).children.iter().map(|c| gr.name(c.node)).collect() };
    assert_eq!(shown(&LayoutOptions::default()), ["app", "gen"]);
    let filtered = LayoutOptions { hidden_tags: vec!["filter:generated".into()], ..Default::default() };
    assert_eq!(shown(&filtered), ["app"]);
    assert!(filtered.hides(gr, g.ids["gen"]) && !filtered.hides(gr, g.ids["app"]));
    let mut tests = shown(&LayoutOptions { show_tests: true, ..Default::default() });
    tests.sort();
    assert_eq!(tests, ["app", "gen", "it"]);
}

#[test]
fn details_can_be_opened_for_one_container_and_closed_again() {
    let g = ports();
    let gr = g.file.graph();
    let pkg = gr.children(gr.root()).start;
    let opts = LayoutOptions { details_in: vec![pkg], ..Default::default() };
    let open = layout_container(gr, pkg, &[], &opts);
    assert!(open.child(g.ids["Price"]).is_some());
    assert_eq!((open.hidden_details, open.shown_details), (0, 1), "the chip can offer to hide one detail");
    assert!(!opts.hides(gr, g.ids["Price"]));
    let elsewhere = LayoutOptions { details_in: vec![g.ids["Handler"]], ..Default::default() };
    assert!(elsewhere.hides(gr, g.ids["Price"]), "opening another box shows nothing here");
    let global = layout_container(gr, pkg, &[], &LayoutOptions { show_details: true, ..Default::default() });
    assert_eq!(global.shown_details, 0, "View > Details is not undone from a box");
}

#[test]
fn revealing_a_detail_opens_its_container_details() {
    let g = ports();
    let gr = g.file.graph();
    let pkg = gr.children(gr.root()).start;
    let mut opts = LayoutOptions::default();
    assert!(opts.hides(gr, g.ids["Price"]));
    assert!(opts.reveal(gr, g.ids["Price"]));
    assert_eq!(opts.details_in, [pkg]);
    assert!(!opts.hides(gr, g.ids["Price"]));
    assert!(!opts.reveal(gr, g.ids["Handler"]), "a visible box needs nothing");
}

#[test]
fn types_are_cards_with_room_for_their_method_rows() {
    let g = ports();
    let gr = g.file.graph();
    let opts = LayoutOptions::default();
    let handler = layout_container(gr, g.ids["Handler"], &[], &opts);
    assert!(handler.children.is_empty(), "methods are rows of the card, not boxes");
    let port = layout_container(gr, g.ids["Repository"], &[], &opts);
    let kids: Vec<&str> = port.children.iter().map(|c| gr.name(c.node)).collect();
    assert_eq!(kids, ["pgRepository"], "folded implementations are still boxes");
    assert!(port.children[0].rect.y >= card_rows_bottom(gr, g.ids["Repository"], &LayoutOptions::default()), "they sit below the method rows");
    let size = box_size(gr, g.ids["Handler"], &LayoutOptions::default());
    assert!(size[1] >= card_rows_bottom(gr, g.ids["Handler"], &LayoutOptions::default()), "the card is tall enough for its rows");
}

#[test]
fn a_card_with_nested_boxes_is_as_wide_as_the_card() {
    let g = ports();
    let gr = g.file.graph();
    let port = layout_container(gr, g.ids["Repository"], &[], &LayoutOptions::default());
    assert!(port.size[0] >= box_size(gr, g.ids["Repository"], &LayoutOptions::default())[0] - 0.01, "rows keep the card's scale: {:?}", port.size);
}

#[test]
fn java_init_counts_as_a_constructor_on_the_card() {
    let mut b = GraphBuilder::new("p");
    let t = b.add_node(b.root(), NodeKind::Type, SubKind::Class, "TextFilter", Lang::Java);
    for (name, line) in [("isActive", 10), ("<init>", 30)] {
        let m = b.add_node(t, NodeKind::Member, SubKind::Method, name, Lang::Java);
        b.set_span(m, "f", line, line + 3);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let t = g.children(g.root()).start;
    let rows = card_members(g, t, &LayoutOptions::default());
    assert_eq!(g.name(rows[0]), "<init>");
    assert!(is_constructor(g, rows[0]));
}

fn open(b: GraphBuilder) -> (tempfile::TempDir, IndexFile) {
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    (dir, f)
}

#[test]
fn cards_list_what_the_options_ask_for() {
    let mut b = GraphBuilder::new("p");
    let t = b.add_node(b.root(), NodeKind::Type, SubKind::Class, "Cart", Lang::Java);
    let mut member = |name: &str, sub: SubKind, vis: Visibility, line: u32| {
        let m = b.add_node(t, NodeKind::Member, sub, name, Lang::Java);
        b.set_span(m, "f", line, line + 1);
        b.set_visibility(m, vis);
        m
    };
    member("<init>", SubKind::Constructor, Visibility::Public, 1);
    member("total", SubKind::Method, Visibility::Public, 5);
    member("recompute", SubKind::Method, Visibility::Private, 9);
    let get = member("getItems", SubKind::Method, Visibility::Public, 13);
    member("items", SubKind::Field, Visibility::Private, 2);
    member("name", SubKind::Field, Visibility::Public, 3);
    b.add_tag(get, "accessor");
    let (_d, f) = open(b);
    let g = f.graph();
    let t = g.children(g.root()).start;
    let names = |opts: &LayoutOptions| card_members(g, t, opts).into_iter().map(|m| g.name(m)).collect::<Vec<_>>();
    let all = LayoutOptions::default();
    assert_eq!(names(&all), ["<init>", "total", "recompute", "getItems"]);
    let lean = LayoutOptions { card: CardOptions { private: false, fields: true, accessors: false, constructors: false }, ..Default::default() };
    assert_eq!(names(&lean), ["total"]);
    assert_eq!(card_field_rows(g, t, &all).0, 2);
    assert_eq!(card_field_rows(g, t, &lean).0, 1, "the private field is left out");
    let no_fields = LayoutOptions { card: CardOptions { fields: false, ..CardOptions::default() }, ..Default::default() };
    assert_eq!(card_field_rows(g, t, &no_fields), (0, 0));
    assert!(box_size(g, t, &lean)[1] < box_size(g, t, &all)[1], "the card shrinks with its rows");
}

#[test]
fn hidden_and_always_shown_blocks_override_the_flags() {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "shop", Lang::Go);
    let plain = b.add_node(pkg, NodeKind::Type, SubKind::Struct, "Cart", Lang::Go);
    let detail = b.add_node(pkg, NodeKind::Type, SubKind::Struct, "CartRow", Lang::Go);
    let generated = b.add_node(pkg, NodeKind::Type, SubKind::Struct, "CartPb", Lang::Go);
    b.add_flags(detail, NodeFlags::DETAIL);
    b.add_flags(generated, NodeFlags::GENERATED);
    let _ = plain;
    let (_d, f) = open(b);
    let g = f.graph();
    let id = |name: &str| (0..g.node_count() as NodeId).find(|&n| g.name(n) == name).unwrap();
    let mut opts = LayoutOptions::default();
    assert!(!opts.hides(g, id("Cart")));
    assert!(opts.hides(g, id("CartRow")), "a detail");
    assert!(opts.hides(g, id("CartPb")), "generated code is hidden by default");
    opts.hidden_nodes.insert(id("Cart"));
    opts.always_shown.insert(id("CartRow"));
    opts.show_generated = true;
    assert!(opts.hides(g, id("Cart")), "hidden by the user");
    assert!(!opts.hides(g, id("CartRow")), "always shown wins over the detail flag");
    assert!(!opts.hides(g, id("CartPb")));
}

#[test]
fn implements_only_and_import_only_arrows_can_be_left_out() {
    let mut b = GraphBuilder::new("p");
    let root = b.root();
    let [m1, m2, m3] = ["web", "api", "util"].map(|n| b.add_node(root, NodeKind::Module, SubKind::None, n, Lang::Java));
    let impl_ = b.add_node(m1, NodeKind::Type, SubKind::Class, "Handler", Lang::Java);
    let iface = b.add_node(m2, NodeKind::Type, SubKind::Interface, "Api", Lang::Java);
    b.add_edge(impl_, iface, EdgeKind::Implements, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    b.add_edge(m1, m3, EdgeKind::DependsOn, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let (_d, f) = open(b);
    let g = f.graph();
    let name = |n: NodeId| g.name(n).to_string();
    let arrows = |opts: &LayoutOptions| {
        let mut v: Vec<(String, String)> = layout_container(g, g.root(), &[], opts).edges.iter().map(|e| (name(e.a), name(e.b))).collect();
        v.sort();
        v
    };
    let all = LayoutOptions::default();
    assert_eq!(arrows(&all), [("web".to_string(), "api".to_string()), ("web".to_string(), "util".to_string())]);
    assert_eq!(arrows(&LayoutOptions { implements_arrows: false, ..Default::default() }), [("web".to_string(), "util".to_string())]);
    assert_eq!(arrows(&LayoutOptions { import_arrows: false, ..Default::default() }), [("web".to_string(), "api".to_string())]);
}
