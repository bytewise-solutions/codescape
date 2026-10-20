#[test]
fn a_screenshot_without_a_project_folder_is_refused_at_once() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_codescape"))
        .args(["--screenshot", "x.png"])
        .env("DISPLAY", "")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("<DIR>"), "{}", String::from_utf8_lossy(&out.stderr));
}
