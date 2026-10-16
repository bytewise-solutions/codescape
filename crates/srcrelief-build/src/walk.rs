use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::Path;

pub fn default_excludes() -> GlobSet {
    let mut b = GlobSetBuilder::new();
    for g in ["**/build/**", "**/.gradle/**", "**/node_modules/**", "**/vendor/**", "**/.git/**", "**/target/**", "**/dist/**", "**/.*/**"] {
        b.add(Glob::new(g).unwrap());
    }
    b.build().unwrap()
}

pub fn rel_path(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn excluded_dir(root: &Path, dir: &Path, excludes: &GlobSet) -> bool {
    let rel = rel_path(root, dir);
    !rel.is_empty() && excludes.is_match(format!("{rel}/x"))
}

fn walker(root: &Path) -> ignore::WalkBuilder {
    let mut w = ignore::WalkBuilder::new(root);
    w.hidden(true).git_ignore(true).git_global(false).git_exclude(false).require_git(false).parents(false);
    w
}

pub fn walk_dirs(root: &Path, excludes: &GlobSet, mut visit: impl FnMut(&Path, &[String])) -> Result<()> {
    let r = root.to_path_buf();
    let ex = excludes.clone();
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    for entry in walker(root).filter_entry(move |e| !(e.file_type().is_some_and(|t| t.is_dir()) && excluded_dir(&r, e.path(), &ex))).build() {
        let entry = entry?;
        if entry.file_type().is_some_and(|t| t.is_dir()) {
            dirs.push(entry.into_path());
        }
    }
    for dir in dirs {
        let names: Vec<String> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        visit(&dir, &names);
    }
    Ok(())
}

pub fn walk_files(root: &Path, excludes: &GlobSet, mut visit: impl FnMut(&Path)) -> Result<()> {
    let r = root.to_path_buf();
    let ex = excludes.clone();
    for entry in walker(root).filter_entry(move |e| !(e.file_type().is_some_and(|t| t.is_dir()) && excluded_dir(&r, e.path(), &ex))).build() {
        let entry = entry?;
        if entry.file_type().is_some_and(|t| t.is_file()) {
            visit(entry.path());
        }
    }
    Ok(())
}
