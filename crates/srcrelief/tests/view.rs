use srcrelief::view::{ViewItem, layout_options, view_items};
use srcrelief_index::{Key, SettingsPaths, SettingsStore, View};
use srcrelief_layout::LayoutOptions;
use srcrelief_rules::VisualRules;

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

const FILTERS: &str = "[[filter]]\nname = \"migrations\"\npaths = [\"**/migrations/**\"]\nshown = false\n\n[[filter]]\nname = \"generated\"\npaths = [\"**/gen/**\"]\n";

#[test]
fn default_settings_give_the_default_layout() {
    let (_t, s) = store("");
    assert_eq!(layout_options(s.settings(), &VisualRules::default()), LayoutOptions::default());
}

#[test]
fn display_settings_map_onto_layout_options() {
    let (_t, s) = store(&format!("[view]\ntests = true\ntest_deps = true\ndetails = true\nerrors = true\nwiring = true\nfilters = {{ generated = false }}\n\n{FILTERS}"));
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
fn view_menu_lists_built_in_toggles_then_filters_then_the_legend() {
    let (_t, s) = store(FILTERS);
    let items = view_items(s.settings());
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["Tests", "Test dependencies", "Details", "Errors", "Wiring", "migrations", "generated", "Legend"]);
    assert_eq!(items[5], ViewItem { key: Key::FilterShown("migrations".into()), label: "migrations".into(), checked: false });
    assert_eq!(items[6].checked, true);
    assert_eq!(items[7], ViewItem { key: Key::View(View::Legend), label: "Legend".into(), checked: true });
}
