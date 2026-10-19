use crate::review::{git, is_git};
use std::collections::HashMap;
use std::path::Path;

pub fn file_changes(dir: &Path, since_days: u32) -> anyhow::Result<HashMap<String, u32>> {
    let mut counts = HashMap::new();
    if !is_git(dir) {
        return Ok(counts);
    }
    let since = format!("--since={since_days}.days");
    let log = git(dir, &["log", &since, "--name-only", "--relative", "-z", "--format="])?;
    for path in log.split('\0').filter(|p| !p.is_empty()) {
        *counts.entry(path.to_string()).or_insert(0) += 1;
    }
    Ok(counts)
}
