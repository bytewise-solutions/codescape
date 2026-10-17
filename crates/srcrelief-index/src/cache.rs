use crate::config::ProjectConfig;
use globset::GlobSet;
use srcrelief_format::ReliefFile;
use srcrelief_rules::{Packs, detection_hash};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const DETECTION_STAMP: &str = "(detection)";
pub const INPUTS_STAMP: &str = "(inputs)";
pub const ENGINE_STAMP: &str = "(engine)";
pub const SETTINGS_STAMP: &str = "(settings)";

pub fn engine_stamp() -> &'static str {
    env!("SRCRELIEF_ENGINE_BUILD")
}

pub fn project_key(project_dir: &Path) -> String {
    let canonical = project_dir.canonicalize().unwrap_or_else(|_| project_dir.to_path_buf());
    blake3::hash(canonical.to_string_lossy().as_bytes()).to_hex()[..16].to_string()
}

pub fn cache_path(project_dir: &Path) -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("srcrelief").join(format!("{}.relief", project_key(project_dir)))
}

pub fn index_path(project_dir: &Path, cfg: &ProjectConfig) -> PathBuf {
    cfg.index.clone().unwrap_or_else(|| cache_path(project_dir))
}

fn is_input(p: &Path) -> bool {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    matches!(p.extension().and_then(|e| e.to_str()), Some("java" | "go" | "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" | "rs"))
        || matches!(
            name,
            "settings.gradle"
                | "settings.gradle.kts"
                | "build.gradle"
                | "build.gradle.kts"
                | "go.mod"
                | "go.work"
                | "package.json"
                | "tsconfig.json"
                | "pnpm-workspace.yaml"
                | "Cargo.toml"
        )
}

pub fn inputs_fingerprint(project_dir: &Path, excludes: &GlobSet) -> String {
    let mut entries: Vec<(String, u64, u128)> = Vec::new();
    let _ = srcrelief_build::walk_files(project_dir, excludes, |p| {
        if is_input(p)
            && let Ok(m) = p.metadata()
        {
            let mtime = m.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
            entries.push((srcrelief_build::rel_path(project_dir, p), m.len(), mtime));
        }
    });
    entries.sort();
    let mut h = blake3::Hasher::new();
    for (p, len, mtime) in &entries {
        h.update(p.as_bytes());
        h.update(&len.to_le_bytes());
        h.update(&mtime.to_le_bytes());
    }
    h.finalize().to_hex()[..16].to_string()
}

pub fn needs_reindex(project_dir: &Path, index: &Path, cfg: &ProjectConfig, packs: &Packs) -> bool {
    let Ok(file) = ReliefFile::open(index) else { return true };
    let g = file.graph();
    let current = detection_hash(packs);
    if !g.provenance().any(|(n, _, h)| n == DETECTION_STAMP && h == current) {
        return true;
    }
    if !g.provenance().any(|(n, _, h)| n == ENGINE_STAMP && h == engine_stamp()) {
        return true;
    }
    if !g.provenance().any(|(n, _, h)| n == SETTINGS_STAMP && h == cfg.fingerprint()) {
        return true;
    }
    let dir = project_dir.canonicalize().unwrap_or_else(|_| project_dir.to_path_buf());
    let inputs = inputs_fingerprint(&dir, &crate::pipeline::excludes(cfg));
    !g.provenance().any(|(n, _, h)| n == INPUTS_STAMP && h == inputs)
}
