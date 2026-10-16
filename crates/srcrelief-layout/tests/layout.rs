use srcrelief_format::*;
use srcrelief_layout::*;
use srcrelief_rules::FrameRule;
use std::collections::HashMap;

struct G {
    _dir: tempfile::TempDir,
    file: ReliefFile,
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
    let path = dir.path().join("g.relief");
    write_graph(&b.finish(), &path).unwrap();
    let file = ReliefFile::open(&path).unwrap();
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
    let g = graph(&["android", "bot", "client", "core", "desktop"], &main_deps(&[("bot", "desktop"), ("desktop", "client"), ("client", "core"), ("android", "client")]));
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
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let l = layout_container(g, g.root(), &[], &LayoutOptions::default());
    assert_eq!(l.children.len(), 1);
    assert!(l.edges.is_empty());
}

#[test]
fn auto_prefix_frames() {
    let g = graph(&["client", "core", "server-core", "server-game", "server-world"], &main_deps(&[("server-game", "server-core"), ("server-world", "server-core"), ("server-core", "core"), ("client", "core")]));
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[FrameRule::AutoPrefix { sep: "-".into() }], &LayoutOptions::default());
    assert_eq!(l.frames.len(), 1);
    assert_eq!(l.frames[0].label, "server");
    for m in ["server-core", "server-game", "server-world"] {
        let r = l.children.iter().find(|c| c.node == g.ids[m]).unwrap().rect;
        assert!(l.frames[0].rect.contains_rect(&r), "{m} outside frame");
    }
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
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let pkg = g.children(g.children(g.root()).start).start;
    let t = std::time::Instant::now();
    let l = layout_container(g, pkg, &[], &LayoutOptions::default());
    let elapsed = t.elapsed();
    assert_eq!(l.children.len(), 2000);
    assert!(elapsed.as_millis() < 100, "took {elapsed:?}");
    let aspect = l.size[0] / l.size[1];
    assert!((0.3..4.0).contains(&aspect), "aspect {aspect}");
}
