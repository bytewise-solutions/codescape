use crate::{BuildKind, DiscoveredModule, Discovery, push_link, read};
use globset::{GlobBuilder, GlobMatcher};
use srcrelief_format::{Lang, Scope};
use std::path::{Path, PathBuf};
use toml::{Table, Value};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CargoCrate {
    pub lib_name: String,
    pub lib: Option<PathBuf>,
    pub bins: Vec<(String, PathBuf)>,
    pub deps: Vec<(String, usize)>,
    pub examples: Vec<(String, PathBuf)>,
    pub tests: Vec<(String, PathBuf)>,
    pub benches: Vec<(String, PathBuf)>,
    pub build_script: Option<PathBuf>,
}

struct Manifest {
    dir: PathBuf,
    table: Table,
}

fn load(path: &Path) -> Option<Manifest> {
    let dir = path.parent()?;
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let table = toml::from_str::<Table>(&read(path)).ok()?;
    Some(Manifest { dir, table })
}

fn strs(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn matcher(pattern: &str) -> Option<GlobMatcher> {
    GlobBuilder::new(pattern.trim_end_matches('/')).literal_separator(true).build().ok().map(|g| g.compile_matcher())
}

fn lists(ws: &Manifest, pkg_dir: &Path) -> bool {
    if ws.dir == pkg_dir {
        return true;
    }
    let Ok(rel) = pkg_dir.strip_prefix(&ws.dir) else { return false };
    let w = ws.table.get("workspace");
    let hit = |key: &str| strs(w.and_then(|w| w.get(key))).iter().any(|p| rel.starts_with(p.trim_end_matches('/')) || matcher(p).is_some_and(|m| m.is_match(rel)));
    hit("members") && !hit("exclude")
}

fn package_name(m: &Manifest) -> Option<&str> {
    m.table.get("package")?.get("name")?.as_str()
}

fn existing(dir: &Path, names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(|n| dir.join(n)).filter(|p| p.is_dir()).collect()
}

fn workspace_of<'m>(workspaces: &'m [Manifest], dir: &Path) -> Option<&'m Manifest> {
    workspaces.iter().filter(|w| dir.starts_with(&w.dir)).max_by_key(|w| w.dir.as_os_str().len())
}

fn find_workspace(root: &Path, dir: &Path) -> Option<Manifest> {
    let mut cur = Some(dir);
    while let Some(c) = cur {
        if let Some(m) = load(&c.join("Cargo.toml")).filter(|m| m.table.contains_key("workspace")) {
            return Some(m);
        }
        if c == root {
            break;
        }
        cur = c.parent();
    }
    None
}

fn dep_tables(t: &Table) -> impl Iterator<Item = (&Table, Scope)> {
    [("dependencies", Scope::Main), ("build-dependencies", Scope::Main), ("dev-dependencies", Scope::Test)]
        .into_iter()
        .filter_map(|(key, scope)| Some((t.get(key)?.as_table()?, scope)))
}

fn dependencies(m: &Manifest, ws: Option<&Manifest>) -> Vec<(String, PathBuf, Scope)> {
    let targets = m.table.get("target").and_then(Value::as_table).into_iter().flat_map(|t| t.values()).filter_map(Value::as_table);
    let tables: Vec<(&Table, Scope)> = dep_tables(&m.table).chain(targets.flat_map(dep_tables)).collect();
    let ws_deps = ws.and_then(|w| Some((w, w.table.get("workspace")?.get("dependencies")?.as_table()?)));
    let mut out = Vec::new();
    for (table, scope) in tables {
        for (key, v) in table {
            let path = match (v.get("path").and_then(Value::as_str), v.get("workspace").and_then(Value::as_bool)) {
                (Some(p), _) => Some(m.dir.join(p)),
                (None, Some(true)) => ws_deps.and_then(|(w, deps)| deps.get(key)?.get("path")?.as_str().map(|p| w.dir.join(p))),
                _ => None,
            };
            if let Some(p) = path {
                out.push((key.replace('-', "_"), p.canonicalize().unwrap_or(p), scope));
            }
        }
    }
    out
}

