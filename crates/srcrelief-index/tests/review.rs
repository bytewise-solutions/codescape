use srcrelief_format::*;
use srcrelief_index::review::*;
use srcrelief_index::{ProjectConfig, index_project};
use std::path::Path;

struct Side {
    _tmp: tempfile::TempDir,
    dir: std::path::PathBuf,
    file: ReliefFile,
}

fn side(files: &[(&str, &str)]) -> Side {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("proj");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("go.mod"), "module example.com/shop\n\ngo 1.22\n").unwrap();
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    let cfg = ProjectConfig { index: Some(tmp.path().join("i.relief")), ..Default::default() };
    let out = index_project(&dir, &cfg, &|_| {}).unwrap();
    let file = ReliefFile::open(&out.path).unwrap();
    Side { _tmp: tmp, dir, file }
}

fn node(g: &ArchivedGraph, path: &[&str]) -> NodeId {
    let mut n = g.root();
    for p in path {
        n = g.children(n).find(|&c| g.name(c) == *p).unwrap_or_else(|| panic!("no {p}"));
    }
    n
}

const BASE_CART: &str = "package cart\n\ntype Cart struct{}\n\nfunc (c *Cart) Add(x int) int {\n\treturn x\n}\n\nfunc (c *Cart) Total() int {\n\treturn 0\n}\n\nfunc (c *Cart) Clear() {}\n";
const TARGET_CART: &str = "package cart\n\nimport \"example.com/shop/pay\"\n\ntype Cart struct{}\n\nfunc (c *Cart) Add(x int) int {\n\tif x > 0 && x < 9 {\n\t\tx++\n\t}\n\tpay.Charge(x)\n\treturn x\n}\n\nfunc (c *Cart) Total() int {\n        return 0\n}\n\nfunc (c *Cart) Discount() int { return 1 }\n";
const PAY: &str = "package pay\n\nfunc Charge(x int) {}\n";

#[test]
fn blocks_are_new_changed_unchanged_or_removed() {
    let base = side(&[("cart/cart.go", BASE_CART), ("pay/pay.go", PAY)]);
    let target = side(&[("cart/cart.go", TARGET_CART), ("pay/pay.go", PAY)]);
    let (bg, tg) = (base.file.graph(), target.file.graph());
    let r = compare(bg, Path::new(&base.dir), tg, Path::new(&target.dir));
    let at = |p: &[&str]| r.change(node(tg, p));
    assert_eq!(at(&["proj", "cart", "Cart", "Discount"]).status, Status::New);
    let add = at(&["proj", "cart", "Cart", "Add"]);
    assert_eq!(add.status, Status::Changed);
    assert_eq!((add.added, add.removed), (4, 0));
    assert_eq!(add.complexity, Some((1, 3)));
    assert_eq!(at(&["proj", "cart", "Cart", "Total"]).status, Status::Unchanged, "re-indenting is not a change");
    assert_eq!(at(&["proj", "pay", "Charge"]).status, Status::Unchanged);
    let removed: Vec<&str> = r.removed.iter().map(|x| x.path.as_str()).collect();
    assert_eq!(removed, ["proj/cart/Cart/Clear"]);
    assert_eq!(r.removed[0].parent, Some(node(tg, &["proj", "cart", "Cart"])));
    let cart = at(&["proj", "cart", "Cart"]);
    assert_eq!(cart.status, Status::Changed);
    assert_eq!((cart.inside.new, cart.inside.changed, cart.inside.removed), (1, 1, 1));
    let pkg = at(&["proj", "cart"]);
    assert_eq!((pkg.inside.new, pkg.inside.changed, pkg.inside.removed), (1, 2, 1), "Add and its type Cart changed");
    let arrows: Vec<(&str, &str)> = r.new_arrows.iter().map(|&(a, b)| (tg.name(a), tg.name(b))).collect();
    assert!(arrows.contains(&("cart", "pay")), "{arrows:?}");
    assert!(r.removed_arrows.is_empty());
}

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).current_dir(dir).output().unwrap();
    assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
}

#[test]
fn a_branch_is_compared_with_where_it_left_main() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let proj = repo.join("svc");
    std::fs::create_dir_all(proj.join("cart")).unwrap();
    std::fs::write(proj.join("go.mod"), "module example.com/svc\n\ngo 1.22\n").unwrap();
    std::fs::write(proj.join("cart/cart.go"), BASE_CART).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["checkout", "-q", "-b", "agent"]);
    std::fs::write(proj.join("cart/cart.go"), BASE_CART.replace("func (c *Cart) Clear() {}\n", "")).unwrap();
    git(&repo, &["commit", "-q", "-am", "drop clear"]);
    std::fs::write(proj.join("cart/extra.go"), "package cart\n\nfunc Extra() {}\n").unwrap();

    let base_rev = default_base(&proj).unwrap();
    let cache = tmp.path().join("cache");
    let cfg = ProjectConfig::default();
    let base = index_revision(&proj, &base_rev, &cfg, &cache, &|_| {}).unwrap();
    assert!(base.dir.ends_with("svc"), "{:?}", base.dir);
    assert!(base.dir.starts_with(&cache), "checked out under the cache, not in the repository");
    let again = index_revision(&proj, &base_rev, &cfg, &cache, &|_| {}).unwrap();
    assert_eq!(again.index, base.index, "a commit is indexed once");

    let target_cfg = ProjectConfig { index: Some(tmp.path().join("target.relief")), ..Default::default() };
    let target = index_project(&proj, &target_cfg, &|_| {}).unwrap();
    let (bf, tf) = (ReliefFile::open(&base.index).unwrap(), ReliefFile::open(&target.path).unwrap());
    let r = compare(bf.graph(), &base.dir, tf.graph(), &proj);
    let tg = tf.graph();
    assert_eq!(r.change(node(tg, &["svc", "cart", "Extra"])).status, Status::New, "uncommitted work counts");
    assert_eq!(r.removed.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(), ["svc/cart/Cart/Clear"]);
    let status = std::process::Command::new("git").args(["status", "--porcelain"]).current_dir(&repo).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&status.stdout).trim(), "?? svc/cart/extra.go", "the checkout is left alone");
}
