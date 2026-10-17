use srcrelief_format::*;
use srcrelief_index::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn node(g: &ArchivedGraph, path: &[&str]) -> NodeId {
    let mut n = g.root();
    for p in path {
        n = g
            .children(n)
            .find(|&c| g.name(c) == *p)
            .unwrap_or_else(|| panic!("missing {p} in {path:?}; have {:?}", g.children(n).map(|c| g.name(c)).collect::<Vec<_>>()));
    }
    n
}

fn mains(g: &ArchivedGraph) -> Vec<(String, Category)> {
    let mut v: Vec<(String, Category)> =
        (0..g.entries().len()).filter(|&i| g.entry_kind(i) == "main").map(|i| (g.entry_label(i).to_string(), g.entry_category(i))).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn calls(g: &ArchivedGraph, from: NodeId, to: NodeId) -> bool {
    g.out_edges(from).iter().any(|e| e.to.to_native() == to && EdgeKind::from_u8(e.kind) == EdgeKind::Calls)
}

#[test]
fn rust_mains_are_entry_points_by_target() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path().join("proj");
    write(&r, "Cargo.toml", "[package]\nname = \"tool\"\n");
    write(&r, "src/main.rs", "#[tokio::main]\nasync fn main() { tool::run(); }\n");
    write(&r, "src/lib.rs", "pub fn run() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { super::run(); }\n}\n");
    write(&r, "src/bin/migrate.rs", "fn main() {}\n");
    write(&r, "examples/demo.rs", "fn main() {}\n");
    write(&r, "benches/speed.rs", "fn main() {}\n");
    write(&r, "tests/harness.rs", "fn main() {}\n");
    write(&r, "build.rs", "fn main() {}\n");
    let cfg = ProjectConfig { index: Some(tmp.path().join("out.relief")), ..Default::default() };
    let out = index_project(&r, &cfg, &|_| {}).unwrap();
    let file = ReliefFile::open(&out.path).unwrap();
    let g = file.graph();
    assert_eq!(
        mains(g),
        [
            ("(build)".to_string(), Category::DevTool),
            ("demo".to_string(), Category::DevTool),
            ("harness".to_string(), Category::Test),
            ("migrate".to_string(), Category::Application),
            ("speed".to_string(), Category::DevTool),
            ("tool".to_string(), Category::Application),
        ]
    );
    let t = node(g, &["tool", "tests", "t"]);
    assert!(g.flags(t).contains(NodeFlags::TEST));
    assert!(calls(g, node(g, &["tool", "tool", "main"]), node(g, &["tool", "(lib)", "run"])));
    assert!(g.skipped_roots().next().is_none());
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

#[test]
#[ignore]
fn srcrelief_indexes_itself() {
    let dir = repo();
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = ProjectConfig { index: Some(tmp.path().join("self.relief")), ..Default::default() };
    cfg.exclude.push("**/tests/fixtures/**".into());
    let out = index_project(&dir, &cfg, &|_| {}).unwrap();
    println!("{}", out.summary);
    let file = ReliefFile::open(&out.path).unwrap();
    let g = file.graph();

    let mut crates: Vec<String> = std::fs::read_dir(dir.join("crates"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("Cargo.toml").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    crates.sort();
    let mut mods: Vec<String> = g.children(g.root()).map(|c| g.name(c).to_string()).filter(|m| m != "(external)").collect();
    mods.sort();
    assert_eq!(mods, crates);

    let deps: HashSet<(String, String)> = g
        .children(g.root())
        .flat_map(|m| {
            g.out_edges(m)
                .iter()
                .filter(|e| EdgeKind::from_u8(e.kind) == EdgeKind::DependsOn)
                .map(move |e| (g.name(m).to_string(), g.name(e.to.to_native()).to_string()))
        })
        .collect();
    for (from, to) in
        [("srcrelief-index", "srcrelief-format"), ("srcrelief-index", "srcrelief-rust"), ("srcrelief", "srcrelief-layout"), ("srcrelief-go", "srcrelief-build")]
    {
        assert!(deps.contains(&(from.to_string(), to.to_string())), "{from} -> {to}");
    }
    assert!(!deps.iter().any(|(f, _)| f == "srcrelief-format"), "srcrelief-format depends on no crate");

    let m = mains(g);
    for label in ["srcrelief", "srcrelief-index", "scene_bench", "srcrelief-synth"] {
        assert!(m.contains(&(label.to_string(), Category::Application)), "{label} in {m:?}");
    }
    assert!(m.iter().filter(|(_, c)| *c == Category::Application).count() == 4, "{m:?}");

    let gb = node(g, &["srcrelief-format", "builder", "GraphBuilder"]);
    let members: Vec<&str> = g.children(gb).map(|c| g.name(c)).collect();
    assert!(members.contains(&"finish") && members.contains(&"add_node"), "{members:?}");
    assert_eq!(g.sub_kind(node(g, &["srcrelief-format", "builder", "GraphBuilder", "new"])), SubKind::Constructor);
    let cache = node(g, &["srcrelief-layout", "(lib)", "LayoutCache"]);
    assert_eq!(g.kind(cache), NodeKind::Type);

    let index_project_fn = node(g, &["srcrelief-index", "pipeline", "index_project"]);
    assert!(calls(g, index_project_fn, node(g, &["srcrelief-go", "resolve", "extract_go"])));
    assert!(calls(g, index_project_fn, node(g, &["srcrelief-rust", "resolve", "extract_rust"])));
    assert!(calls(g, index_project_fn, node(g, &["srcrelief-format", "builder", "GraphBuilder", "finish"])));

    let test_fn = node(g, &["srcrelief-rust", "tests/all::extract", "packages_follow_the_module_tree"]);
    assert!(g.flags(test_fn).contains(NodeFlags::TEST));
    assert!(!g.flags(index_project_fn).contains(NodeFlags::TEST));

    let s = g.stats();
    let (res, gue, unr) = (s.calls_resolved.to_native(), s.calls_guessed.to_native(), s.calls_unresolved.to_native());
    let total = (res + gue + unr).max(1) as f64;
    println!(
        "calls: {res} resolved ({:.1}%), {gue} guessed ({:.1}%), {unr} unresolved ({:.1}%)",
        res as f64 * 100.0 / total,
        gue as f64 * 100.0 / total,
        unr as f64 * 100.0 / total
    );
    assert!(g.skipped_roots().next().is_none(), "{:?}", g.skipped_roots().collect::<Vec<_>>());
}
