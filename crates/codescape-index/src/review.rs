use codescape_format::{ArchivedGraph, NodeFlags, NodeId, NodeKind, SCOPE_MAIN, Visibility};
use similar::{ChangeTag, DiffOp, TextDiff};
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
    pub fingerprint: u64,
    pub signature: bool,
    pub widened: Option<(Visibility, Visibility)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileDiff {
    pub added: Vec<u32>,
    pub removed: Vec<(u32, Vec<String>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Removed {
    pub path: String,
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    pub flags: NodeFlags,
}

#[derive(Clone, Debug, Default)]
pub struct Review {
    changes: Vec<Change>,
    pub removed: Vec<Removed>,
    pub new_arrows: Vec<(NodeId, NodeId)>,
    pub removed_arrows: Vec<(String, String)>,
    files: HashMap<String, FileDiff>,
}

impl Review {
    pub fn change(&self, n: NodeId) -> &Change {
        &self.changes[n as usize]
    }

    pub fn file(&self, path: &str) -> Option<&FileDiff> {
        self.files.get(path)
    }

    pub fn totals(&self) -> Counts {
        self.changes.first().map_or(Counts::default(), |c| c.inside)
    }
}

fn compared(g: &ArchivedGraph, n: NodeId) -> bool {
    matches!(g.kind(n), NodeKind::Module | NodeKind::Package | NodeKind::Type | NodeKind::Member)
        && g.path(n).get(1).is_none_or(|&m| g.name(m) != EXTERNAL_MODULE)
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
    fn file(&mut self, rel: &str) -> &Vec<String> {
        let path = self.root.join(rel);
        self.files.entry(path.clone()).or_insert_with(|| std::fs::read_to_string(&path).map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default())
    }

    fn lines(&mut self, n: NodeId) -> Vec<String> {
        let (start, end) = self.g.span(n);
        let Some(file) = self.g.file(n).filter(|_| start >= 1 && end >= start) else { return Vec::new() };
        let all = self.file(file);
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

fn fingerprint(lines: &[String]) -> u64 {
    lines.iter().flat_map(|l| l.bytes().chain(*b"\n")).fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

fn openness(v: Visibility) -> Option<u8> {
    match v {
        Visibility::Unknown => None,
        Visibility::Private => Some(0),
        Visibility::Package => Some(1),
        Visibility::Protected => Some(2),
        Visibility::Public => Some(3),
    }
}

fn file_diff(before: &[String], after: &[String]) -> Option<FileDiff> {
    let old: Vec<&str> = before.iter().map(|l| l.trim()).collect();
    let new: Vec<&str> = after.iter().map(|l| l.trim()).collect();
    let mut d = FileDiff::default();
    for op in similar::capture_diff_slices(similar::Algorithm::Myers, &old, &new) {
        match op {
            DiffOp::Insert { new_index, new_len, .. } => d.added.extend((new_index..new_index + new_len).map(|i| i as u32 + 1)),
            DiffOp::Delete { old_index, old_len, new_index } => d.removed.push((new_index as u32 + 1, before[old_index..old_index + old_len].to_vec())),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                d.added.extend((new_index..new_index + new_len).map(|i| i as u32 + 1));
                d.removed.push((new_index as u32 + 1, before[old_index..old_index + old_len].to_vec()));
            }
            DiffOp::Equal { .. } => {}
        }
    }
    (d != FileDiff::default()).then_some(d)
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
    let unchanged = Change {
        status: Status::Unchanged,
        added: 0,
        removed: 0,
        complexity: None,
        inside: Counts::default(),
        fingerprint: 0,
        signature: false,
        widened: None,
    };
    let mut changes = vec![unchanged.clone(); target.node_count()];
    for n in (0..target.node_count() as NodeId).filter(|&n| compared(target, n)) {
        let c = &mut changes[n as usize];
        match base_keys.get(&key(target, n)) {
            None => {
                let lines = after.lines(n);
                c.status = Status::New;
                c.added = lines.len() as u32;
                c.fingerprint = fingerprint(&lines);
            }
            Some(&b) if matches!(target.kind(n), NodeKind::Type | NodeKind::Member) => {
                let (old, new) = (before.lines(b), after.lines(n));
                c.fingerprint = fingerprint(&new);
                if old != new {
                    let (add, del) = line_delta(&old, &new);
                    *c = Change { status: Status::Changed, added: add, removed: del, signature: old.first() != new.first(), ..c.clone() };
                }
                if let (Some(x), Some(y)) = (openness(base.visibility(b)), openness(target.visibility(n)))
                    && y > x
                {
                    c.widened = Some((base.visibility(b), target.visibility(n)));
                }
                if let (Some(x), Some(y)) = (base.complexity(b), target.complexity(n))
                    && x != y
                {
                    c.complexity = Some((x, y));
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
        removed.push(Removed { path: k.clone(), kind: base.kind(b), parent: parent_key.and_then(|pk| target_keys.get(&pk).copied()), flags: base.flags(b) });
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
    let mut added: Vec<(NodeId, NodeId)> = new_arrows.difference(&old_arrows).filter_map(|(a, b)| Some((*target_keys.get(a)?, *target_keys.get(b)?))).collect();
    added.sort();
    let mut gone: Vec<(String, String)> = old_arrows.difference(&new_arrows).cloned().collect();
    gone.sort();
    let mut files = HashMap::new();
    let paths: HashSet<&str> = (0..target.node_count() as NodeId).filter(|&n| compared(target, n)).filter_map(|n| target.file(n)).collect();
    for path in paths {
        let new = after.file(path).clone();
        if let Some(d) = file_diff(before.file(path), &new) {
            files.insert(path.to_string(), d);
        }
    }
    Review { changes, removed, new_arrows: added, removed_arrows: gone, files }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RevisionIndex {
    pub dir: PathBuf,
    pub index: PathBuf,
    pub sha: String,
    pub version: PathBuf,
}

const USED_MARK: &str = ".codescape-used";

fn export(root: &Path, sha: &str, rel: &Path, dest: &Path) -> anyhow::Result<()> {
    let partial = dest.with_extension(format!("partial{}", crate::unique_suffix()));
    std::fs::create_dir_all(&partial)?;
    let mut args = vec!["archive".to_string(), "--format=tar".into(), sha.into()];
    if !rel.as_os_str().is_empty() {
        args.extend(["--".into(), rel.to_string_lossy().into_owned()]);
    }
    let mut archive = std::process::Command::new("git").args(&args).current_dir(root).stdout(std::process::Stdio::piped()).spawn()?;
    let tar = std::process::Command::new("tar").arg("-x").arg("-C").arg(&partial).stdin(archive.stdout.take().expect("piped")).output()?;
    let archived = archive.wait()?;
    if !archived.success() || !tar.status.success() {
        anyhow::bail!("exporting {sha}: {}", String::from_utf8_lossy(&tar.stderr).trim());
    }
    if let Err(e) = std::fs::rename(&partial, dest) {
        std::fs::remove_dir_all(&partial)?;
        if !dest.exists() {
            return Err(e.into());
        }
    }
    Ok(())
}

fn remove_stale_partials(dest: &Path) {
    let (Some(dir), Some(name)) = (dest.parent(), dest.with_extension("partial").file_name().map(|n| n.to_string_lossy().into_owned())) else { return };
    let stale =
        |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().unwrap_or_default() > std::time::Duration::from_secs(3600));
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().strip_prefix(&name).is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b == b'-' || b.is_ascii_digit()))
            && stale(&e.path())
        {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

pub fn mark_used_at(r: &RevisionIndex, at: std::time::SystemTime) -> std::io::Result<()> {
    std::fs::File::create(r.version.join(USED_MARK))?.set_modified(at)
}

fn is_version_key(name: &str) -> bool {
    name.split_once('-').is_some_and(|(p, s)| p.len() == 16 && s.len() == 12 && name.bytes().all(|b| b == b'-' || b.is_ascii_hexdigit()))
}

pub fn clean_versions(project_dir: &Path, cache_root: &Path, unused_for: std::time::Duration) -> anyhow::Result<usize> {
    let root = PathBuf::from(git(project_dir, &["rev-parse", "--show-toplevel"])?);
    let prefix = format!("{}-", crate::project_key(&project_dir.canonicalize()?));
    let ours = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix)))
            .collect()
    };
    let mut removed = 0;
    for old in ours(&cache_root.join("worktrees")) {
        let _ = git(&root, &["worktree", "remove", "--force", &old.to_string_lossy()]);
        if old.exists() {
            std::fs::remove_dir_all(&old)?;
        }
        removed += 1;
    }
    let _ = git(&root, &["worktree", "prune"]);
    let now = std::time::SystemTime::now();
    for version in ours(&cache_root.join("exports")) {
        let used = std::fs::metadata(version.join(USED_MARK)).or_else(|_| std::fs::metadata(&version)).and_then(|m| m.modified()).unwrap_or(now);
        if now.duration_since(used).unwrap_or_default() < unused_for {
            continue;
        }
        std::fs::remove_dir_all(&version)?;
        removed += 1;
    }
    let legacy = cache_root.join("versions");
    for old in std::fs::read_dir(&legacy).into_iter().flatten().flatten().map(|e| e.path()) {
        if old.is_dir() {
            std::fs::remove_dir_all(&old)?
        } else {
            std::fs::remove_file(&old)?
        }
        if let Some(name) = old.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| is_version_key(n)) {
            let _ = std::fs::remove_file(cache_root.join(format!("{name}.codescape")));
        }
        removed += 1;
    }
    let _ = std::fs::remove_dir(&legacy);
    Ok(removed)
}

pub(crate) fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().map_err(|e| anyhow::anyhow!("running git: {e}"))?;
    if !out.status.success() {
        anyhow::bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
}

pub fn is_git(dir: &Path) -> bool {
    git(dir, &["rev-parse", "--show-toplevel"]).is_ok()
}

pub fn current_branch(dir: &Path) -> Option<String> {
    git(dir, &["symbolic-ref", "--short", "-q", "HEAD"]).ok().filter(|b| !b.is_empty())
}

pub fn branches(dir: &Path) -> Vec<String> {
    let list = |refs: &str| git(dir, &["for-each-ref", "--format=%(refname:short)", refs]).unwrap_or_default();
    let (local, remote) = (list("refs/heads"), list("refs/remotes"));
    let current = current_branch(dir);
    let mut out: Vec<String> = current.clone().into_iter().collect();
    out.extend(local.lines().filter(|b| Some(*b) != current.as_deref()).map(str::to_string));
    out.extend(remote.lines().filter(|b| b.contains('/') && !b.ends_with("/HEAD")).map(str::to_string));
    out
}

pub fn commits(dir: &Path, base: &str, target: &str) -> anyhow::Result<Vec<Commit>> {
    let listed = git(dir, &["log", "--reverse", "--format=%H%x09%s", &format!("{base}..{target}")])?;
    Ok(listed.lines().filter_map(|l| l.split_once('\t')).map(|(sha, subject)| Commit { sha: sha.into(), subject: subject.into() }).collect())
}

pub fn has_uncommitted(dir: &Path) -> bool {
    git(dir, &["status", "--porcelain", "--", "."]).is_ok_and(|s| !s.is_empty())
}

pub fn base_for(project_dir: &Path, target: Option<&str>) -> anyhow::Result<String> {
    let target = target.unwrap_or("HEAD");
    for main in ["main", "master", "origin/main", "origin/master"] {
        if let Ok(sha) = git(project_dir, &["merge-base", target, main]) {
            return Ok(sha);
        }
    }
    git(project_dir, &["rev-parse", target])
}

pub fn index_revision(
    project_dir: &Path,
    rev: &str,
    cfg: &crate::ProjectConfig,
    cache_root: &Path,
    progress: &dyn Fn(crate::Progress),
) -> anyhow::Result<RevisionIndex> {
    let sha = git(project_dir, &["rev-parse", &format!("{rev}^{{commit}}")])?;
    let root = PathBuf::from(git(project_dir, &["rev-parse", "--show-toplevel"])?).canonicalize()?;
    let project = project_dir.canonicalize()?;
    let rel = project.strip_prefix(&root)?.to_path_buf();
    let key = format!("{}-{}", crate::project_key(&project), &sha[..12]);
    let version = cache_root.join("exports").join(&key);
    let checkout = version.join(root.file_name().unwrap_or_else(|| "repo".as_ref()));
    if !checkout.exists() {
        std::fs::create_dir_all(&version)?;
        remove_stale_partials(&checkout);
        export(&root, &sha, &rel, &checkout)?;
    }
    let dir = checkout.join(rel);
    let index = version.join("index.codescape");
    let cfg = crate::ProjectConfig { index: Some(index.clone()), ..cfg.clone() };
    let packs = crate::project_packs(&dir, &cfg)?;
    if crate::needs_reindex(&dir, &index, &cfg, &packs) {
        crate::index_project(&dir, &cfg, progress)?;
    }
    let r = RevisionIndex { dir, index, sha, version };
    mark_used_at(&r, std::time::SystemTime::now())?;
    Ok(r)
}
