use codescape_format::*;
use codescape_index::*;
use std::path::Path;

#[test]
fn typescript_workspace_indexes_with_entries_and_routes() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/ts-resolve");
    let tmp = tempfile::tempdir().unwrap();
    let cfg = ProjectConfig { index: Some(tmp.path().join("ts.codescape")), ..Default::default() };
    let out = index_project(&dir, &cfg, &|_| {}).unwrap();
    let file = IndexFile::open(&out.path).unwrap();
    let g = file.graph();
    let mut mods: Vec<&str> = g.children(g.root()).map(|c| g.name(c)).collect();
    mods.sort();
    assert_eq!(mods, ["(external)", "@demo/shared", "server", "web"]);
    assert_eq!(g.skipped_roots().count(), 0);
    let entry = |label: &str| (0..g.entries().len()).find(|&i| g.entry_label(i) == label).unwrap_or_else(|| panic!("no entry {label}"));
    let web = entry("web");
    assert_eq!(g.entry_handoff(web), Some("Angular"));
    let server = entry("server");
    assert_eq!(g.entry_kind(server), "main");
    let users = entry("GET /users");
    assert_eq!(g.entry_parent(users), Some(server));
    assert_eq!(g.name(g.entries()[users].node.to_native()), "list");
}
