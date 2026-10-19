use codescape_format::*;
use codescape_index::clones::{CloneGroup, find_clones};
use codescape_index::{ProjectConfig, index_project};
use std::path::Path;

const CART: &str = r#"package shop;

public class Cart {
    int total(int[] prices, String label) {
        int sum = 0;
        for (int i = 0; i < prices.length; i++) {
            if (prices[i] > 100) {
                sum += prices[i] * 2;
            } else {
                sum += prices[i];
            }
        }
        System.out.println("total for " + label + ": " + sum);
        return sum;
    }
}
"#;

const ORDER: &str = r#"package shop;

public class Order {
    private int unrelated() { return 4; }

    int amount(int[] costs, String name) {
        int acc = 0;
        for (int j = 0; j < costs.length; j++) {
            if (costs[j] > 250) {
                acc += costs[j] * 2;
            } else {
                acc += costs[j];
            }
        }
        System.out.println("amount of " + name + " = " + acc);
        return acc;
    }
}
"#;

const SPAM: &str = r#"package shop;

public class Spam {
    void spam(StringBuilder sb) {
        sb.append("a").append(1).append(2);
        sb.append("b").append(3).append(4);
        sb.append("c").append(5).append(6);
        sb.append("d").append(7).append(8);
        sb.append("e").append(9).append(10);
        sb.append("f").append(11).append(12);
        sb.append("g").append(13).append(14);
        sb.append("h").append(15).append(16);
    }
}
"#;

fn small(class: &str, var: &str) -> String {
    format!(
        "package shop;\n\npublic class {class} {{\n    int small(int[] xs) {{\n        int {var} = 0;\n        for (int i = 0; i < xs.length; i++) {{\n            {var} += xs[i] * 3;\n        }}\n        return {var};\n    }}\n}}\n"
    )
}

fn project(dir: &Path, files: &[(&str, &str)]) {
    std::fs::create_dir_all(dir).unwrap();
    for (rel, text) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

fn index(files: &[(&str, &str)], min_tokens: Option<u32>) -> (tempfile::TempDir, IndexFile) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("proj");
    project(&dir, &[("settings.gradle", "rootProject.name = 'proj'\n"), ("build.gradle", "apply plugin: 'java'\n")]);
    project(&dir, files);
    let mut cfg = ProjectConfig { index: Some(tmp.path().join("i.codescape")), ..Default::default() };
    if let Some(n) = min_tokens {
        cfg.duplicate_tokens = n;
    }
    let file = IndexFile::open(&index_project(&dir, &cfg, &|_| {}).unwrap().path).unwrap();
    (tmp, file)
}

fn named(g: &ArchivedGraph, name: &str) -> NodeId {
    (0..g.node_count() as NodeId).find(|&n| g.name(n) == name).unwrap_or_else(|| panic!("no {name}"))
}

fn span(g: &ArchivedGraph, n: NodeId) -> (u32, u32) {
    (g.node(n).start_line.to_native(), g.node(n).end_line.to_native())
}

#[test]
fn copies_with_renamed_variables_and_other_strings_form_one_group() {
    let (_tmp, file) = index(&[("src/main/java/shop/Cart.java", CART), ("src/main/java/shop/Order.java", ORDER)], None);
    let g = file.graph();
    let (total, amount) = (named(g, "total"), named(g, "amount"));
    let groups: Vec<_> = g.clone_groups().collect();
    assert_eq!(groups.len(), 1, "{groups:?}");
    let (tokens, places) = &groups[0];
    assert!(*tokens >= 50, "{tokens}");
    let (ts, te) = span(g, total);
    let (as_, ae) = span(g, amount);
    assert_eq!(places, &vec![(total, ts, te), (amount, as_, ae)]);
    assert_eq!(g.clones_of(total), vec![(0, vec![(amount, as_, ae)])]);
    assert_eq!(g.clones_of(amount), vec![(0, vec![(total, ts, te)])]);
    assert!(g.clones_of(named(g, "unrelated")).is_empty());
}

#[test]
fn a_method_repeating_a_statement_inside_itself_is_not_a_clone() {
    let (_tmp, file) = index(&[("src/main/java/shop/Spam.java", SPAM)], None);
    assert_eq!(file.graph().clone_groups().count(), 0);
}

#[test]
fn copies_shorter_than_the_threshold_are_ignored() {
    let files = [("src/main/java/shop/A.java", small("A", "s")), ("src/main/java/shop/B.java", small("B", "total"))];
    let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let (_tmp, file) = index(&files, None);
    assert_eq!(file.graph().clone_groups().count(), 0, "37 tokens is below the default 50");
    let (_tmp, file) = index(&files, Some(30));
    let groups: Vec<_> = file.graph().clone_groups().collect();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, 37);
}

