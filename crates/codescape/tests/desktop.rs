use codescape::desktop::{data_dirs_from, ensure_entry};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

fn entry_in(data: &Path) -> String {
    std::fs::read_to_string(data.join("applications/codescape.desktop")).unwrap()
}

#[test]
fn an_appimage_leaves_a_hidden_entry_and_icon_so_the_window_gets_its_icon() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path();
    assert!(ensure_entry(data, &[], Path::new("/home/me/Applications/CodeScape.AppImage")).unwrap());
    let entry = entry_in(data);
    assert!(entry.contains("NoDisplay=true") && entry.contains("Icon=codescape"), "{entry}");
    assert!(entry.contains("Exec=/home/me/Applications/CodeScape.AppImage %f"), "{entry}");
    assert!(data.join("icons/hicolor/scalable/apps/codescape.svg").exists());
    assert!(!ensure_entry(data, &[], Path::new("/home/me/Applications/CodeScape.AppImage")).unwrap(), "written once");
}

#[test]
fn an_installed_entry_is_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let apps = tmp.path().join("applications");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("codescape.desktop"), "[Desktop Entry]\nName=Mine\nNoDisplay=true\n").unwrap();
    assert!(!ensure_entry(tmp.path(), &[], Path::new("/a/CodeScape.AppImage")).unwrap());
    assert_eq!(std::fs::read_to_string(apps.join("codescape.desktop")).unwrap(), "[Desktop Entry]\nName=Mine\nNoDisplay=true\n");
}

#[test]
fn an_entry_installed_in_a_system_data_dir_is_not_shadowed() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, usr, local) = (tmp.path().join("home"), tmp.path().join("usr/share"), tmp.path().join("usr/local/share"));
    std::fs::create_dir_all(local.join("applications")).unwrap();
    std::fs::write(local.join("applications/codescape.desktop"), "[Desktop Entry]\nName=CodeScape\n").unwrap();
    assert!(!ensure_entry(&home, &[usr.clone(), local.clone()], Path::new("/a/CodeScape.AppImage")).unwrap());
    assert!(!home.join("applications/codescape.desktop").exists());
    ensure_entry(&home, &[usr], Path::new("/a/CodeScape.AppImage")).unwrap();
    assert!(home.join("applications/codescape.desktop").exists());
}

#[test]
fn the_system_data_dirs_default_to_usr_local_share_and_usr_share() {
    let default = [PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")];
    assert_eq!(data_dirs_from(None), default);
    assert_eq!(data_dirs_from(Some(OsStr::new(""))), default);
    assert_eq!(data_dirs_from(Some(OsStr::new("/opt/share:relative:/usr/share"))), [PathBuf::from("/opt/share"), PathBuf::from("/usr/share")]);
}
