use codescape_index::churn::file_changes;
use codescape_index::review::is_git;
use std::path::Path;

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

fn commit(dir: &Path, file: &str, body: &str) {
    let p = dir.join(file);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "change"]);
}

#[test]
fn counts_commits_touching_each_file() {
    let tmp = tempfile::tempdir().unwrap();
    git(tmp.path(), &["init", "-q"]);
    commit(tmp.path(), "a.java", "1");
    commit(tmp.path(), "b.java", "1");
    commit(tmp.path(), "a.java", "2");
    let changes = file_changes(tmp.path(), 365).unwrap();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes["a.java"], 2);
    assert_eq!(changes["b.java"], 1);
}

#[test]
fn a_folder_that_is_not_a_repository_has_no_changes() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!is_git(tmp.path()));
    assert!(file_changes(tmp.path(), 365).unwrap().is_empty());
}

#[test]
fn paths_are_relative_to_a_project_folder_inside_the_repository() {
    let tmp = tempfile::tempdir().unwrap();
    git(tmp.path(), &["init", "-q"]);
    commit(tmp.path(), "proj/src/a.java", "1");
    commit(tmp.path(), "proj/src/a.java", "2");
    commit(tmp.path(), "other/b.java", "1");
    let changes = file_changes(&tmp.path().join("proj"), 365).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes["src/a.java"], 2);
}
