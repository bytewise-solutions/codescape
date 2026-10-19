fn main() {
    println!("cargo:rerun-if-changed=../../packaging/icon/codescape.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/icon/codescape.ico");
        res.compile().expect("embedding the Windows icon");
    }
}
