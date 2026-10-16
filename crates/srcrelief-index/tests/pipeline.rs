use srcrelief_format::*;
use srcrelief_index::*;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name).canonicalize().unwrap()
}

fn cfg_with_index(dir: &Path) -> ProjectConfig {
    ProjectConfig { index: Some(dir.join("out.relief")), ..Default::default() }
}

fn names(g: &ArchivedGraph, parent: NodeId) -> Vec<String> {
    g.children(parent).map(|c| g.name(c).to_string()).collect()
}

#[test]
fn indexes_fixture_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let out = index_project(&fixture("java-resolve"), &cfg_with_index(tmp.path()), &|_| {}).unwrap();
    let file = ReliefFile::open(&out.path).unwrap();
    let g = file.graph();
    let mods = names(g, g.root());
    for m in ["app", "core", "util", "(external)"] {
        assert!(mods.contains(&m.to_string()), "{mods:?}");
    }
    let main = (0..g.entries().len()).find(|&i| g.entry_label(i) == "Main").expect("Main entry");
    assert_eq!(g.entry_category(main), Category::Application);
    assert!(g.stats().files.to_native() > 0);
    assert!(g.provenance().any(|(n, _, _)| n == "libgdx"));
    assert!(out.summary.to_string().contains("Main"));
}

#[test]
fn reindex_when_source_newer() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("p");
    copy_dir(&fixture("go-resolve"), &proj);
    let cfg = cfg_with_index(tmp.path());
    let out = index_project(&proj, &cfg, &|_| {}).unwrap();
    let packs = project_packs(&proj, &cfg).unwrap();
    assert!(!needs_reindex(&proj, &out.path, &cfg, &packs));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(proj.join("shared/log.go"), "package shared\n\nfunc Log(s string) {}\nfunc More() {}\n").unwrap();
    assert!(needs_reindex(&proj, &out.path, &cfg, &packs));
}

#[test]
fn reindex_when_index_corrupt() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg_with_index(tmp.path());
    let dir = fixture("go-resolve");
    let out = index_project(&dir, &cfg, &|_| {}).unwrap();
    let bytes = std::fs::read(&out.path).unwrap();
    std::fs::write(&out.path, &bytes[..bytes.len() / 3]).unwrap();
    let packs = project_packs(&dir, &cfg).unwrap();
    assert!(needs_reindex(&dir, &out.path, &cfg, &packs));
    assert!(needs_reindex(&dir, &tmp.path().join("missing.relief"), &cfg, &packs));
}

#[test]
fn reindex_when_detection_rules_change() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = fixture("go-resolve");
    let cfg = cfg_with_index(tmp.path());
    let out = index_project(&dir, &cfg, &|_| {}).unwrap();
    let mut changed = cfg.clone();
    changed.categories.push(CategoryOverride { glob: "**/cmd/**".into(), category: Category::DevTool });
    let packs = project_packs(&dir, &changed).unwrap();
    assert!(needs_reindex(&dir, &out.path, &changed, &packs));
}

#[test]
fn config_category_override() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("srcrelief.toml");
    let index = tmp.path().join("x.relief");
    std::fs::write(&cfg_path, format!("index = {:?}\n\n[[category]]\nglob = \"**/cmd/svc-a/**\"\ncategory = \"dev-tool\"\n", index)).unwrap();
    let dir = fixture("go-resolve");
    let cfg = ProjectConfig::load(&dir, Some(&cfg_path)).unwrap();
    let out = index_project(&dir, &cfg, &|_| {}).unwrap();
    let file = ReliefFile::open(&out.path).unwrap();
    let g = file.graph();
    let e = (0..g.entries().len()).find(|&i| g.entry_label(i) == "svc-a").unwrap();
    assert_eq!(g.entry_category(e), Category::DevTool);
}

#[test]
fn index_path_in_cache_not_project() {
    let dir = fixture("java-gradle");
    let before = listing(&dir);
    let out = index_project(&dir, &ProjectConfig::default(), &|_| {}).unwrap();
    assert!(out.path.starts_with(dirs::cache_dir().unwrap().join("srcrelief")), "{}", out.path.display());
    assert_eq!(out.path, cache_path(&dir));
    assert_eq!(listing(&dir), before);
}

#[test]
fn config_file_at_project_root_is_used() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("p");
    copy_dir(&fixture("go-resolve"), &proj);
    std::fs::write(proj.join("srcrelief.toml"), "name = \"Renamed\"\nexclude = [\"shared/**\"]\n").unwrap();
    let cfg = ProjectConfig::load(&proj, None).unwrap();
    assert_eq!(cfg.name.as_deref(), Some("Renamed"));
    let cfg = ProjectConfig { index: Some(tmp.path().join("o.relief")), ..cfg };
    let out = index_project(&proj, &cfg, &|_| {}).unwrap();
    let file = ReliefFile::open(&out.path).unwrap();
    let g = file.graph();
    assert_eq!(g.name(g.root()), "Renamed");
    assert!(!names(g, g.root()).contains(&"shared".to_string()));
}

fn listing(dir: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p.clone());
            }
            v.push(p);
        }
    }
    v.sort();
    v
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn write_pack(dir: &Path, detection_pattern: &str, color: [u8; 3]) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("pack.toml"),
        format!(
            "name = \"mine\"\nversion = \"1\"\n[[rule]]\nid = \"mine.repo\"\nname_tag = {{ kinds = [\"type\"], pattern = \"{detection_pattern}\", tag = \"layer:repository\" }}\n[visual]\n[[visual.tag_style]]\ntag = \"layer:repository\"\ncolor = [{}, {}, {}]\n",
            color[0], color[1], color[2]
        ),
    )
    .unwrap();
}

#[test]
fn user_pack_from_config_dir_tags_without_touching_the_project() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("packs/mine");
    write_pack(&pack, "Repo$", [200, 10, 10]);
    let dir = fixture("go-resolve");
    let before = listing(&dir);
    let mut cfg = cfg_with_index(tmp.path());
    cfg.packs.extra_dirs.push(tmp.path().join("packs"));
    let out = index_project(&dir, &cfg, &|_| {}).unwrap();
    let file = ReliefFile::open(&out.path).unwrap();
    let g = file.graph();
    let tagged: Vec<&str> = (0..g.node_count() as NodeId).filter(|&n| g.has_tag(n, "layer:repository")).map(|n| g.name(n)).collect();
    assert!(tagged.contains(&"Repo") && tagged.contains(&"pgRepo"), "{tagged:?}");
    assert_eq!(listing(&dir), before);

    write_pack(&pack, "Repo$", [10, 200, 10]);
    let packs = project_packs(&dir, &cfg).unwrap();
    assert!(!needs_reindex(&dir, &out.path, &cfg, &packs), "a colour change is visual only");
    assert_eq!(packs.visual().style_for("layer:repository").unwrap().color, [10, 200, 10]);
    write_pack(&pack, "Service$", [10, 200, 10]);
    let packs = project_packs(&dir, &cfg).unwrap();
    assert!(needs_reindex(&dir, &out.path, &cfg, &packs), "a detection change needs a new index");
}
