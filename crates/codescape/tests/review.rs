use codescape::review_view::*;
use codescape_format::*;
use codescape_index::analysis::analyse;
use codescape_index::review::compare;
use codescape_index::{ProjectConfig, index_project};

struct Side {
    _tmp: tempfile::TempDir,
    dir: std::path::PathBuf,
    file: IndexFile,
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
    let cfg = ProjectConfig { index: Some(tmp.path().join("i.codescape")), ..Default::default() };
    let out = index_project(&dir, &cfg, &|_| {}).unwrap();
    Side { file: IndexFile::open(&out.path).unwrap(), _tmp: tmp, dir }
}

const BASE: &str = "package cart\n\ntype Cart struct{}\n\nfunc (c *Cart) Add(x int) int {\n\treturn x\n}\n\nfunc (c *Cart) Clear() {}\n";
const TARGET: &str = "package cart\n\ntype Cart struct{}\n\nfunc (c *Cart) Add(x int) int {\n\tif x > 0 {\n\t\tx++\n\t}\n\treturn x\n}\n\ntype Coupon struct{}\n\nfunc (k *Coupon) Apply() {}\n";

#[test]
fn the_change_list_names_each_change_once_grouped_by_package() {
    let (b, t) = (side(&[("cart/cart.go", BASE)]), side(&[("cart/cart.go", TARGET)]));
    let tg = t.file.graph();
    let r = compare(b.file.graph(), &b.dir, tg, &t.dir);
    let items = items(tg, &r, &analyse(tg, &t.dir));
    let shown: Vec<(&str, &str, ItemKind)> = items.iter().map(|i| (i.group.as_str(), i.label.as_str(), i.kind)).collect();
    assert_eq!(
        shown,
        [("proj / cart", "Cart.Add", ItemKind::Changed), ("proj / cart", "Cart.Clear", ItemKind::Removed), ("proj / cart", "Coupon", ItemKind::New)],
        "a new type is listed once, not with each of its methods; its unchanged parent type is not listed"
    );
    assert_eq!(items[0].detail, "+3 −0 · complexity 1 to 2");
    assert!(items[1].node.is_none() && items[1].parent.is_some());
}

#[test]
fn n_and_p_step_through_the_changes_and_wrap() {
    assert_eq!(step(3, None, true), Some(0));
    assert_eq!(step(3, Some(0), true), Some(1));
    assert_eq!(step(3, Some(2), true), Some(0));
    assert_eq!(step(3, Some(0), false), Some(2));
    assert_eq!(step(0, None, true), None);
}

const TANGLED: &str = "package cart\n\ntype Cart struct{}\n\nfunc (c *Cart) Add(x int) int {\n\tif x > 1 {\n\t\tx++\n\t}\n\tif x > 2 {\n\t\tx++\n\t}\n\tif x > 3 {\n\t\tx++\n\t}\n\tif x > 4 {\n\t\tx++\n\t}\n\tif x > 5 || x < 0 {\n\t\tx++\n\t}\n\tif x > 6 && x < 99 {\n\t\tx++\n\t}\n\tfor x > 7 {\n\t\tx--\n\t}\n\treturn x\n}\n\ntype Coupon struct{}\n\nfunc (k *Coupon) Apply() {}\n";

#[test]
fn changes_are_ordered_by_risk_with_their_reasons() {
    let (b, t) = (side(&[("cart/cart.go", BASE)]), side(&[("cart/cart.go", TANGLED)]));
    let tg = t.file.graph();
    let r = compare(b.file.graph(), &b.dir, tg, &t.dir);
    let mut items = items(tg, &r, &analyse(tg, &t.dir));
    by_risk(&mut items);
    let shown: Vec<(&str, Vec<String>)> = items.iter().map(|i| (i.label.as_str(), i.risks.iter().map(Risk::label).collect())).collect();
    assert_eq!(shown[0], ("Cart.Add", vec!["complexity 1 to 10".to_string()]));
    assert_eq!(shown[1], ("Coupon", vec!["no caller in the project".to_string()]));
    assert_eq!(shown[2], ("Cart.Clear", vec![]));
}

