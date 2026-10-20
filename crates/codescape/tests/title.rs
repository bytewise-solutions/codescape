use codescape::app::window_title;
use std::path::Path;

#[test]
fn the_window_title_names_the_version_and_the_open_project() {
    let v = env!("CARGO_PKG_VERSION");
    assert_eq!(window_title(None), format!("CodeScape {v}"));
    assert_eq!(window_title(Some(Path::new("/home/me/work/shop"))), format!("CodeScape {v} — shop"));
}
