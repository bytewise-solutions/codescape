use codescape_build::*;
use codescape_format::{Lang, Scope};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name).canonicalize().unwrap()
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
    d.modules.iter().find(|m| m.name == name).unwrap()
}

#[test]
fn gradle_groovy_modules_and_scoped_links() {
    let d = discover(&fixture("java-gradle"), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["app", "core", "util"]);
    assert_eq!(
        links(&d),
        [("app".into(), "core".into(), Scope::Main), ("core".into(), "app".into(), Scope::Test), ("core".into(), "util".into(), Scope::Main),]
    );
    assert!(d.modules.iter().all(|m| m.lang == Lang::Java && m.build == BuildKind::Gradle));
}

#[test]
fn gradle_kts_nested_include() {
    let root = fixture("java-gradle-kts");
    let d = discover(&root, &default_excludes()).unwrap();
    assert_eq!(names(&d), ["app", "libs:json"]);
    assert_eq!(module(&d, "libs:json").dir, root.join("libs/json"));
    assert_eq!(
        links(&d),
        [("app".into(), "app".into(), Scope::Test), ("app".into(), "libs:json".into(), Scope::Main)].into_iter().filter(|l| l.0 != l.1).collect::<Vec<_>>()
    );
}

#[test]
fn gradle_source_roots() {
    let root = fixture("java-gradle");
    let d = discover(&root, &default_excludes()).unwrap();
    assert_eq!(module(&d, "app").source_roots, [root.join("app/src/main/java")]);
    assert_eq!(module(&d, "util").source_roots, [root.join("util/src")]);
    assert_eq!(module(&d, "core").test_roots, [root.join("core/src/test/java")]);
}

#[test]
fn go_walk_links_with_and_without_replace() {
    let d = discover(&fixture("go-mono"), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["shared", "svc-a", "svc-b"]);
    assert_eq!(links(&d), [("svc-a".into(), "shared".into(), Scope::Main), ("svc-b".into(), "shared".into(), Scope::Main)]);
    assert_eq!(module(&d, "svc-a").go_module_path.as_deref(), Some("example.com/mono/svc-a"));
    assert_eq!(d.skipped, [SkippedRoot { kind: "package.json".into(), rel: "web".into() }]);
}

#[test]
fn go_work_takes_precedence() {
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&fixture("go-mono"), tmp.path());
    std::fs::write(tmp.path().join("go.work"), "go 1.22\n\nuse (\n\t./svc-a\n\t./shared\n)\n").unwrap();
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["shared", "svc-a"]);
}

#[test]
fn mixed_project_has_both() {
    let d = discover(&fixture("mixed"), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["lib", "tools/gen"]);
    assert_eq!(module(&d, "tools/gen").lang, Lang::Go);
    assert_eq!(module(&d, "lib").lang, Lang::Java);
}

#[test]
fn never_looks_above_root() {
    let d = discover(&fixture("go-mono").join("svc-a"), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["svc-a"]);
    assert!(d.links.is_empty());
}

#[test]
fn fallback_top_level_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("a")).unwrap();
    std::fs::create_dir_all(tmp.path().join("b")).unwrap();
    std::fs::write(tmp.path().join("a/X.java"), "class X {}").unwrap();
    std::fs::write(tmp.path().join("b/y.go"), "package b").unwrap();
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["a", "b"]);
    assert_eq!(module(&d, "a").lang, Lang::Java);
    assert_eq!(module(&d, "b").lang, Lang::Go);
    assert!(d.modules.iter().all(|m| m.build == BuildKind::Dirs));
}

#[test]
fn excluded_dirs_are_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&fixture("go-mono"), tmp.path());
    std::fs::create_dir_all(tmp.path().join("svc-b/vendor/x")).unwrap();
    std::fs::write(tmp.path().join("svc-b/vendor/x/go.mod"), "module x\n").unwrap();
    std::fs::create_dir_all(tmp.path().join("web/node_modules/p")).unwrap();
    std::fs::write(tmp.path().join("web/node_modules/p/package.json"), "{}").unwrap();
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["shared", "svc-a", "svc-b"]);
    assert_eq!(d.skipped.len(), 1);
}

#[test]
fn parse_gradle_settings_forms() {
    let s = "include 'a', \"b\"\ninclude(\":c:d\")\ninclude ':e'\nproject(':e').projectDir = file('modules/e')\n/* include 'zz' */";
    let parsed = parse_settings(s);
    assert_eq!(parsed.includes, ["a", "b", "c:d", "e"]);
    assert_eq!(parsed.project_dirs, [("e".to_string(), "modules/e".to_string())]);
}

