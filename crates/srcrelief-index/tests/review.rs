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
    if !files.iter().any(|(rel, _)| *rel == "package.json") {
        std::fs::write(dir.join("go.mod"), "module example.com/shop\n\ngo 1.22\n").unwrap();
    }
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

    let base_rev = base_for(&proj, None).unwrap();
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
    assert_eq!(git_out(&repo, &["worktree", "list"]).lines().count(), 1, "no worktree is registered in the repository");
    assert!(!repo.join(".git/worktrees").exists());
}

fn small_repo(tmp: &Path) -> std::path::PathBuf {
    let repo = tmp.join("repo");
    std::fs::create_dir_all(repo.join("cart")).unwrap();
    std::fs::write(repo.join("go.mod"), "module example.com/svc\n\ngo 1.22\n").unwrap();
    std::fs::write(repo.join("cart/cart.go"), BASE_CART).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    repo
}

#[test]
fn cached_versions_unused_for_a_while_are_removed_with_old_worktrees() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = small_repo(tmp.path());
    let cache = tmp.path().join("cache");
    let cfg = ProjectConfig::default();
    let head = git_out(&repo, &["rev-parse", "HEAD"]);
    let fresh = index_revision(&repo, &head, &cfg, &cache, &|_| {}).unwrap();
    std::fs::write(repo.join("cart/b.go"), "package cart\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "b"]);
    let second = git_out(&repo, &["rev-parse", "HEAD"]);
    let stale = index_revision(&repo, &second, &cfg, &cache, &|_| {}).unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 24 * 3600);
    mark_used_at(&stale, old);
    let legacy = cache.join("worktrees").join(format!("{}-{}", srcrelief_index::project_key(&repo.canonicalize().unwrap()), &head[..12]));
    git(&repo, &["worktree", "add", "-q", "--detach", &legacy.to_string_lossy(), &head]);

    let removed = clean_versions(&repo, &cache, std::time::Duration::from_secs(14 * 24 * 3600)).unwrap();
    assert_eq!(removed, 2, "the stale version and the old worktree");
    assert!(fresh.dir.exists() && fresh.index.exists());
    assert!(!stale.dir.exists() && !stale.index.exists());
    assert!(!legacy.exists());
    assert_eq!(git_out(&repo, &["worktree", "list"]).lines().count(), 1);
    assert_eq!(clean_versions(&repo, &cache, std::time::Duration::ZERO).unwrap(), 1, "clearing removes everything left");
    assert!(!fresh.dir.exists());
}

#[test]
fn files_are_diffed_line_by_line_ignoring_indentation() {
    let base = side(&[("cart/cart.go", BASE_CART), ("pay/pay.go", PAY)]);
    let target = side(&[("cart/cart.go", TARGET_CART), ("pay/pay.go", PAY)]);
    let r = compare(base.file.graph(), Path::new(&base.dir), target.file.graph(), Path::new(&target.dir));
    let d = r.file("cart/cart.go").expect("cart.go differs");
    for line in [3, 8, 9, 10, 11, 19] {
        assert!(d.added.contains(&line), "line {line} is new: {:?}", d.added);
    }
    assert!(!d.added.contains(&16), "re-indented line");
    let gone: Vec<&String> = d.removed.iter().flat_map(|(_, lines)| lines).collect();
    assert!(gone.iter().any(|l| l.contains("Clear()")), "{gone:?}");
    assert!(r.file("pay/pay.go").is_none());
}

#[test]
fn a_mark_follows_the_text_of_a_block() {
    let base = side(&[("cart/cart.go", BASE_CART), ("pay/pay.go", PAY)]);
    let target = side(&[("cart/cart.go", TARGET_CART), ("pay/pay.go", PAY)]);
    let again = side(&[("cart/cart.go", &TARGET_CART.replace("x++", "x += 2")), ("pay/pay.go", PAY)]);
    let (bg, tg, ag) = (base.file.graph(), target.file.graph(), again.file.graph());
    let first = compare(bg, Path::new(&base.dir), tg, Path::new(&target.dir));
    let second = compare(bg, Path::new(&base.dir), ag, Path::new(&again.dir));
    let print = |r: &Review, g: &ArchivedGraph, m: &str| r.change(node(g, &["proj", "cart", "Cart", m])).fingerprint;
    assert_eq!(print(&first, tg, "Discount"), print(&second, ag, "Discount"), "same text, same mark");
    assert_ne!(print(&first, tg, "Add"), print(&second, ag, "Add"), "the agent changed Add again");
    assert_ne!(print(&first, tg, "Add"), print(&first, tg, "Discount"));
}

#[test]
fn signature_and_visibility_changes_are_told() {
    let base = side(&[("cart/cart.go", BASE_CART), ("pay/pay.go", PAY)]);
    let target = side(&[("cart/cart.go", &TARGET_CART.replace("Total() int", "Total(tax int) int")), ("pay/pay.go", PAY)]);
    let r = compare(base.file.graph(), Path::new(&base.dir), target.file.graph(), Path::new(&target.dir));
    let tg = target.file.graph();
    assert!(r.change(node(tg, &["proj", "cart", "Cart", "Total"])).signature);
    assert!(!r.change(node(tg, &["proj", "cart", "Cart", "Add"])).signature, "only the body changed");

    let ts = |vis: &str| format!("export class Cart {{\n  {vis}helper(): number {{\n    return 1;\n  }}\n}}\n");
    let base = side(&[("package.json", "{\"name\":\"shop\"}"), ("src/cart.ts", &ts("private "))]);
    let target = side(&[("package.json", "{\"name\":\"shop\"}"), ("src/cart.ts", &ts(""))]);
    let r = compare(base.file.graph(), Path::new(&base.dir), target.file.graph(), Path::new(&target.dir));
    let tg = target.file.graph();
    let helper = (0..tg.node_count() as NodeId).find(|&n| tg.name(n) == "helper").unwrap();
    assert_eq!(r.change(helper).widened, Some((Visibility::Private, Visibility::Public)));
}

#[test]
fn branches_and_the_commits_between_two_versions_are_listed() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!is_git(tmp.path()));
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("cart")).unwrap();
    std::fs::write(repo.join("go.mod"), "module example.com/svc\n\ngo 1.22\n").unwrap();
    std::fs::write(repo.join("cart/cart.go"), BASE_CART).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    let main = git_out(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["checkout", "-q", "-b", "agent"]);
    std::fs::write(repo.join("cart/a.go"), "package cart\n\nfunc A() {}\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "add A"]);
    std::fs::write(repo.join("cart/b.go"), "package cart\n\nfunc B() {}\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "add B"]);
    git(&repo, &["checkout", "-q", "main"]);

    assert!(is_git(&repo.join("cart")));
    assert_eq!(current_branch(&repo).as_deref(), Some("main"));
    assert_eq!(branches(&repo), ["main", "agent"], "the checked-out branch first");
    let subjects: Vec<String> = commits(&repo, "main", "agent").unwrap().into_iter().map(|c| c.subject).collect();
    assert_eq!(subjects, ["add A", "add B"], "oldest first");
    assert_eq!(base_for(&repo, Some("agent")).unwrap(), main, "where agent left main");
    assert!(!has_uncommitted(&repo));
    std::fs::write(repo.join("cart/c.go"), "package cart\n").unwrap();
    assert!(has_uncommitted(&repo));
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}
