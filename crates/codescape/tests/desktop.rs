use codescape::desktop::ensure_entry;
use std::path::Path;

#[test]
fn an_appimage_leaves_a_hidden_entry_and_icon_so_the_window_gets_its_icon() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path();
    assert!(ensure_entry(data, Path::new("/home/me/Applications/CodeScape.AppImage")).unwrap());
    let entry = std::fs::read_to_string(data.join("applications/codescape.desktop")).unwrap();
    assert!(entry.contains("NoDisplay=true") && entry.contains("Icon=codescape"), "{entry}");
    assert!(entry.contains("Exec=/home/me/Applications/CodeScape.AppImage %f"), "{entry}");
    assert!(data.join("icons/hicolor/scalable/apps/codescape.svg").exists());
    assert!(!ensure_entry(data, Path::new("/elsewhere/CodeScape.AppImage")).unwrap(), "written once");
}

#[test]
fn an_installed_entry_is_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let apps = tmp.path().join("applications");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("codescape.desktop"), "[Desktop Entry]\nName=Mine\n").unwrap();
    ensure_entry(tmp.path(), Path::new("/a/CodeScape.AppImage")).unwrap();
    assert_eq!(std::fs::read_to_string(apps.join("codescape.desktop")).unwrap(), "[Desktop Entry]\nName=Mine\n");
}
