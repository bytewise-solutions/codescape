fn main() {
    let dir = std::env::args().nth(1).expect("usage: discover <dir>");
    let d = codescape_build::discover(std::path::Path::new(&dir), &codescape_build::default_excludes()).unwrap();
    for m in &d.modules {
        println!("{} {:?} main={:?} test={:?} dev={:?}", m.name, m.lang, m.source_roots, m.test_roots, m.dev_roots);
    }
    for l in &d.links {
        println!("{} -> {} {:?}", d.modules[l.from].name, d.modules[l.to].name, l.scope);
    }
    println!("skipped: {:?}", d.skipped);
}
