use codescape::view::{ViewItem, layout_options, view_items};
use codescape_index::{Key, SettingsPaths, SettingsStore, View};
use codescape_layout::LayoutOptions;
use codescape_rules::VisualRules;

fn store(personal: &str) -> (tempfile::TempDir, SettingsStore) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("p");
    std::fs::create_dir_all(&project).unwrap();
    let paths = SettingsPaths::with_root(&project, None, &tmp.path().join("config"));
    std::fs::create_dir_all(paths.personal.parent().unwrap()).unwrap();
    std::fs::write(&paths.personal, personal).unwrap();
    let store = SettingsStore::load(paths).unwrap();
    (tmp, store)
}

const FILTERS: &str =
    "[[filter]]\nname = \"migrations\"\npaths = [\"**/migrations/**\"]\nshown = false\n\n[[filter]]\nname = \"generated\"\npaths = [\"**/gen/**\"]\n";

#[test]
fn default_settings_give_the_default_layout() {
    let (_t, s) = store("");
    assert_eq!(layout_options(s.settings(), &VisualRules::default()), LayoutOptions::default());
}

#[test]
fn display_settings_map_onto_layout_options() {
    let (_t, s) =
        store(&format!("[view]\ntests = true\ntest_deps = true\ndetails = true\nerrors = true\nwiring = true\nfilters = {{ generated = false }}\n\n{FILTERS}"));
    let vis = VisualRules { hide_tags: vec!["noise:logging".into()], ..Default::default() };
    let o = layout_options(s.settings(), &vis);
    assert!(o.show_tests && o.include_test_edges && o.show_details && o.show_noise && o.show_wiring);
    assert_eq!(o.hidden_tags, ["noise:logging", "filter:migrations", "filter:generated"]);
}

#[test]
fn a_shown_filter_hides_nothing() {
    let (_t, s) = store(&format!("[view]\nfilters = {{ migrations = true }}\n\n{FILTERS}"));
    assert!(layout_options(s.settings(), &VisualRules::default()).hidden_tags.is_empty());
}

#[test]
fn view_menu_lists_built_in_toggles_then_filters_then_arrows_and_legend() {
    let (_t, s) = store(FILTERS);
    let items = view_items(s.settings());
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Tests", "Test dependencies", "Details", "Errors", "Wiring", "Complexity hints", "Unused code", "migrations", "generated", "Quiet arrows", "Legend"]
    );
    assert_eq!(items[5], ViewItem { key: Key::View(View::Complexity), label: "Complexity hints".into(), checked: true });
    assert_eq!(items[6], ViewItem { key: Key::View(View::Unused), label: "Unused code".into(), checked: false });
    assert_eq!(items[7], ViewItem { key: Key::FilterShown("migrations".into()), label: "migrations".into(), checked: false });
    assert!(items[8].checked);
    assert_eq!(items[9], ViewItem { key: Key::View(View::QuietArrows), label: "Quiet arrows".into(), checked: true });
    assert_eq!(items[10], ViewItem { key: Key::View(View::Legend), label: "Legend".into(), checked: true });
}

#[test]
fn map_switches_reach_the_layout_options() {
    let (_t, s) = store(
        "[view]\ngenerated = true\nprivate_members = false\nfields = false\naccessors = false\nconstructors = false\nimplements_arrows = false\nimport_arrows = false\nguessed_calls = false\n",
    );
    let o = layout_options(s.settings(), &VisualRules::default());
    assert!(o.show_generated && !o.implements_arrows && !o.import_arrows && !o.guessed_calls);
    assert_eq!(o.card, codescape_layout::CardOptions { private: false, fields: false, accessors: false, constructors: false });
}

#[test]
fn hidden_paths_and_name_patterns_resolve_to_blocks() {
    use codescape_format::*;
    let mut b = GraphBuilder::new("proj");
    let m = b.add_node(b.root(), NodeKind::Module, SubKind::None, "app", Lang::Java);
    let p = b.add_node(m, NodeKind::Package, SubKind::None, "cart", Lang::Java);
    for t in ["Cart", "CartException", "Coupon"] {
        b.add_node(p, NodeKind::Type, SubKind::Class, t, Lang::Java);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let (_t, s) = store("[view]\nhidden = [\"app/cart/Coupon\"]\nhidden_names = [\"Exception$\"]\nalways_shown = [\"app/cart/Cart\"]\n");
    let (hidden, always) = codescape::view::resolve_hidden(g, s.settings());
    let names = |set: &std::collections::HashSet<NodeId>| {
        let mut v: Vec<&str> = set.iter().map(|&n| g.name(n)).collect();
        v.sort();
        v
    };
    assert_eq!(names(&hidden), ["CartException", "Coupon"]);
    assert_eq!(names(&always), ["Cart"]);
    let (_t2, s2) = store("[view]\nhidden_names = [\"^Ca\"]\nalways_shown = [\"app/cart/Cart\"]\n");
    let (hidden, _) = codescape::view::resolve_hidden(g, s2.settings());
    assert_eq!(names(&hidden), ["CartException"], "always showing a block beats a name pattern that matches it");
    assert_eq!(codescape::view::block_path(g, (0..g.node_count() as NodeId).find(|&n| g.name(n) == "Coupon").unwrap()), "app/cart/Coupon");
}

#[test]
fn a_name_offers_its_last_word_to_hide_alike_names() {
    use codescape::view::name_suffix;
    assert_eq!(name_suffix("CartException"), Some("Exception"));
    assert_eq!(name_suffix("HTTPError"), Some("Error"));
    assert_eq!(name_suffix("getUserName"), Some("Name"));
    assert_eq!(name_suffix("Cart"), None);
    assert_eq!(name_suffix("toDo"), None, "too short to be a family of names");
}
