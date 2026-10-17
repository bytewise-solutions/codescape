use crate::{BuildKind, DiscoveredModule, Discovery, is_named, push_link, read, rel_path, walk_files_under};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde_json::Value;
use srcrelief_format::{Lang, Scope};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const NPM_SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];
pub const NPM_EXCLUDED_DIRS: &[&str] = &["node_modules", "dist", "build", "out", "coverage", ".angular", ".next", ".nuxt", ".svelte-kit"];

pub fn is_npm_source(path: &Path) -> bool {
    let name = path.file_name().map(|f| f.to_string_lossy()).unwrap_or_default();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    NPM_SOURCE_EXTENSIONS.contains(&ext) && !name.ends_with(".d.ts") && !name.ends_with(".d.mts") && !name.ends_with(".d.cts") && !name.ends_with(".min.js")
}

pub fn is_npm_tooling_file(path: &Path) -> bool {
    let name = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = name.split('.').next().unwrap_or("");
    name.contains(".config.") || name.contains(".conf.") || stem.ends_with("rc") || matches!(stem, "Gruntfile" | "gulpfile" | "Jakefile")
}

pub fn in_npm_excluded_dir(module_dir: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(module_dir).unwrap_or(path);
    let parent = rel.parent().unwrap_or(Path::new(""));
    parent.components().any(|c| NPM_EXCLUDED_DIRS.iter().any(|x| c.as_os_str() == *x))
}

pub fn read_json_loose(path: &Path) -> Option<Value> {
    let src = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&src).ok().or_else(|| serde_json::from_str(&strip_jsonc(&src)).ok())
}

