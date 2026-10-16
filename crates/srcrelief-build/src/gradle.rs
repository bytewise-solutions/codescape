use regex::Regex;
use srcrelief_format::Scope;
use std::sync::LazyLock;

#[derive(Debug, Default, PartialEq)]
pub struct GradleSettings {
    pub includes: Vec<String>,
    pub project_dirs: Vec<(String, String)>,
    pub root_name: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct SourceSets {
    pub sets: Vec<(String, Vec<String>)>,
}

static BLOCK_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
static LINE_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)//.*$").unwrap());
static INCLUDE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*include\b").unwrap());
static ROOT_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"rootProject\.name\s*=\s*['"]([^'"]+)['"]"#).unwrap());
static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"['"]([^'"]+)['"]"#).unwrap());
static PROJECT_DIR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"project\(\s*['"]([^'"]+)['"]\s*\)\.projectDir\s*=\s*(?:file|new File)\(\s*(?:[^,()]*,\s*)?['"]([^'"]+)['"]"#).unwrap());
static DEP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*(\w+)\s*\(?\s*project\s*\(\s*(?:path\s*:\s*)?['"]([^'"]+)['"]"#).unwrap()
});
static SRC_FLAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"sourceSets\.(\w+)\.java\.srcDirs?\s*=\s*\[([^\]]*)\]").unwrap());
static SRC_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\b(\w+)\s*\{[^{}]*?java\.srcDirs?\s*=\s*\[([^\]]*)\]").unwrap());

pub fn strip_comments(s: &str) -> String {
    let s = BLOCK_COMMENT.replace_all(s, "");
    LINE_COMMENT.replace_all(&s, "").into_owned()
}

fn normalize_path(p: &str) -> String {
    p.trim_start_matches(':').to_string()
}

pub fn parse_settings(src: &str) -> GradleSettings {
    let src = strip_comments(src);
    let mut out = GradleSettings::default();
    out.root_name = ROOT_NAME.captures(&src).map(|c| c[1].to_string());
    for m in INCLUDE.find_iter(&src) {
        let args = include_arguments(&src[m.end()..]);
        for q in QUOTED.captures_iter(args) {
            let name = normalize_path(&q[1]);
            if !name.is_empty() && !out.includes.contains(&name) {
                out.includes.push(name);
            }
        }
    }
    for cap in PROJECT_DIR.captures_iter(&src) {
        out.project_dirs.push((normalize_path(&cap[1]), cap[2].to_string()));
    }
    out
}

fn include_arguments(rest: &str) -> &str {
    let trimmed = rest.trim_start_matches([' ', '\t']);
    if let Some(inner) = trimmed.strip_prefix('(') {
        let end = inner.find(')').unwrap_or(inner.len());
        return &inner[..end];
    }
    let mut end = 0;
    for line in trimmed.split_inclusive('\n') {
        end += line.len();
        if !line.trim_end().ends_with(',') {
            break;
        }
    }
    &trimmed[..end]
}

pub fn parse_build_deps(src: &str) -> Vec<(String, Scope)> {
    let src = strip_comments(src);
    let mut out = Vec::new();
    for cap in DEP.captures_iter(&src) {
        let config = cap[1].to_ascii_lowercase();
        if config == "project" {
            continue;
        }
        let scope = if config.contains("test") { Scope::Test } else { Scope::Main };
        let dep = (normalize_path(&cap[2]), scope);
        if !out.contains(&dep) {
            out.push(dep);
        }
    }
    out
}

pub fn parse_source_sets(src: &str) -> SourceSets {
    let src = strip_comments(src);
    let mut out = SourceSets::default();
    let mut add = |set: &str, list: &str| {
        let dirs: Vec<String> = QUOTED.captures_iter(list).map(|q| q[1].trim_end_matches('/').to_string()).collect();
        if dirs.is_empty() || set == "resources" {
            return;
        }
        match out.sets.iter_mut().find(|(s, _)| s == set) {
            Some((_, existing)) => existing.extend(dirs),
            None => out.sets.push((set.to_string(), dirs)),
        }
    };
    for cap in SRC_FLAT.captures_iter(&src) {
        add(&cap[1], &cap[2]);
    }
    for cap in SRC_BLOCK.captures_iter(&src) {
        if &cap[1] != "sourceSets" && &cap[1] != "java" {
            add(&cap[1], &cap[2]);
        }
    }
    out
}
