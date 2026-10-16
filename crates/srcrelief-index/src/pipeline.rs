use crate::cache::{DETECTION_STAMP, index_path};
use crate::config::ProjectConfig;
use crate::summary::Summary;
use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use srcrelief_build::{default_excludes, discover};
use srcrelief_facts::{ExtractStats, ProjectFacts};
use srcrelief_format::*;
use srcrelief_rules::{Packs, apply_detection, detection_hash, load_packs, user_rules_dir};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Discovering,
    Parsing,
    Rules,
    Writing,
}

pub struct IndexOutcome {
    pub path: PathBuf,
    pub summary: Summary,
}

pub fn excludes(cfg: &ProjectConfig) -> GlobSet {
    let mut b = GlobSetBuilder::new();
    for g in ["**/build/**", "**/.gradle/**", "**/node_modules/**", "**/vendor/**", "**/.git/**", "**/target/**", "**/dist/**", "**/.*/**"] {
        b.add(Glob::new(g).unwrap());
    }
    for g in &cfg.exclude {
        if let Ok(glob) = Glob::new(g) {
            b.add(glob);
        }
    }
    b.build().unwrap_or_else(|_| default_excludes())
}

pub fn project_packs(project_dir: &Path, cfg: &ProjectConfig) -> Result<Packs> {
    let base = load_packs(Some(project_dir), user_rules_dir().as_deref(), &cfg.packs)?;
    let mut packs: Vec<_> = base.packs().to_vec();
    if let Some(p) = cfg.config_pack() {
        packs.push(p);
    }
    Ok(Packs::from_packs(packs, &cfg.packs))
}

pub fn index_project(dir: &Path, cfg: &ProjectConfig, progress: &dyn Fn(Progress)) -> Result<IndexOutcome> {
    let start = Instant::now();
    let dir = dir.canonicalize().with_context(|| format!("opening {}", dir.display()))?;
    let excludes = excludes(cfg);
    progress(Progress::Discovering);
    let d = discover(&dir, &excludes)?;
    let name = cfg.name.clone().unwrap_or_else(|| dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default());
    let mut b = GraphBuilder::new(&name);
    let module_nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    progress(Progress::Parsing);
    let mut facts = ProjectFacts::default();
    let mut stats = ExtractStats::default();
    stats.add(srcrelief_java::extract_java(&d, &module_nodes, &excludes, &mut b, &mut facts));
    stats.add(srcrelief_go::extract_go(&d, &module_nodes, &excludes, &mut b, &mut facts));
    progress(Progress::Rules);
    let packs = project_packs(&dir, cfg)?;
    let root = d.root.clone();
    apply_detection(&packs, &facts, &move |rel: &str| std::fs::read(root.join(rel)).ok(), &mut b);
    for p in packs.packs() {
        b.add_provenance(&p.name, &p.version, &p.hash);
    }
    b.add_provenance(DETECTION_STAMP, "", &detection_hash(&packs));
    b.set_stats(Stats {
        files: stats.files,
        lines: stats.lines,
        source_bytes: stats.bytes,
        calls_resolved: stats.resolved,
        calls_guessed: stats.guessed,
        calls_unresolved: stats.unresolved,
        skipped_roots: d.skipped.iter().map(|s| (s.kind.clone(), s.rel.clone())).collect(),
    });
    progress(Progress::Writing);
    let graph = b.finish();
    let path = index_path(&dir, cfg);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("relief.tmp{}", std::process::id()));
    write_graph(&graph, &tmp)?;
    std::fs::rename(&tmp, &path)?;
    let file = ReliefFile::open(&path)?;
    let summary = Summary::from_graph(file.graph(), &packs.visual(), file.byte_len() as u64, start.elapsed());
    Ok(IndexOutcome { path, summary })
}