fn strip_jsonc(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(b.len());
                out.push_str(&src[start..i]);
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b',' => {
                let next = src[i + 1..].trim_start();
                if !(next.starts_with('}') || next.starts_with(']')) {
                    out.push(',');
                }
                i += 1;
            }
            _ => {
                let ch = src[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

fn string_list(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        Some(Value::Object(o)) => string_list(o.get("packages")),
        _ => Vec::new(),
    }
}

fn pnpm_packages(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in src.lines() {
        let t = line.split('#').next().unwrap_or("").trim_end();
        if t.trim().is_empty() {
            continue;
        }
        if !line.starts_with([' ', '\t', '-']) {
            inside = t.trim() == "packages:";
            continue;
        }
        if let Some(item) = t.trim().strip_prefix('-').filter(|_| inside) {
            out.push(item.trim().trim_matches(['\'', '"']).to_string());
        }
    }
    out
}

struct Workspace {
    dir: PathBuf,
    include: GlobSet,
    exclude: GlobSet,
}

impl Workspace {
    fn new(dir: &Path, patterns: &[String]) -> Workspace {
        let (mut inc, mut exc) = (GlobSetBuilder::new(), GlobSetBuilder::new());
        for p in patterns {
            let (set, p) = match p.strip_prefix('!') {
                Some(rest) => (&mut exc, rest),
                None => (&mut inc, p.as_str()),
            };
            let p = p.trim_start_matches("./").trim_end_matches('/');
            if let Ok(g) = GlobBuilder::new(p).literal_separator(true).build() {
                set.add(g);
            }
        }
        Workspace { dir: dir.to_path_buf(), include: inc.build().unwrap_or_else(|_| GlobSet::empty()), exclude: exc.build().unwrap_or_else(|_| GlobSet::empty()) }
    }

    fn has_member(&self, dir: &Path) -> bool {
        dir != self.dir && dir.starts_with(&self.dir) && {
            let rel = rel_path(&self.dir, dir);
            self.include.is_match(&rel) && !self.exclude.is_match(&rel)
        }
    }
}

struct Own {
    sources: usize,
    typescript: bool,
}

fn own_sources(root: &Path, dir: &Path, nested: &[&PathBuf], excludes: &GlobSet) -> Own {
    let mut own = Own { sources: 0, typescript: false };
    let _ = walk_files_under(root, dir, excludes, |p| {
        if !is_npm_source(p) || in_npm_excluded_dir(dir, p) || nested.iter().any(|n| p.starts_with(n)) || is_npm_tooling_file(p) {
            return;
        }
        own.sources += 1;
        own.typescript |= matches!(p.extension().and_then(|e| e.to_str()), Some("ts" | "tsx" | "mts" | "cts"));
    });
    own
}

pub(crate) fn discover_npm(root: &Path, markers: &[PathBuf], excludes: &GlobSet, d: &mut Discovery) -> Vec<PathBuf> {
    let gradle_settings = ["settings.gradle", "settings.gradle.kts"];
    let claimed: Vec<PathBuf> = d
        .modules
        .iter()
        .filter(|m| matches!(m.build, BuildKind::GoMod | BuildKind::Gradle))
        .map(|m| m.dir.clone())
        .chain(markers.iter().filter(|p| is_named(p, &gradle_settings)).map(|p| p.parent().unwrap().to_path_buf()))
        .collect();
    let mut dirs: Vec<PathBuf> = markers
        .iter()
        .filter(|p| is_named(p, &["package.json"]))
        .map(|p| p.parent().unwrap().to_path_buf())
        .filter(|dir| !claimed.iter().any(|c| dir.starts_with(c)))
        .collect();
    dirs.sort();
    let manifests: HashMap<PathBuf, Value> = dirs.iter().map(|dir| (dir.clone(), read_json_loose(&dir.join("package.json")).unwrap_or(Value::Null))).collect();
    let workspaces: Vec<Workspace> = dirs
        .iter()
        .filter_map(|dir| {
            let mut patterns = string_list(manifests[dir].get("workspaces"));
            let pnpm = dir.join("pnpm-workspace.yaml");
            if pnpm.is_file() {
                patterns.extend(pnpm_packages(&read(&pnpm)));
            }
            (!patterns.is_empty()).then(|| Workspace::new(dir, &patterns))
        })
        .collect();
    let is_member = |dir: &Path| workspaces.iter().any(|w| w.has_member(dir));
    let first = d.modules.len();
    let mut accepted: Vec<PathBuf> = Vec::new();
    let mut workspace_roots: Vec<PathBuf> = Vec::new();
    let mut npm_names: HashMap<String, usize> = HashMap::new();
    for dir in &dirs {
        if accepted.iter().any(|a| dir.starts_with(a)) && !is_member(dir) {
            continue;
        }
        let nested: Vec<&PathBuf> = dirs.iter().filter(|n| *n != dir && n.starts_with(dir)).collect();
        let own = own_sources(root, dir, &nested, excludes);
        if own.sources == 0 {
            if workspaces.iter().any(|w| &w.dir == dir) {
                workspace_roots.push(dir.clone());
            }
            continue;
        }
        let manifest = &manifests[dir];
        let npm_name = manifest.get("name").and_then(|n| n.as_str()).map(String::from);
        let folder = dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| ".".into());
        let mut name = npm_name.clone().unwrap_or(folder);
        if d.modules.iter().any(|m| m.name == name) {
            name = rel_path(root, dir);
        }
        let lang = if own.typescript || dir.join("tsconfig.json").is_file() { Lang::TypeScript } else { Lang::JavaScript };
        if let Some(n) = npm_name {
            npm_names.insert(n, d.modules.len());
        }
        d.modules.push(DiscoveredModule {
            name,
            dir: dir.clone(),
            lang,
            build: BuildKind::Npm,
            go_module_path: None,
            source_roots: vec![dir.clone()],
            test_roots: Vec::new(),
            dev_roots: Vec::new(),
        });
        accepted.push(dir.clone());
    }
    for idx in first..d.modules.len() {
        let manifest = &manifests[&d.modules[idx].dir];
        let scoped = [("dependencies", Scope::Main), ("peerDependencies", Scope::Main), ("optionalDependencies", Scope::Main), ("devDependencies", Scope::Test)];
        for (key, scope) in scoped {
            let Some(Value::Object(deps)) = manifest.get(key) else { continue };
            for dep in deps.keys() {
                if let Some(&to) = npm_names.get(dep) {
                    push_link(d, idx, to, scope);
                }
            }
        }
    }
    workspace_roots.retain(|w| accepted.iter().any(|a| a.starts_with(w)));
    workspace_roots
}
