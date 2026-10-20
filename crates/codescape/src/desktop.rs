use std::path::Path;

const ICON: &[u8] = include_bytes!("../../../packaging/icon/codescape.svg");

pub fn ensure_entry(data_dir: &Path, appimage: &Path) -> std::io::Result<bool> {
    let entry = data_dir.join("applications/codescape.desktop");
    let icon = data_dir.join("icons/hicolor/scalable/apps/codescape.svg");
    let mut wrote = false;
    if !entry.exists() {
        std::fs::create_dir_all(entry.parent().unwrap_or(data_dir))?;
        let text = format!(
            "[Desktop Entry]\nType=Application\nName=CodeScape\nExec={} %f\nIcon=codescape\nStartupWMClass=codescape\nNoDisplay=true\nTerminal=false\n",
            appimage.display()
        );
        std::fs::write(&entry, text)?;
        wrote = true;
    }
    if !icon.exists() {
        std::fs::create_dir_all(icon.parent().unwrap_or(data_dir))?;
        std::fs::write(&icon, ICON)?;
        wrote = true;
    }
    Ok(wrote)
}
