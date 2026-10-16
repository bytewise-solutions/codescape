use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for dir in ["srcrelief-format", "srcrelief-facts", "srcrelief-build", "srcrelief-java", "srcrelief-go", "srcrelief-rules", "srcrelief-index"] {
        println!("cargo:rerun-if-changed={}", crates.join(dir).join("src").display());
    }
    println!("cargo:rerun-if-changed={}", crates.join("../rules").display());
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    println!("cargo:rustc-env=SRCRELIEF_ENGINE_BUILD={now:x}");
}
