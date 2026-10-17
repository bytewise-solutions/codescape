use srcrelief::review_view::*;
use srcrelief_format::*;
use srcrelief_index::review::compare;
use srcrelief_index::{ProjectConfig, index_project};

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
    Side { file: ReliefFile::open(&out.path).unwrap(), _tmp: tmp, dir }
}

const BASE: &str = "package cart\n\ntype Cart struct{}\n\nfunc (c *Cart) Add(x int) int {\n\treturn x\n}\n\nfunc (c *Cart) Clear() {}\n";
const TARGET: &str = "package cart\n\ntype Cart struct{}\n\nfunc (c *Cart) Add(x int) int {\n\tif x > 0 {\n\t\tx++\n\t}\n\treturn x\n}\n\ntype Coupon struct{}\n\nfunc (k *Coupon) Apply() {}\n";

#[test]
fn the_change_list_names_each_change_once_grouped_by_package() {
    let (b, t) = (side(&[("cart/cart.go", BASE)]), side(&[("cart/cart.go", TARGET)]));
    let tg = t.file.graph();
    let r = compare(b.file.graph(), &b.dir, tg, &t.dir);
    let items = items(tg, &r);
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
