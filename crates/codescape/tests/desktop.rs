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
    assert!(entry.contains("Exec=\"/home/me/Applications/CodeScape.AppImage\" %f"), "{entry}");
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
    ensure_entry(&home, std::slice::from_ref(&usr), Path::new("/a/CodeScape.AppImage")).unwrap();
    assert!(home.join("applications/codescape.desktop").exists());
    assert!(ensure_entry(&home, &[usr, local], Path::new("/a/CodeScape.AppImage")).unwrap());
    assert!(!home.join("applications/codescape.desktop").exists(), "ours goes once an installed one appears");
}

#[test]
fn the_system_data_dirs_default_to_usr_local_share_and_usr_share() {
    let default = [PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")];
    assert_eq!(data_dirs_from(None), default);
    assert_eq!(data_dirs_from(Some(OsStr::new(""))), default);
    assert_eq!(data_dirs_from(Some(OsStr::new("/opt/share:relative:/usr/share"))), [PathBuf::from("/opt/share"), PathBuf::from("/usr/share")]);
}

#[test]
fn the_entry_follows_the_appimage_when_it_moves_or_is_upgraded() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path();
    ensure_entry(data, &[], Path::new("/apps/CodeScape-0.1.0-x86_64.AppImage")).unwrap();
    std::fs::remove_file(data.join("icons/hicolor/scalable/apps/codescape.svg")).unwrap();
    assert!(ensure_entry(data, &[], Path::new("/apps/CodeScape-0.2.0-x86_64.AppImage")).unwrap());
    assert!(entry_in(data).contains("Exec=\"/apps/CodeScape-0.2.0-x86_64.AppImage\" %f"), "{}", entry_in(data));
    assert!(data.join("icons/hicolor/scalable/apps/codescape.svg").exists());
}

#[test]
fn the_exec_path_is_quoted_and_escaped() {
    let tmp = tempfile::tempdir().unwrap();
    ensure_entry(tmp.path(), &[], Path::new("/home/me/My Apps/CodeScape 100%.AppImage")).unwrap();
    assert!(entry_in(tmp.path()).contains("Exec=\"/home/me/My Apps/CodeScape 100%%.AppImage\" %f"), "{}", entry_in(tmp.path()));
    ensure_entry(tmp.path(), &[], Path::new("/x/a\"b`c$d\\e.AppImage")).unwrap();
    assert!(entry_in(tmp.path()).contains(r#"Exec="/x/a\\"b\\`c\\$d\\\\e.AppImage" %f"#), "{}", entry_in(tmp.path()));
}

#[test]
fn only_the_appimage_codescape_runs_from_is_trusted() {
    use codescape::desktop::running_appimage;
    let tmp = tempfile::tempdir().unwrap();
    let image = tmp.path().join("CodeScape.AppImage");
    std::fs::write(&image, "").unwrap();
    let mount = tmp.path().join(".mount_CodeSc");
    let exe = mount.join("usr/bin/codescape");
    assert_eq!(running_appimage(Some(image.as_os_str()), Some(mount.as_os_str()), Some(&exe)), Some(image.clone()));
    let other = tmp.path().join(".mount_IDE");
    assert_eq!(running_appimage(Some(image.as_os_str()), Some(other.as_os_str()), Some(&exe)), None, "inherited from another AppImage");
    assert_eq!(running_appimage(Some(image.as_os_str()), None, Some(&exe)), None);
    assert_eq!(running_appimage(Some(tmp.path().join("gone.AppImage").as_os_str()), Some(mount.as_os_str()), Some(&exe)), None);
    assert_eq!(running_appimage(None, Some(mount.as_os_str()), Some(&exe)), None);
}
