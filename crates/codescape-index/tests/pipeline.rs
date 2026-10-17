use codescape_format::*;
use codescape_index::*;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name).canonicalize().unwrap()
}

fn cfg_with_index(dir: &Path) -> ProjectConfig {
    ProjectConfig { index: Some(dir.join("out.codescape")), ..Default::default() }
}

fn names(g: &ArchivedGraph, parent: NodeId) -> Vec<String> {
    g.children(parent).map(|c| g.name(c).to_string()).collect()
}

#[test]
fn indexes_fixture_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let out = index_project(&fixture("java-resolve"), &cfg_with_index(tmp.path()), &|_| {}).unwrap();
    let file = IndexFile::open(&out.path).unwrap();
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
    assert!(needs_reindex(&dir, &tmp.path().join("missing.codescape"), &cfg, &packs));
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
    let cfg_path = tmp.path().join("codescape.toml");
    let index = tmp.path().join("x.codescape");
    std::fs::write(&cfg_path, format!("index = {:?}\n\n[[category]]\nglob = \"**/cmd/svc-a/**\"\ncategory = \"dev-tool\"\n", index)).unwrap();
    let dir = fixture("go-resolve");
    let cfg = ProjectConfig::load(&dir, Some(&cfg_path)).unwrap();
    let out = index_project(&dir, &cfg, &|_| {}).unwrap();
    let file = IndexFile::open(&out.path).unwrap();
    let g = file.graph();
    let e = (0..g.entries().len()).find(|&i| g.entry_label(i) == "svc-a").unwrap();
    assert_eq!(g.entry_category(e), Category::DevTool);
}

#[test]
fn index_path_in_cache_not_project() {
    let dir = fixture("java-gradle");
    let before = listing(&dir);
    let out = index_project(&dir, &ProjectConfig::default(), &|_| {}).unwrap();
    assert!(out.path.starts_with(dirs::cache_dir().unwrap().join("codescape")), "{}", out.path.display());
    assert_eq!(out.path, cache_path(&dir));
    assert_eq!(listing(&dir), before);
}

#[test]
fn config_file_at_project_root_is_used() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("p");
    copy_dir(&fixture("go-resolve"), &proj);
    std::fs::write(proj.join("codescape.toml"), "name = \"Renamed\"\nexclude = [\"shared/**\"]\n").unwrap();
    let cfg = ProjectConfig::load(&proj, None).unwrap();
    assert_eq!(cfg.name.as_deref(), Some("Renamed"));
    let cfg = ProjectConfig { index: Some(tmp.path().join("o.codescape")), ..cfg };
    let out = index_project(&proj, &cfg, &|_| {}).unwrap();
    let file = IndexFile::open(&out.path).unwrap();
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
    let file = IndexFile::open(&out.path).unwrap();
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

#[test]
fn reindex_when_a_source_is_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("p");
    copy_dir(&fixture("go-resolve"), &proj);
    let cfg = cfg_with_index(tmp.path());
    let out = index_project(&proj, &cfg, &|_| {}).unwrap();
    let packs = project_packs(&proj, &cfg).unwrap();
    assert!(!needs_reindex(&proj, &out.path, &cfg, &packs));
    std::fs::remove_file(proj.join("shared/bus.go")).unwrap();
    assert!(needs_reindex(&proj, &out.path, &cfg, &packs));
}

#[test]
fn project_excludes_are_relative_to_the_project_root() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = cfg_with_index(tmp.path());
    cfg.exclude.push("svc-a/internal/repo/**".into());
    let out = index_project(&fixture("go-resolve"), &cfg, &|_| {}).unwrap();
    let file = IndexFile::open(&out.path).unwrap();
    let g = file.graph();
    let svc = g.children(g.root()).find(|&m| g.name(m) == "svc-a").unwrap();
    let pkgs = names(g, svc);
    assert!(!pkgs.iter().any(|p| p.starts_with("internal/repo")), "{pkgs:?}");
    assert!(pkgs.contains(&"internal/handler".to_string()));
}

#[test]
fn unsupported_include_setting_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("codescape.toml");
    std::fs::write(&cfg_path, "include = [\"src/**\"]\n").unwrap();
    let err = ProjectConfig::load(&fixture("go-resolve"), Some(&cfg_path)).unwrap_err().to_string();
    assert!(err.contains("include"), "{err}");
}

#[test]
fn index_records_the_indexer_build() {
    let tmp = tempfile::tempdir().unwrap();
    let out = index_project(&fixture("go-resolve"), &cfg_with_index(tmp.path()), &|_| {}).unwrap();
    let file = IndexFile::open(&out.path).unwrap();
    let stamp = engine_stamp();
    assert!(!stamp.is_empty());
    assert!(file.graph().provenance().any(|(n, _, h)| n == "(engine)" && h == stamp));
}

