use crate::config::ProjectConfig;
use globset::GlobSet;
use srcrelief_format::ReliefFile;
use srcrelief_rules::{Packs, detection_hash};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const DETECTION_STAMP: &str = "(detection)";

pub fn cache_path(project_dir: &Path) -> PathBuf {
    let canonical = project_dir.canonicalize().unwrap_or_else(|_| project_dir.to_path_buf());
    let key = blake3::hash(canonical.to_string_lossy().as_bytes()).to_hex();
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("srcrelief").join(format!("{}.relief", &key[..16]))
}

pub fn index_path(project_dir: &Path, cfg: &ProjectConfig) -> PathBuf {
    cfg.index.clone().unwrap_or_else(|| cache_path(project_dir))
}

fn is_input(p: &Path) -> bool {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    matches!(p.extension().and_then(|e| e.to_str()), Some("java" | "go"))
        || matches!(name, "settings.gradle" | "settings.gradle.kts" | "build.gradle" | "build.gradle.kts" | "go.mod" | "go.work" | crate::config::CONFIG_FILE)
}

pub fn newest_input(project_dir: &Path, excludes: &GlobSet) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    let _ = srcrelief_build::walk_files(project_dir, excludes, |p| {
        if is_input(p) {
            if let Ok(t) = p.metadata().and_then(|m| m.modified()) {
                newest = Some(newest.map_or(t, |n| n.max(t)));
            }
        }
    });
    newest
}

pub fn needs_reindex(project_dir: &Path, index: &Path, cfg: &ProjectConfig, packs: &Packs) -> bool {
    let Ok(file) = ReliefFile::open(index) else { return true };
    let g = file.graph();
    let current = detection_hash(packs);
    if !g.provenance().any(|(n, _, h)| n == DETECTION_STAMP && h == current) {
        return true;
    }
    let Ok(built) = index.metadata().and_then(|m| m.modified()) else { return true };
    let excludes = crate::pipeline::excludes(cfg);
    newest_input(project_dir, &excludes).is_some_and(|t| t > built)
}