#[test]
fn parse_gradle_deps_forms() {
    let s = "dependencies {\n implementation project(':a')\n api(project(\":b\"))\n testImplementation project(path: ':c')\n compileOnly project(':d')\n}";
    assert_eq!(parse_build_deps(s), [("a".to_string(), Scope::Main), ("b".into(), Scope::Main), ("c".into(), Scope::Test), ("d".into(), Scope::Main)]);
}

#[test]
fn parse_go_mod_forms() {
    let s = "module m/x\n\nrequire (\n\tm/y v1\n\tgithub.com/z v2 // indirect\n)\nrequire m/w v3\nreplace m/y => ../y\n";
    let g = parse_go_mod(s);
    assert_eq!(g.module.as_deref(), Some("m/x"));
    assert_eq!(g.requires, ["m/y", "github.com/z", "m/w"]);
    assert_eq!(g.replaces, [("m/y".to_string(), "../y".to_string())]);
}

fn copy_dir(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&dest).unwrap();
            copy_dir(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn multi_line_include_is_read() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "settings.gradle.kts", "rootProject.name = \"m\"\ninclude(\n    \":a\",\n    \":b\",\n)\n");
    write(tmp.path(), "a/build.gradle.kts", "dependencies {\n    implementation(project(\":b\"))\n}\n");
    write(tmp.path(), "a/src/main/java/a/A.java", "package a; class A {}");
    write(tmp.path(), "b/src/main/java/b/B.java", "package b; class B {}");
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["a", "b"]);
    assert!(d.modules.iter().all(|m| m.build == BuildKind::Gradle));
    assert_eq!(links(&d), [("a".to_string(), "b".to_string(), Scope::Main)]);
    let groovy = parse_settings("include 'x',\n        'y'\n");
    assert_eq!(groovy.includes, ["x", "y"]);
}

#[test]
fn single_project_gradle_build_is_one_module_with_tests_apart() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "settings.gradle.kts", "rootProject.name = \"single\"\n");
    write(tmp.path(), "build.gradle.kts", "plugins { java }\n");
    write(tmp.path(), "src/main/java/a/App.java", "package a; class App {}");
    write(tmp.path(), "src/test/java/a/AppTest.java", "package a; class AppTest {}");
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["single"]);
    let m = module(&d, "single");
    assert_eq!(m.build, BuildKind::Gradle);
    assert_eq!(m.source_roots, [tmp.path().canonicalize().unwrap().join("src/main/java")]);
    assert_eq!(m.test_roots, [tmp.path().canonicalize().unwrap().join("src/test/java")]);
}

#[test]
fn root_project_sources_join_the_included_modules() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "settings.gradle", "rootProject.name = 'rootsrc'\ninclude ':lib'\n");
    write(tmp.path(), "build.gradle", "dependencies {\n    implementation project(':lib')\n}\n");
    write(tmp.path(), "src/main/java/r/Main.java", "package r; class Main {}");
    write(tmp.path(), "lib/src/main/java/l/L.java", "package l; class L {}");
    let d = discover(tmp.path(), &default_excludes()).unwrap();
    assert_eq!(names(&d), ["lib", "rootsrc"]);
    assert_eq!(links(&d), [("rootsrc".to_string(), "lib".to_string(), Scope::Main)]);
}

#[test]
fn excludes_match_paths_relative_to_the_walk_base() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "b/src/main/java/b/B.java", "class B {}");
    write(tmp.path(), "b/src/main/java/keep/K.java", "class K {}");
    let mut gs = globset::GlobSetBuilder::new();
    gs.add(globset::Glob::new("b/src/main/java/b/**").unwrap());
    let ex = gs.build().unwrap();
    let mut seen = Vec::new();
    walk_files_under(tmp.path(), &tmp.path().join("b/src/main/java"), &ex, |p| seen.push(p.file_name().unwrap().to_string_lossy().into_owned())).unwrap();
    assert_eq!(seen, ["K.java"]);
}

#[test]
#[cfg(unix)]
fn unreadable_directories_are_skipped() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&fixture("go-mono"), tmp.path());
    let locked = tmp.path().join("locked/secret");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let d = discover(tmp.path(), &default_excludes());
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(names(&d.unwrap()), ["shared", "svc-a", "svc-b"]);
}
