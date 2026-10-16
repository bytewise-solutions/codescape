mod gomod;
mod gradle;
mod walk;

pub use gomod::{GoMod, parse_go_mod, parse_go_work};
pub use gradle::{GradleSettings, SourceSets, parse_build_deps, parse_settings, parse_source_sets};
pub use walk::{default_excludes, rel_path, walk_files, walk_files_under};

use anyhow::Result;
use globset::GlobSet;
use srcrelief_format::{Lang, Scope};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildKind {
    Gradle,
    GoMod,
    Dirs,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiscoveredModule {
    pub name: String,
    pub dir: PathBuf,
    pub lang: Lang,
    pub build: BuildKind,
    pub go_module_path: Option<String>,
    pub source_roots: Vec<PathBuf>,
    pub test_roots: Vec<PathBuf>,
    pub dev_roots: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleLink {
    pub from: usize,
    pub to: usize,
    pub scope: Scope,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedRoot {
    pub kind: String,
    pub rel: String,
}

#[derive(Clone, Debug)]
pub struct Discovery {
    pub root: PathBuf,
    pub modules: Vec<DiscoveredModule>,
    pub links: Vec<ModuleLink>,
    pub skipped: Vec<SkippedRoot>,
}

const UNSUPPORTED_ROOTS: &[&str] = &["package.json", "pom.xml", "Cargo.toml", "pyproject.toml"];
const GRADLE_SETTINGS: &[&str] = &["settings.gradle", "settings.gradle.kts"];

pub fn discover(root: &Path, excludes: &GlobSet) -> Result<Discovery> {
    let root = root.canonicalize()?;
    let mut markers: Vec<PathBuf> = Vec::new();
    walk::walk_dirs(&root, excludes, |dir, names| {
        for marker in GRADLE_SETTINGS.iter().chain(&["go.mod", "go.work"]).chain(UNSUPPORTED_ROOTS) {
            if names.iter().any(|n| n == marker) {
                markers.push(dir.join(marker));
            }
        }
    })?;

    let mut d = Discovery { root: root.clone(), modules: Vec::new(), links: Vec::new(), skipped: Vec::new() };
    let gradle_roots = top_level(markers.iter().filter(|p| is_named(p, GRADLE_SETTINGS)));
    for build in &gradle_roots {
        discover_gradle(&root, build, &mut d);
    }
    discover_go(&root, &markers, &mut d);

    let claimed: Vec<PathBuf> = d.modules.iter().map(|m| m.dir.clone()).chain(gradle_roots.iter().cloned()).collect();
    let mut skipped_dirs: Vec<PathBuf> = Vec::new();
    for marker in markers.iter().filter(|p| is_named(p, UNSUPPORTED_ROOTS)) {
        let dir = marker.parent().unwrap().to_path_buf();
        if claimed.iter().any(|c| dir.starts_with(c)) || skipped_dirs.iter().any(|s| dir.starts_with(s)) {
            continue;
        }
        skipped_dirs.push(dir.clone());
        let kind = marker.file_name().unwrap().to_string_lossy().into_owned();
        d.skipped.push(SkippedRoot { kind, rel: rel_path(&root, &dir) });
    }
    d.skipped.sort_by(|a, b| a.rel.cmp(&b.rel));

    if d.modules.is_empty() {
        discover_dirs(&root, excludes, &skipped_dirs, &mut d)?;
    }
    Ok(d)
}

fn is_named(p: &Path, names: &[&str]) -> bool {
    p.file_name().is_some_and(|f| names.iter().any(|n| f == *n))
}

fn top_level<'a>(markers: impl Iterator<Item = &'a PathBuf>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = markers.map(|m| m.parent().unwrap().to_path_buf()).collect();
    dirs.sort();
    dirs.dedup();
    let all = dirs.clone();
    dirs.retain(|d| !all.iter().any(|o| o != d && d.starts_with(o)));
    dirs
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn build_file(dir: &Path) -> Option<PathBuf> {
    ["build.gradle", "build.gradle.kts"].iter().map(|f| dir.join(f)).find(|p| p.is_file())
}

fn gradle_module(d: &mut Discovery, name: String, dir: PathBuf) -> usize {
    let sets = build_file(&dir).map(|f| parse_source_sets(&read(&f))).unwrap_or_default();
    let (source_roots, test_roots, dev_roots) = java_roots(&dir, &sets);
    d.modules.push(DiscoveredModule { name, dir, lang: Lang::Java, build: BuildKind::Gradle, go_module_path: None, source_roots, test_roots, dev_roots });
    d.modules.len() - 1
}

fn discover_gradle(root: &Path, build: &Path, d: &mut Discovery) {
    let Some(settings_path) = GRADLE_SETTINGS.iter().map(|f| build.join(f)).find(|p| p.is_file()) else { return };
    let settings = parse_settings(&read(&settings_path));
    let prefix = if build == root { String::new() } else { format!("{}:", rel_path(root, build)) };
    let first = d.modules.len();
    let mut by_path: HashMap<String, usize> = HashMap::new();
    for include in &settings.includes {
        let rel_dir = settings
            .project_dirs
            .iter()
            .find(|(p, _)| p == include)
            .map(|(_, dir)| dir.clone())
            .unwrap_or_else(|| include.replace(':', "/"));
        let dir = build.join(rel_dir);
        if !dir.is_dir() {
            continue;
        }
        let idx = gradle_module(d, format!("{prefix}{include}"), dir);
        by_path.insert(include.clone(), idx);
    }
    let root_sets = build_file(build).map(|f| parse_source_sets(&read(&f))).unwrap_or_default();
    let (main, test, dev) = java_roots(build, &root_sets);
    if !(main.is_empty() && test.is_empty() && dev.is_empty()) {
        let fallback = build.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| "root".into());
        let name = format!("{prefix}{}", settings.root_name.clone().unwrap_or(fallback));
        let idx = gradle_module(d, name, build.to_path_buf());
        by_path.insert(String::new(), idx);
    }
    for idx in first..d.modules.len() {
        let Some(bf) = build_file(&d.modules[idx].dir) else { continue };
        for (dep, scope) in parse_build_deps(&read(&bf)) {
            if let Some(&to) = by_path.get(&dep) {
                push_link(d, idx, to, scope);
            }
        }
    }
}

fn push_link(d: &mut Discovery, from: usize, to: usize, scope: Scope) {
    if from == to || d.links.iter().any(|l| l.from == from && l.to == to && (l.scope == scope || l.scope == Scope::Main)) {
        return;
    }
    if scope == Scope::Main {
        d.links.retain(|l| !(l.from == from && l.to == to));
    }
    d.links.push(ModuleLink { from, to, scope });
}

fn existing(dir: &Path, rels: &[String]) -> Vec<PathBuf> {
    rels.iter().map(|r| dir.join(r)).filter(|p| p.is_dir()).collect()
}

fn java_roots(dir: &Path, sets: &SourceSets) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
    let declared = |name: &str| sets.sets.iter().find(|(s, _)| s == name).map(|(_, d)| existing(dir, d));
    let main = declared("main").unwrap_or_else(|| {
        let conventional = dir.join("src/main/java");
        if conventional.is_dir() {
            vec![conventional]
        } else if dir.join("src").is_dir() && !dir.join("src/main").is_dir() {
            vec![dir.join("src")]
        } else {
            Vec::new()
        }
    });
    let test = declared("test").unwrap_or_else(|| existing(dir, &["src/test/java".to_string()]));
    let dev = sets
        .sets
        .iter()
        .filter(|(s, _)| s != "main" && s != "test")
        .flat_map(|(_, d)| existing(dir, d))
        .collect();
    (main, test, dev)
}

fn discover_go(root: &Path, markers: &[PathBuf], d: &mut Discovery) {
    let work = root.join("go.work");
    let mod_dirs: Vec<PathBuf> = if work.is_file() {
        parse_go_work(&read(&work)).iter().map(|u| root.join(u)).filter(|p| p.join("go.mod").is_file()).collect()
    } else {
        let mut v: Vec<PathBuf> = markers.iter().filter(|p| is_named(p, &["go.mod"])).map(|p| p.parent().unwrap().to_path_buf()).collect();
        v.sort();
        v
    };
    let first = d.modules.len();
    let mut requires: Vec<Vec<String>> = Vec::new();
    for dir in mod_dirs {
        let dir = dir.canonicalize().unwrap_or(dir);
        let gm = parse_go_mod(&read(&dir.join("go.mod")));
        let name = if dir == root {
            root.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| ".".into())
        } else {
            rel_path(root, &dir)
        };
        requires.push(gm.requires);
        d.modules.push(DiscoveredModule {
            name,
            dir: dir.clone(),
            lang: Lang::Go,
            build: BuildKind::GoMod,
            go_module_path: gm.module,
            source_roots: vec![dir],
            test_roots: Vec::new(),
            dev_roots: Vec::new(),
        });
    }
    for (i, reqs) in requires.iter().enumerate() {
        for req in reqs {
            if let Some(to) = (first..d.modules.len()).find(|&j| d.modules[j].go_module_path.as_deref() == Some(req.as_str())) {
                push_link(d, first + i, to, Scope::Main);
            }
        }
    }
}

fn discover_dirs(root: &Path, excludes: &GlobSet, skipped: &[PathBuf], d: &mut Discovery) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(root)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !skipped.iter().any(|s| p.starts_with(s)))
        .filter(|p| !p.file_name().is_some_and(|f| f.to_string_lossy().starts_with('.')))
        .collect();
    entries.sort();
    for dir in entries {
        let (mut java, mut go) = (0usize, 0usize);
        walk_files(&dir, excludes, |p| match p.extension().and_then(|e| e.to_str()) {
            Some("java") => java += 1,
            Some("go") => go += 1,
            _ => {}
        })?;
        if java + go == 0 {
            continue;
        }
        d.modules.push(DiscoveredModule {
            name: rel_path(root, &dir),
            dir: dir.clone(),
            lang: if java >= go { Lang::Java } else { Lang::Go },
            build: BuildKind::Dirs,
            go_module_path: None,
            source_roots: vec![dir],
            test_roots: Vec::new(),
            dev_roots: Vec::new(),
        });
    }
    Ok(())
}
