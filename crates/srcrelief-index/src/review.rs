use similar::{ChangeTag, TextDiff};
use srcrelief_format::{ArchivedGraph, NodeId, NodeKind, SCOPE_MAIN};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const EXTERNAL_MODULE: &str = "(external)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    New,
    Changed,
    Unchanged,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub new: u32,
    pub changed: u32,
    pub removed: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub status: Status,
    pub added: u32,
    pub removed: u32,
    pub complexity: Option<(u16, u16)>,
    pub inside: Counts,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Removed {
    pub path: String,
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
}

#[derive(Clone, Debug, Default)]
pub struct Review {
    changes: Vec<Change>,
    pub removed: Vec<Removed>,
    pub new_arrows: Vec<(NodeId, NodeId)>,
    pub removed_arrows: Vec<(String, String)>,
}

impl Review {
    pub fn change(&self, n: NodeId) -> &Change {
        &self.changes[n as usize]
    }

    pub fn totals(&self) -> Counts {
        self.changes.first().map_or(Counts::default(), |c| c.inside)
    }
}

fn compared(g: &ArchivedGraph, n: NodeId) -> bool {
    matches!(g.kind(n), NodeKind::Module | NodeKind::Package | NodeKind::Type | NodeKind::Member) && g.path(n).get(1).is_none_or(|&m| g.name(m) != EXTERNAL_MODULE)
}

fn key(g: &ArchivedGraph, n: NodeId) -> String {
    g.path(n).iter().skip(1).map(|&p| g.name(p)).collect::<Vec<_>>().join("/")
}

struct Source<'a> {
    g: &'a ArchivedGraph,
    root: &'a Path,
    files: HashMap<PathBuf, Vec<String>>,
}

impl Source<'_> {
    fn lines(&mut self, n: NodeId) -> Vec<String> {
        let (start, end) = self.g.span(n);
        let Some(file) = self.g.file(n).filter(|_| start >= 1 && end >= start) else { return Vec::new() };
        let path = self.root.join(file);
        let all = self.files.entry(path.clone()).or_insert_with(|| std::fs::read_to_string(&path).map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default());
        let end = (end as usize).min(all.len());
        all[(start as usize - 1).min(end)..end].iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
    }
}

fn line_delta(before: &[String], after: &[String]) -> (u32, u32) {
    let (a, b) = (before.join("\n") + "\n", after.join("\n") + "\n");
    let diff = TextDiff::from_lines(&a, &b);
    diff.iter_all_changes().fold((0, 0), |(add, del), c| match c.tag() {
        ChangeTag::Insert => (add + 1, del),
        ChangeTag::Delete => (add, del + 1),
        ChangeTag::Equal => (add, del),
    })
}

fn arrows(g: &ArchivedGraph) -> HashSet<(String, String)> {
    let mut out = HashSet::new();
    for n in (0..g.node_count() as NodeId).filter(|&n| n == g.root() || compared(g, n)) {
        for a in g.aggregates(n) {
            if a.calls.to_native() + a.deps.to_native() > 0 && a.scope_mask & SCOPE_MAIN != 0 {
                let (x, y) = (a.a.to_native(), a.b.to_native());
                if compared(g, x) && compared(g, y) {
                    out.insert((key(g, x), key(g, y)));
                }
            }
        }
    }
    out
}