#[test]
fn a_reviewed_mark_holds_until_the_block_changes_again() {
    let b = side(&[("cart/cart.go", BASE)]);
    let (t1, t2) = (side(&[("cart/cart.go", TARGET)]), side(&[("cart/cart.go", &TARGET.replace("x++", "x += 2"))]));
    let list = |t: &Side| items(t.file.graph(), &compare(b.file.graph(), &b.dir, t.file.graph(), &t.dir), &analyse(t.file.graph(), &t.dir));
    let (first, second) = (list(&t1), list(&t2));
    let mut marks = ReviewMarks::default();
    assert_eq!(marks.state(&first[0]), MarkState::Unseen);
    for i in &first {
        marks.toggle(i);
    }
    assert!(first.iter().all(|i| marks.state(i) == MarkState::Reviewed));
    let path = b._tmp.path().join("p.marks");
    marks.save(&path).unwrap();
    let marks = ReviewMarks::load(&path);
    let states: Vec<(&str, MarkState)> = second.iter().map(|i| (i.label.as_str(), marks.state(i))).collect();
    assert_eq!(states, [("Cart.Add", MarkState::ChangedSince), ("Cart.Clear", MarkState::Reviewed), ("Coupon", MarkState::Reviewed)]);
}

#[test]
fn each_commit_step_compares_a_commit_with_its_parent() {
    use codescape_index::review::Commit;
    let steps = [CommitStep::Commit(Commit { sha: "abc".into(), subject: "add A".into() }), CommitStep::Uncommitted];
    assert_eq!(steps[0].range(), (Some("abc^".to_string()), Some("abc".to_string())));
    assert_eq!(steps[1].range(), (Some("HEAD".to_string()), None), "the working copy against its last commit");
    assert_eq!(steps[0].label(), "add A");
    assert_eq!(steps[1].label(), "Uncommitted changes");
}

#[test]
fn risk_thresholds_follow_the_settings() {
    let (b, t) = (side(&[("cart/cart.go", BASE)]), side(&[("cart/cart.go", TANGLED)]));
    let tg = t.file.graph();
    let r = compare(b.file.graph(), &b.dir, tg, &t.dir);
    let a = analyse(tg, &t.dir);
    let add = |items: &[ReviewItem]| items.iter().find(|i| i.label == "Cart.Add").unwrap().risks.clone();
    assert_eq!(add(&items(tg, &r, &a)), [Risk::Complexity(1, 10)]);
    let strict = Limits { complexity_hint: 11, ..Limits::default() };
    assert!(add(&items_with(tg, &r, &a, strict)).is_empty(), "10 is below a hint level of 11");
}

#[test]
fn a_selection_marks_every_change_inside_it_reviewed() {
    let (b, t) = (side(&[("cart/cart.go", BASE)]), side(&[("cart/cart.go", TARGET)]));
    let tg = t.file.graph();
    let items = items(tg, &compare(b.file.graph(), &b.dir, tg, &t.dir), &analyse(tg, &t.dir));
    let named = |name: &str| (0..tg.node_count() as NodeId).find(|&n| tg.name(n) == name).unwrap_or_else(|| panic!("no {name}"));
    let labels = |found: Vec<usize>| found.into_iter().map(|i| items[i].label.as_str()).collect::<Vec<_>>();
    assert_eq!(labels(within(tg, &items, named("cart"))), ["Cart.Add", "Cart.Clear", "Coupon"], "a package holds all its changes");
    assert_eq!(labels(within(tg, &items, named("Cart"))), ["Cart.Add", "Cart.Clear"], "a removed method counts under its type");
    assert_eq!(labels(within(tg, &items, named("Add"))), ["Cart.Add"]);
    let mut marks = ReviewMarks::default();
    let inside = within(tg, &items, named("Cart"));
    assert!(!marks.all_reviewed(&items, &inside));
    marks.set_reviewed(&items, &inside, true);
    assert!(marks.all_reviewed(&items, &inside));
    assert_eq!(marks.state(&items[2]), MarkState::Unseen, "outside the selection stays as it was");
    marks.set_reviewed(&items, &inside, false);
    assert_eq!(marks.state(&items[0]), MarkState::Unseen);
}
