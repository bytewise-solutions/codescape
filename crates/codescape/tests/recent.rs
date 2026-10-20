use codescape::app::{parse_recent, shown_recent};

#[test]
fn a_recent_folder_that_is_missing_is_kept_but_not_listed() {
    let tmp = tempfile::tempdir().unwrap();
    let (here, gone) = (tmp.path().join("here"), tmp.path().join("unmounted/share"));
    std::fs::create_dir_all(&here).unwrap();
    let text = format!("recent = [{:?}, {:?}]\n", gone.display().to_string(), here.display().to_string());
    let list = parse_recent(&text);
    assert_eq!(list, [gone.clone(), here.clone()], "kept for when the disk is back");
    assert_eq!(shown_recent(&list).collect::<Vec<_>>(), [&here]);
}
