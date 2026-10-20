use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const ICON: &[u8] = include_bytes!("../../../packaging/icon/codescape.svg");
const ENTRY: &str = "applications/codescape.desktop";
const OURS: &str = "X-CodeScape-AppImage=true";

pub fn data_dirs_from(var: Option<&OsStr>) -> Vec<PathBuf> {
    let dirs: Vec<PathBuf> = var.map(std::env::split_paths).into_iter().flatten().filter(|p| p.is_absolute()).collect();
    if dirs.is_empty() { vec![PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")] } else { dirs }
}

fn exec_arg(path: &Path) -> String {
    let mut out = String::from("\"");
    for c in path.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' => out.push_str("\\\\"),
            '\\' => out.push_str("\\\\\\"),
            '%' => out.push('%'),
            _ => {}
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn entry_text(appimage: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=CodeScape\nExec={} %f\nIcon=codescape\nStartupWMClass=codescape\nNoDisplay=true\nTerminal=false\n{OURS}\n",
        exec_arg(appimage)
    )
}

pub fn ensure_entry(data_home: &Path, data_dirs: &[PathBuf], appimage: &Path) -> std::io::Result<bool> {
    let entry = data_home.join(ENTRY);
    let icon = data_home.join("icons/hicolor/scalable/apps/codescape.svg");
    let existing = std::fs::read_to_string(&entry).ok();
    if existing.as_ref().is_some_and(|t| !t.lines().any(|l| l.trim() == OURS)) {
        return Ok(false);
    }
    if data_dirs.iter().any(|d| d.join(ENTRY).exists()) {
        if existing.is_none() {
            return Ok(false);
        }
        std::fs::remove_file(&entry)?;
        return Ok(true);
    }
    let text = entry_text(appimage);
    if existing.as_deref() == Some(text.as_str()) && icon.exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(entry.parent().unwrap_or(data_home))?;
    std::fs::write(&entry, text)?;
    std::fs::create_dir_all(icon.parent().unwrap_or(data_home))?;
    std::fs::write(&icon, ICON)?;
    Ok(true)
}
