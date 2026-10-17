use srcrelief_build::*;
use srcrelief_format::{Lang, Scope};
use std::path::Path;

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn names(d: &Discovery) -> Vec<&str> {
    let mut v: Vec<&str> = d.modules.iter().map(|m| m.name.as_str()).collect();
    v.sort();
    v
}

fn links(d: &Discovery) -> Vec<(String, String, Scope)> {
    let mut v: Vec<_> = d.links.iter().map(|l| (d.modules[l.from].name.clone(), d.modules[l.to].name.clone(), l.scope)).collect();
    v.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    v
}

fn index(d: &Discovery, name: &str) -> usize {
    d.modules.iter().position(|m| m.name == name).unwrap_or_else(|| panic!("no module {name}"))
}

fn workspace() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    write(
        r,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\", \"tools/gen\"]\nexclude = [\"crates/skip\"]\n\n[workspace.dependencies]\ncore-lib = { path = \"crates/core\" }\nutil-x = { package = \"util\", path = \"crates/util\" }\nserde = \"1\"\n",
    );
    write(r, "crates/core/Cargo.toml", "[package]\nname = \"core-lib\"\n\n[dependencies]\nserde.workspace = true\n");
    write(r, "crates/core/src/lib.rs", "pub fn f() {}\n");
    write(r, "crates/util/Cargo.toml", "[package]\nname = \"util\"\n");
    write(r, "crates/util/src/lib.rs", "");
    write(
        r,
        "crates/app/Cargo.toml",
        "[package]\nname = \"app\"\n\n[dependencies]\ncore-lib.workspace = true\nhelpers = { package = \"util\", path = \"../util\" }\n\n[dev-dependencies]\ngen = { path = \"../../tools/gen\" }\n",
    );
    write(r, "crates/app/src/main.rs", "fn main() {}\n");
    write(r, "crates/app/src/bin/extra.rs", "fn main() {}\n");
    write(r, "crates/app/tests/it.rs", "");
    write(r, "crates/app/examples/demo.rs", "fn main() {}\n");
    write(r, "crates/app/benches/b.rs", "");
    write(r, "crates/app/target/debug/junk.rs", "");
    write(r, "crates/skip/Cargo.toml", "[package]\nname = \"skip\"\n");
    write(r, "crates/skip/src/lib.rs", "");
    write(r, "tools/gen/Cargo.toml", "[package]\nname = \"gen\"\n\n[build-dependencies]\nutil-x.workspace = true\n");
    write(r, "tools/gen/src/lib.rs", "");
    tmp
}

#[test]
fn workspace_members_are_modules() {
    let tmp = workspace();
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["app", "core-lib", "gen", "util"]);
    assert!(d.modules.iter().all(|m| m.lang == Lang::Rust && m.build == BuildKind::Cargo));
    assert!(d.skipped.is_empty(), "{:?}", d.skipped);
}

#[test]
fn workspace_roots() {
    let tmp = workspace();
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    let app = &d.modules[index(&d, "app")];
    let dir = tmp.path().canonicalize().unwrap().join("crates/app");
    assert_eq!(app.dir, dir);
    assert_eq!(app.source_roots, [dir.join("src")]);
    assert_eq!(app.test_roots, [dir.join("tests")]);
    assert_eq!(app.dev_roots, [dir.join("examples"), dir.join("benches")]);
    assert!(d.modules[index(&d, "util")].test_roots.is_empty());
}

#[test]
fn path_and_workspace_dependencies_link_crates() {
    let tmp = workspace();
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(
        links(&d),
        [
            ("app".into(), "core-lib".into(), Scope::Main),
            ("app".into(), "gen".into(), Scope::Test),
            ("app".into(), "util".into(), Scope::Main),
            ("gen".into(), "util".into(), Scope::Main),
        ]
    );
}

#[test]
fn crate_targets_and_extern_names() {
    let tmp = workspace();
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    let app = index(&d, "app");
    let c = cargo_crate(&d, app);
    let dir = &d.modules[app].dir;
    assert_eq!(c.lib_name, "app");
    assert_eq!(c.lib, None);
    assert_eq!(c.bins, [("app".to_string(), dir.join("src/main.rs")), ("extra".to_string(), dir.join("src/bin/extra.rs"))]);
    let mut deps: Vec<(String, &str)> = c.deps.iter().map(|(a, m)| (a.clone(), d.modules[*m].name.as_str())).collect();
    deps.sort();
    assert_eq!(deps, [("core_lib".to_string(), "core-lib"), ("gen".to_string(), "gen"), ("helpers".to_string(), "util")]);
    let core = cargo_crate(&d, index(&d, "core-lib"));
    assert_eq!(core.lib_name, "core_lib");
    assert_eq!(core.lib, Some(d.modules[index(&d, "core-lib")].dir.join("src/lib.rs")));
    assert!(core.bins.is_empty());
}

#[test]
fn declared_lib_and_bin_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    write(r, "Cargo.toml", "[package]\nname = \"tool\"\n\n[lib]\nname = \"toolkit\"\npath = \"src/kit.rs\"\n\n[[bin]]\nname = \"tool-cli\"\npath = \"src/main.rs\"\n");
    write(r, "src/kit.rs", "");
    write(r, "src/main.rs", "fn main() {}\n");
    let d = discover(r, &default_excludes()).unwrap();
    assert_eq!(names(&d), ["tool"]);
    let c = cargo_crate(&d, 0);
    assert_eq!(c.lib_name, "toolkit");
    assert_eq!(c.lib, Some(d.modules[0].dir.join("src/kit.rs")));
    assert_eq!(c.bins, [("tool-cli".to_string(), d.modules[0].dir.join("src/main.rs"))]);
}

#[test]
fn standalone_packages_and_package_workspace_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    write(r, "server/Cargo.toml", "[package]\nname = \"server\"\n\n[workspace]\nmembers = [\"proto\"]\n\n[dependencies]\nproto = { path = \"proto\" }\n");
    write(r, "server/src/main.rs", "fn main() {}\n");
    write(r, "server/proto/Cargo.toml", "[package]\nname = \"proto\"\n");
    write(r, "server/proto/src/lib.rs", "");
    write(r, "cli/Cargo.toml", "[package]\nname = \"cli\"\n\n[dependencies]\nproto = { path = \"../server/proto\" }\n");
    write(r, "cli/src/main.rs", "fn main() {}\n");
    write(r, "web/package.json", "{}");
    let d = discover(r, &default_excludes()).unwrap();
    assert_eq!(names(&d), ["cli", "proto", "server"]);
    assert_eq!(links(&d), [("cli".into(), "proto".into(), Scope::Main), ("server".into(), "proto".into(), Scope::Main)]);
    assert_eq!(d.skipped, [SkippedRoot { kind: "package.json".into(), rel: "web".into() }]);
}

#[test]
fn cargo_does_not_claim_go_module_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    write(r, "svc/go.mod", "module example.com/svc\n");
    write(r, "svc/main.go", "package main\n");
    write(r, "svc/ffi/Cargo.toml", "[package]\nname = \"ffi\"\n");
    write(r, "svc/ffi/src/lib.rs", "");
    write(r, "lib/Cargo.toml", "[package]\nname = \"lib\"\n");
    write(r, "lib/src/lib.rs", "");
    let d = discover(r, &default_excludes()).unwrap();
    assert_eq!(names(&d), ["lib", "svc"]);
}