#[test]
fn display_settings_in_the_shared_file_need_no_new_index_but_a_new_name_does() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("p");
    copy_dir(&fixture("go-resolve"), &proj);
    let cfg = cfg_with_index(tmp.path());
    let out = index_project(&proj, &cfg, &|_| {}).unwrap();
    let packs = project_packs(&proj, &cfg).unwrap();
    std::fs::write(proj.join("codescape.toml"), "[view]\ndetails = true\nwiring = true\n").unwrap();
    let loaded = ProjectConfig { index: cfg.index.clone(), ..ProjectConfig::load(&proj, None).unwrap() };
    assert!(!needs_reindex(&proj, &out.path, &loaded, &packs));
    let renamed = ProjectConfig { name: Some("Other".into()), ..loaded.clone() };
    assert!(needs_reindex(&proj, &out.path, &renamed, &packs));
    let mut excluded = loaded;
    excluded.exclude.push("nothing-matches/**".into());
    assert!(needs_reindex(&proj, &out.path, &excluded, &packs));
}

fn filter_project(root: &Path) {
    let files = [
        ("go.mod", "module example.com/app\n\ngo 1.22\n"),
        ("main.go", "package main\n\nimport \"example.com/app/internal/svc\"\n\nfunc main() {\n\tsvc.Run()\n}\n"),
        ("internal/svc/service.go", "package svc\n\ntype Service struct{}\n\nfunc (s *Service) Serve() {}\n\nfunc Run() {}\n"),
        ("internal/svc/user_migration.go", "package svc\n\ntype UserMigration struct{}\n\nfunc (u UserMigration) Up() {}\n"),
        ("internal/migrations/m001.go", "package migrations\n\ntype AddUsers struct{}\n\nfunc (a AddUsers) Up() {}\n"),
        ("it/smoke.go", "package it\n\ntype Smoke struct{}\n\nfunc (s Smoke) Check() {}\n"),
    ];
    for (rel, src) in files {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }
}

fn node(g: &ArchivedGraph, name: &str) -> NodeId {
    (0..g.node_count() as NodeId).find(|&n| g.name(n) == name).unwrap_or_else(|| panic!("no node {name}"))
}

fn migrations() -> Filter {
    Filter { name: "migrations".into(), paths: vec!["**/migrations/**".into()], names: vec!["Migration$".into()], shown: false }
}

#[test]
fn a_filter_tags_matching_types_their_members_and_containers_left_with_nothing_else() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("p");
    filter_project(&proj);
    let mut cfg = cfg_with_index(tmp.path());
    cfg.filters.push(migrations());
    let out = index_project(&proj, &cfg, &|_| {}).unwrap();
    let file = IndexFile::open(&out.path).unwrap();
    let g = file.graph();
    let tag = "filter:migrations";
    for n in ["AddUsers", "UserMigration", "internal/migrations"] {
        assert!(g.has_tag(node(g, n), tag), "{n} should be tagged");
    }
    let up: Vec<NodeId> = (0..g.node_count() as NodeId).filter(|&n| g.name(n) == "Up").collect();
    assert_eq!(up.len(), 2);
    assert!(up.iter().all(|&n| g.has_tag(n, tag)));
    for n in ["Service", "Serve", "internal/svc", "Run"] {
        assert!(!g.has_tag(node(g, n), tag), "{n} should not be tagged");
    }
    let packs = project_packs(&proj, &cfg).unwrap();
    assert!(!needs_reindex(&proj, &out.path, &cfg, &packs));
    let mut shown = cfg.clone();
    shown.filters[0].shown = true;
    assert!(!needs_reindex(&proj, &out.path, &shown, &project_packs(&proj, &shown).unwrap()), "shown is a display setting");
    let mut changed = cfg.clone();
    changed.filters[0].names = vec!["Service$".into()];
    assert!(needs_reindex(&proj, &out.path, &changed, &project_packs(&proj, &changed).unwrap()));
}

#[test]
fn extra_test_globs_mark_files_as_test_code() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("p");
    filter_project(&proj);
    let cfg = cfg_with_index(tmp.path());
    let out = index_project(&proj, &cfg, &|_| {}).unwrap();
    let file = IndexFile::open(&out.path).unwrap();
    assert!(!file.graph().flags(node(file.graph(), "Smoke")).contains(NodeFlags::TEST));
    let mut with = cfg.clone();
    with.test_paths.push("it/**".into());
    let packs = project_packs(&proj, &with).unwrap();
    assert!(needs_reindex(&proj, &out.path, &with, &packs));
    let out = index_project(&proj, &with, &|_| {}).unwrap();
    let file = IndexFile::open(&out.path).unwrap();
    let g = file.graph();
    assert!(g.flags(node(g, "Smoke")).contains(NodeFlags::TEST));
    assert!(!g.flags(node(g, "Service")).contains(NodeFlags::TEST));
}
