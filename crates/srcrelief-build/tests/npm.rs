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

fn module<'a>(d: &'a Discovery, name: &str) -> &'a DiscoveredModule {
    d.modules.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("no module {name}"))
}

#[test]
fn package_with_typescript_sources_is_a_typescript_module() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "web/package.json", r#"{ "name": "web-app", "dependencies": { "rxjs": "7" } }"#);
    write(tmp.path(), "web/src/main.ts", "console.log(1);");
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["web-app"]);
    let m = module(&d, "web-app");
    assert_eq!(m.lang, Lang::TypeScript);
    assert_eq!(m.build, BuildKind::Npm);
    assert_eq!(m.source_roots, [tmp.path().canonicalize().unwrap().join("web")]);
    assert!(d.skipped.is_empty(), "{:?}", d.skipped);
}

#[test]
fn package_without_typescript_is_javascript_and_tsconfig_makes_typescript() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "js/package.json", r#"{ "name": "plain" }"#);
    write(tmp.path(), "js/index.js", "module.exports = {};");
    write(tmp.path(), "js/types/index.d.ts", "export {};");
    write(tmp.path(), "cfg/package.json", "{}");
    write(tmp.path(), "cfg/tsconfig.json", "{}");
    write(tmp.path(), "cfg/lib/a.mjs", "export const a = 1;");
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["cfg", "plain"]);
    assert_eq!(module(&d, "plain").lang, Lang::JavaScript);
    assert_eq!(module(&d, "cfg").lang, Lang::TypeScript);
}

#[test]
fn package_without_sources_stays_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "svc/go.mod", "module example.com/svc\n");
    write(tmp.path(), "svc/main.go", "package main\nfunc main() {}\n");
    write(tmp.path(), "tooling/package.json", r#"{ "name": "tooling" }"#);
    write(tmp.path(), "tooling/dist/bundle.js", "x()");
    write(tmp.path(), "tooling/index.html", "<html></html>");
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["svc"]);
    assert_eq!(d.skipped, [SkippedRoot { kind: "package.json".into(), rel: "tooling".into() }]);
}

#[test]
fn workspaces_link_members_and_a_root_without_sources_is_not_a_module() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "package.json", r#"{ "name": "mono", "private": true, "workspaces": ["packages/*", "apps/web"] }"#);
    write(tmp.path(), "packages/core/package.json", r#"{ "name": "@acme/core" }"#);
    write(tmp.path(), "packages/core/src/index.ts", "export const x = 1;");
    write(tmp.path(), "packages/ui/package.json", r#"{ "name": "@acme/ui", "peerDependencies": { "@acme/core": "*" } }"#);
    write(tmp.path(), "packages/ui/src/index.tsx", "export const y = 1;");
    write(tmp.path(), "apps/web/package.json", r#"{ "name": "web", "dependencies": { "@acme/ui": "workspace:*" }, "devDependencies": { "@acme/core": "*" } }"#);
    write(tmp.path(), "apps/web/main.js", "require('@acme/ui');");
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["@acme/core", "@acme/ui", "web"]);
    assert_eq!(
        links(&d),
        [
            ("@acme/ui".into(), "@acme/core".into(), Scope::Main),
            ("web".into(), "@acme/core".into(), Scope::Test),
            ("web".into(), "@acme/ui".into(), Scope::Main),
        ]
    );
    assert!(d.skipped.is_empty(), "{:?}", d.skipped);
}

#[test]
fn workspace_root_with_its_own_sources_is_a_module_too() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "package.json", r#"{ "name": "root", "workspaces": { "packages": ["libs/*"] } }"#);
    write(tmp.path(), "server.js", "require('./libs/a');");
    write(tmp.path(), "libs/a/package.json", r#"{ "name": "a" }"#);
    write(tmp.path(), "libs/a/index.js", "module.exports = 1;");
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["a", "root"]);
}

#[test]
fn pnpm_workspace_yaml_lists_members() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "package.json", r#"{ "name": "mono" }"#);
    write(tmp.path(), "scripts/release.mjs", "");
    write(tmp.path(), "pnpm-workspace.yaml", "packages:\n  - 'apps/*'\n  - \"!apps/legacy\"\n");
    write(tmp.path(), "apps/one/package.json", r#"{ "name": "one" }"#);
    write(tmp.path(), "apps/one/a.ts", "export {};");
    write(tmp.path(), "apps/legacy/package.json", r#"{ "name": "legacy" }"#);
    write(tmp.path(), "apps/legacy/a.ts", "export {};");
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["mono", "one"], "apps/legacy is left out of the workspace, so the root holds it");
}

#[test]
fn nested_packages_belong_to_the_enclosing_module_unless_workspace_members() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "app/package.json", r#"{ "name": "app" }"#);
    write(tmp.path(), "app/src/a.ts", "export {};");
    write(tmp.path(), "app/src/fixtures/pkg/package.json", r#"{ "name": "fixture" }"#);
    write(tmp.path(), "app/src/fixtures/pkg/index.js", "");
    write(tmp.path(), "app/node_modules/dep/package.json", r#"{ "name": "dep" }"#);
    write(tmp.path(), "app/node_modules/dep/index.js", "");
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["app"]);
    assert!(d.skipped.is_empty(), "{:?}", d.skipped);
}

#[test]
fn directories_of_gradle_and_go_modules_are_not_claimed() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "go.mod", "module example.com/root\n");
    write(tmp.path(), "main.go", "package main\nfunc main() {}\n");
    write(tmp.path(), "web/package.json", r#"{ "name": "web" }"#);
    write(tmp.path(), "web/a.ts", "export {};");
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert!(d.modules.iter().all(|m| m.build == BuildKind::GoMod), "{:?}", names(&d));
}

#[test]
fn unnamed_package_takes_its_folder_name() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "tools/gen/package.json", "{ \"version\": \"1.0.0\" }");
    write(tmp.path(), "tools/gen/gen.cjs", "");
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["gen"]);
}
