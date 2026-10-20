use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const ICON: &[u8] = include_bytes!("../../../packaging/icon/codescape.svg");
const ENTRY: &str = "applications/codescape.desktop";

pub fn data_dirs_from(var: Option<&OsStr>) -> Vec<PathBuf> {
    let dirs: Vec<PathBuf> = var.map(std::env::split_paths).into_iter().flatten().filter(|p| p.is_absolute()).collect();
    if dirs.is_empty() { vec![PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")] } else { dirs }
}

pub fn ensure_entry(data_home: &Path, data_dirs: &[PathBuf], appimage: &Path) -> std::io::Result<bool> {
    let entry = data_home.join(ENTRY);
    let icon = data_home.join("icons/hicolor/scalable/apps/codescape.svg");
    if entry.exists() || data_dirs.iter().any(|d| d.join(ENTRY).exists()) {
        return Ok(false);
    }
    std::fs::create_dir_all(entry.parent().unwrap_or(data_home))?;
    let text = format!(
        "[Desktop Entry]\nType=Application\nName=CodeScape\nExec={} %f\nIcon=codescape\nStartupWMClass=codescape\nNoDisplay=true\nTerminal=false\n",
        appimage.display()
    );
    std::fs::write(&entry, text)?;
    if !icon.exists() {
        std::fs::create_dir_all(icon.parent().unwrap_or(data_home))?;
        std::fs::write(&icon, ICON)?;
    }
    Ok(true)
}