pub fn compare(base: &ArchivedGraph, base_root: &Path, target: &ArchivedGraph, target_root: &Path) -> Review {
    let base_keys: HashMap<String, NodeId> = (0..base.node_count() as NodeId).filter(|&n| compared(base, n)).map(|n| (key(base, n), n)).collect();
    let target_keys: HashMap<String, NodeId> = (0..target.node_count() as NodeId).filter(|&n| compared(target, n)).map(|n| (key(target, n), n)).collect();
    let mut before = Source { g: base, root: base_root, files: HashMap::new() };
    let mut after = Source { g: target, root: target_root, files: HashMap::new() };
    let unchanged = Change { status: Status::Unchanged, added: 0, removed: 0, complexity: None, inside: Counts::default() };
    let mut changes = vec![unchanged.clone(); target.node_count()];
    for n in (0..target.node_count() as NodeId).filter(|&n| compared(target, n)) {
        let c = &mut changes[n as usize];
        match base_keys.get(&key(target, n)) {
            None => {
                c.status = Status::New;
                c.added = after.lines(n).len() as u32;
            }
            Some(&b) if matches!(target.kind(n), NodeKind::Type | NodeKind::Member) => {
                let (old, new) = (before.lines(b), after.lines(n));
                if old != new {
                    let (add, del) = line_delta(&old, &new);
                    *c = Change { status: Status::Changed, added: add, removed: del, ..unchanged.clone() };
                }
                if let (Some(x), Some(y)) = (base.complexity(b), target.complexity(n)) {
                    if x != y {
                        c.complexity = Some((x, y));
                    }
                }
            }
            Some(_) => {}
        }
    }
    let mut removed: Vec<Removed> = Vec::new();
    for (k, &b) in &base_keys {
        if target_keys.contains_key(k) {
            continue;
        }
        let parent_key = base.parent(b).filter(|&p| compared(base, p)).map(|p| key(base, p));
        if parent_key.as_ref().is_some_and(|pk| !target_keys.contains_key(pk)) {
            continue;
        }
        removed.push(Removed { path: k.clone(), kind: base.kind(b), parent: parent_key.and_then(|pk| target_keys.get(&pk).copied()) });
    }
    removed.sort_by(|a, b| a.path.cmp(&b.path));
    let mut order: Vec<NodeId> = (0..target.node_count() as NodeId).collect();
    order.sort_by_key(|&n| std::cmp::Reverse(target.path(n).len()));
    for &n in &order {
        let Some(p) = target.parent(n) else { continue };
        let child = changes[n as usize].clone();
        let parent = &mut changes[p as usize];
        parent.inside.new += child.inside.new + u32::from(child.status == Status::New);
        parent.inside.changed += child.inside.changed + u32::from(child.status == Status::Changed);
        parent.inside.removed += child.inside.removed;
        if !matches!(target.kind(p), NodeKind::Type) {
            parent.added += child.added * u32::from(child.status != Status::Unchanged);
            parent.removed += child.removed;
        }
        if parent.status == Status::Unchanged && (child.status != Status::Unchanged || child.inside != Counts::default()) {
            parent.status = Status::Changed;
        }
    }
    for r in &removed {
        let mut cur = r.parent;
        while let Some(p) = cur {
            changes[p as usize].inside.removed += 1;
            if changes[p as usize].status == Status::Unchanged {
                changes[p as usize].status = Status::Changed;
            }
            cur = target.parent(p);
        }
    }
    let (old_arrows, new_arrows) = (arrows(base), arrows(target));
    let mut added: Vec<(NodeId, NodeId)> =
        new_arrows.difference(&old_arrows).filter_map(|(a, b)| Some((*target_keys.get(a)?, *target_keys.get(b)?))).collect();
    added.sort();
    let mut gone: Vec<(String, String)> = old_arrows.difference(&new_arrows).cloned().collect();
    gone.sort();
    Review { changes, removed, new_arrows: added, removed_arrows: gone }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RevisionIndex {
    pub dir: PathBuf,
    pub index: PathBuf,
    pub sha: String,
}

fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().map_err(|e| anyhow::anyhow!("running git: {e}"))?;
    if !out.status.success() {
        anyhow::bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn default_base(project_dir: &Path) -> anyhow::Result<String> {
    for main in ["main", "master", "origin/main", "origin/master"] {
        if let Ok(sha) = git(project_dir, &["merge-base", "HEAD", main]) {
            return Ok(sha);
        }
    }
    git(project_dir, &["rev-parse", "HEAD"])
}

pub fn index_revision(project_dir: &Path, rev: &str, cfg: &crate::ProjectConfig, cache_root: &Path, progress: &dyn Fn(crate::Progress)) -> anyhow::Result<RevisionIndex> {
    let sha = git(project_dir, &["rev-parse", &format!("{rev}^{{commit}}")])?;
    let root = PathBuf::from(git(project_dir, &["rev-parse", "--show-toplevel"])?).canonicalize()?;
    let project = project_dir.canonicalize()?;
    let rel = project.strip_prefix(&root)?.to_path_buf();
    let key = format!("{}-{}", crate::project_key(&project), &sha[..12]);
    let worktree = cache_root.join("worktrees").join(&key);
    if !worktree.join(".git").exists() {
        std::fs::create_dir_all(cache_root.join("worktrees"))?;
        git(&root, &["worktree", "add", "--detach", &worktree.to_string_lossy(), &sha])?;
    }
    let dir = worktree.join(rel);
    let index = cache_root.join(format!("{key}.relief"));
    let cfg = crate::ProjectConfig { index: Some(index.clone()), ..cfg.clone() };
    let packs = crate::project_packs(&dir, &cfg)?;
    if crate::needs_reindex(&dir, &index, &cfg, &packs) {
        crate::index_project(&dir, &cfg, progress)?;
    }
    Ok(RevisionIndex { dir, index, sha })
}