#[test]
fn test_code_is_left_out() {
    let test_cart = CART.replace("class Cart", "class CartTest");
    let test_order = ORDER.replace("class Order", "class OrderTest");
    let (_tmp, file) = index(
        &[("src/test/java/shop/CartTest.java", &test_cart), ("src/test/java/shop/OrderTest.java", &test_order), ("src/main/java/shop/Spam.java", SPAM)],
        None,
    );
    assert_eq!(file.graph().clone_groups().count(), 0);
}

fn seq(from: u32, n: u32) -> Vec<u32> {
    (from..from + n).collect()
}

fn lines(t: &[u32]) -> Vec<u32> {
    (1..=t.len() as u32).collect()
}

#[test]
fn a_match_extends_as_far_as_the_copies_agree() {
    let a = seq(1, 80);
    let b: Vec<u32> = [vec![900], seq(1, 80), vec![901]].concat();
    let (la, lb) = (lines(&a), lines(&b));
    let groups = find_clones(&[(7, &a, &la), (9, &b, &lb)], 50);
    assert_eq!(groups, vec![CloneGroup { tokens: 80, places: vec![(7, 1, 80), (9, 2, 81)] }]);
}

#[test]
fn three_copies_make_one_group_and_longer_groups_come_first() {
    let x = seq(1, 70);
    let y = seq(500, 55);
    let a: Vec<u32> = [x.clone(), vec![900], y.clone()].concat();
    let b: Vec<u32> = [vec![901], x.clone()].concat();
    let c: Vec<u32> = [y.clone(), vec![902]].concat();
    let d = y.clone();
    let (la, lb, lc, ld) = (lines(&a), lines(&b), lines(&c), lines(&d));
    let groups = find_clones(&[(1, &a, &la), (2, &b, &lb), (3, &c, &lc), (4, &d, &ld)], 50);
    assert_eq!(
        groups,
        vec![CloneGroup { tokens: 70, places: vec![(1, 1, 70), (2, 2, 71)] }, CloneGroup { tokens: 55, places: vec![(1, 72, 126), (3, 1, 55), (4, 1, 55)] },]
    );
}

#[test]
fn a_body_repeating_a_copy_counts_once() {
    let x = seq(1, 60);
    let a: Vec<u32> = [x.clone(), x.clone()].concat();
    let b = x.clone();
    let (la, lb) = (lines(&a), lines(&b));
    let groups = find_clones(&[(1, &a, &la), (2, &b, &lb)], 50);
    assert_eq!(groups, vec![CloneGroup { tokens: 60, places: vec![(1, 1, 60), (2, 1, 60)] }]);
    let alone = find_clones(&[(1, &a, &la)], 50);
    assert!(alone.is_empty(), "{alone:?}");
}

#[test]
fn a_shorter_part_shared_by_more_places_is_its_own_group() {
    let x = seq(1, 90);
    let a = x.clone();
    let b = x.clone();
    let c: Vec<u32> = [vec![900], x[10..80].to_vec(), vec![901]].concat();
    let d: Vec<u32> = [vec![902], x[10..80].to_vec(), vec![903]].concat();
    let (la, lb, lc, ld) = (lines(&a), lines(&b), lines(&c), lines(&d));
    let groups = find_clones(&[(1, &a, &la), (2, &b, &lb), (3, &c, &lc), (4, &d, &ld)], 50);
    assert_eq!(
        groups,
        vec![
            CloneGroup { tokens: 90, places: vec![(1, 1, 90), (2, 1, 90)] },
            CloneGroup { tokens: 70, places: vec![(1, 11, 80), (2, 11, 80), (3, 2, 71), (4, 2, 71)] },
        ]
    );
}

#[test]
fn a_repeated_pattern_copied_at_another_phase_is_reported_once() {
    let p = [1, 2, 3, 4, 5];
    let a: Vec<u32> = [vec![100], p.repeat(14), vec![101]].concat();
    let b: Vec<u32> = [vec![200], p[2..].to_vec(), p.repeat(19), vec![201]].concat();
    let (la, lb) = (lines(&a), lines(&b));
    let groups = find_clones(&[(1, &a, &la), (2, &b, &lb)], 50);
    assert_eq!(groups, vec![CloneGroup { tokens: 70, places: vec![(1, 2, 71), (2, 5, 74)] }]);
}

#[test]
fn overloads_sharing_one_block_are_not_copies_of_each_other() {
    let x = seq(1, 60);
    let other: Vec<u32> = [vec![900], x.clone()].concat();
    let (lx, lo) = (lines(&x), lines(&other));
    assert!(find_clones(&[(5, &x, &lx), (5, &other, &lo)], 50).is_empty());
    let groups = find_clones(&[(5, &x, &lx), (6, &other, &lo), (5, &x, &lx)], 50);
    assert_eq!(groups, vec![CloneGroup { tokens: 60, places: vec![(5, 1, 60), (6, 2, 61)] }]);
}