pub(crate) fn discover_cargo(markers: &[PathBuf], d: &mut Discovery) -> Vec<PathBuf> {
    let claimed: Vec<PathBuf> = d.modules.iter().map(|m| m.dir.clone()).collect();
    let mut manifests: Vec<Manifest> = markers
        .iter()
        .filter(|p| p.file_name().is_some_and(|f| f == "Cargo.toml"))
        .filter_map(|p| load(p))
        .filter(|m| !claimed.iter().any(|c| m.dir.starts_with(c)))
        .collect();
    manifests.sort_by(|a, b| a.dir.cmp(&b.dir));
    let workspaces: Vec<Manifest> = manifests.iter().filter(|m| m.table.contains_key("workspace")).map(|m| Manifest { dir: m.dir.clone(), table: m.table.clone() }).collect();
    let first = d.modules.len();
    let mut members: Vec<&Manifest> = Vec::new();
    for pkg in manifests.iter().filter(|m| package_name(m).is_some()) {
        if workspace_of(&workspaces, &pkg.dir).is_some_and(|w| !lists(w, &pkg.dir)) {
            continue;
        }
        members.push(pkg);
        let dir = pkg.dir.clone();
        d.modules.push(DiscoveredModule {
            name: package_name(pkg).unwrap().to_string(),
            source_roots: existing(&dir, &["src"]),
            test_roots: existing(&dir, &["tests"]),
            dev_roots: existing(&dir, &["examples", "benches"]),
            dir,
            lang: Lang::Rust,
            build: BuildKind::Cargo,
            go_module_path: None,
        });
    }
    for (i, pkg) in members.iter().enumerate() {
        for (_, path, scope) in dependencies(pkg, workspace_of(&workspaces, &pkg.dir)) {
            if let Some(to) = (first..d.modules.len()).find(|&j| d.modules[j].dir == path) {
                push_link(d, first + i, to, scope);
            }
        }
    }
    workspaces.into_iter().map(|w| w.dir).collect()
}

fn rs_targets(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(String, PathBuf)> = read
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter_map(|p| {
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            if p.is_file() && p.extension().is_some_and(|e| e == "rs") {
                Some((stem, p))
            } else if p.join("main.rs").is_file() {
                Some((stem, p.join("main.rs")))
            } else {
                None
            }
        })
        .collect();
    out.sort();
    out
}

pub fn cargo_crate(d: &Discovery, module: usize) -> CargoCrate {
    let m = &d.modules[module];
    let Some(manifest) = load(&m.dir.join("Cargo.toml")) else { return CargoCrate::default() };
    let t = &manifest.table;
    let pkg = package_name(&manifest).unwrap_or(&m.name).to_string();
    let lib_t = t.get("lib");
    let lib_name = lib_t.and_then(|l| l.get("name")).and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| pkg.replace('-', "_"));
    let lib = match lib_t.and_then(|l| l.get("path")).and_then(Value::as_str) {
        Some(p) => Some(m.dir.join(p)),
        None => Some(m.dir.join("src/lib.rs")).filter(|p| p.is_file()),
    };
    let mut bins: Vec<(String, PathBuf)> = Vec::new();
    for b in t.get("bin").and_then(Value::as_array).into_iter().flatten() {
        let Some(name) = b.get("name").and_then(Value::as_str) else { continue };
        let path = match b.get("path").and_then(Value::as_str) {
            Some(p) => m.dir.join(p),
            None if name == pkg && m.dir.join("src/main.rs").is_file() => m.dir.join("src/main.rs"),
            None => {
                let single = m.dir.join(format!("src/bin/{name}.rs"));
                if single.is_file() { single } else { m.dir.join(format!("src/bin/{name}/main.rs")) }
            }
        };
        bins.push((name.to_string(), path));
    }
    let autobins = t.get("package").and_then(|p| p.get("autobins")).and_then(Value::as_bool).unwrap_or(true);
    if autobins {
        let main = m.dir.join("src/main.rs");
        let auto = main.is_file().then(|| (pkg.clone(), main)).into_iter().chain(rs_targets(&m.dir.join("src/bin")));
        for (name, path) in auto {
            if !bins.iter().any(|(n, p)| *n == name || *p == path) {
                bins.push((name, path));
            }
        }
    }
    let ws = find_workspace(&d.root, &m.dir);
    let mut deps: Vec<(String, usize)> = Vec::new();
    for (alias, path, _) in dependencies(&manifest, ws.as_ref()) {
        if let Some(to) = d.modules.iter().position(|x| x.dir == path) {
            if !deps.iter().any(|(a, _)| *a == alias) {
                deps.push((alias, to));
            }
        }
    }
    CargoCrate {
        lib_name,
        lib,
        bins,
        deps,
        examples: rs_targets(&m.dir.join("examples")),
        tests: rs_targets(&m.dir.join("tests")),
        benches: rs_targets(&m.dir.join("benches")),
        build_script: Some(m.dir.join("build.rs")).filter(|p| p.is_file()),
    }
}
