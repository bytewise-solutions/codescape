use srcrelief_format::Category;
use srcrelief_index::*;
use std::path::{Path, PathBuf};

struct Env {
    _tmp: tempfile::TempDir,
    project: PathBuf,
    paths: SettingsPaths,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("proj");
    std::fs::create_dir_all(&project).unwrap();
    let paths = SettingsPaths::with_root(&project, None, &tmp.path().join("config"));
    Env { _tmp: tmp, project, paths }
}

fn write(path: &Path, src: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, src).unwrap();
}

fn globs(s: &Settings) -> Vec<(&str, Layer)> {
    s.exclude.iter().map(|e| (e.value.as_str(), e.layer)).collect()
}

#[test]
fn personal_file_sits_next_to_user_defaults_keyed_like_the_cache() {
    let e = env();
    let root = e.paths.user.parent().unwrap().to_path_buf();
    assert_eq!(e.paths.user, root.join("settings.toml"));
    assert_eq!(e.paths.shared, e.project.join("srcrelief.toml"));
    let cache = cache_path(&e.project);
    let key = cache.file_stem().unwrap().to_str().unwrap();
    assert_eq!(e.paths.personal, root.join("projects").join(format!("{key}.toml")));
}

#[test]
fn scalars_come_from_the_highest_layer_that_sets_them() {
    let e = env();
    write(&e.paths.user, "name = \"Mine\"\n[view]\ndetails = true\nerrors = true\n");
    write(&e.paths.shared, "name = \"Team\"\n[view]\ndetails = true\n");
    write(&e.paths.personal, "[view]\ndetails = false\n");
    let store = SettingsStore::load(e.paths.clone()).unwrap();
    let s = store.settings();
    assert_eq!(s.name.as_ref().map(|n| (n.value.as_str(), n.layer)), Some(("Team", Layer::Shared)));
    assert_eq!(s.view(View::Details), Sourced { value: false, layer: Layer::Personal });
    assert_eq!(s.view(View::Errors), Sourced { value: true, layer: Layer::User });
    assert_eq!(s.view(View::Wiring), Sourced { value: false, layer: Layer::Default });
    assert_eq!(s.view(View::Tests), Sourced { value: false, layer: Layer::Default });
    assert_eq!(store.config().name.as_deref(), Some("Team"));
}

#[test]
fn lists_append_across_layers_and_a_higher_layer_can_remove_an_inherited_entry() {
    let e = env();
    write(&e.paths.user, "exclude = [\"a/**\"]\n");
    write(&e.paths.shared, "exclude = [\"b/**\"]\n[remove]\nexclude = [\"**/vendor/**\"]\n");
    write(&e.paths.personal, "exclude = [\"c/**\"]\n[remove]\nexclude = [\"a/**\"]\n");
    let store = SettingsStore::load(e.paths.clone()).unwrap();
    let got = globs(store.settings());
    assert!(got.contains(&("**/build/**", Layer::Default)), "{got:?}");
    assert!(got.contains(&("b/**", Layer::Shared)) && got.contains(&("c/**", Layer::Personal)), "{got:?}");
    assert!(!got.iter().any(|(g, _)| *g == "a/**" || *g == "**/vendor/**"), "{got:?}");
    let removed: Vec<(Key, Layer)> = store.settings().removed.iter().map(|r| (r.value.clone(), r.layer)).collect();
    assert!(removed.contains(&(Key::Exclude("a/**".into()), Layer::Personal)), "{removed:?}");
    let cfg = store.config();
    assert!(cfg.exclude.contains(&"b/**".to_string()) && !cfg.exclude.contains(&"**/vendor/**".to_string()));
}

#[test]
fn keyed_entries_override_by_key_and_report_their_layer() {
    let e = env();
    write(
        &e.paths.shared,
        "[[category]]\nglob = \"**/cmd/migrate/**\"\ncategory = \"dev-tool\"\n\n[[filter]]\nname = \"migrations\"\npaths = [\"**/migrations/**\"]\n\n[packs]\ndisable = [\"libgdx\"]\n\n[test]\npaths = [\"**/it/**\"]\n",
    );
    write(
        &e.paths.personal,
        "[[filter]]\nname = \"migrations\"\npaths = [\"db/**\"]\nnames = [\"Migration$\"]\nshown = false\n\n[packs]\nenable = [\"libgdx\"]\n\n[remove]\ncategory = [\"**/cmd/migrate/**\"]\n",
    );
    let store = SettingsStore::load(e.paths.clone()).unwrap();
    let s = store.settings();
    assert!(s.categories.is_empty(), "{:?}", s.categories);
    assert_eq!(s.filters.len(), 1);
    let f = &s.filters[0];
    assert_eq!((f.layer, f.value.paths.clone(), f.value.names.clone(), f.value.shown), (Layer::Personal, vec!["db/**".to_string()], vec!["Migration$".to_string()], false));
    assert_eq!(s.pack("libgdx"), Some(Sourced { value: true, layer: Layer::Personal }));
    assert_eq!(s.test_paths.iter().map(|t| (t.value.as_str(), t.layer)).collect::<Vec<_>>(), [("**/it/**", Layer::Shared)]);
    assert_eq!(s.filter_shown("migrations"), Some(Sourced { value: false, layer: Layer::Personal }));
    let cfg = store.config();
    assert!(cfg.packs.enable.contains(&"libgdx".to_string()) && !cfg.packs.disable.contains(&"libgdx".to_string()));
    assert_eq!(cfg.test_paths, ["**/it/**"]);
}

#[test]
fn display_toggles_write_only_the_personal_file_and_reload_identically() {
    let e = env();
    let mut store = SettingsStore::load(e.paths.clone()).unwrap();
    store.set_view(View::Details, true);
    store.set_view(View::Tests, true);
    store.set_filter_shown("generated", false);
    store.save_view().unwrap();
    assert!(e.paths.personal.is_file());
    assert!(!e.paths.user.exists() && !e.paths.shared.exists());
    assert_eq!(std::fs::read_dir(&e.project).unwrap().count(), 0, "nothing is written into the project");
    let reloaded = SettingsStore::load(e.paths.clone()).unwrap();
    assert_eq!(reloaded.settings(), store.settings());
    assert_eq!(reloaded.settings().view(View::Details), Sourced { value: true, layer: Layer::Personal });
    assert_eq!(reloaded.settings().filter_shown("generated"), Some(Sourced { value: false, layer: Layer::Personal }));
}

#[test]
fn index_edits_stay_a_draft_until_applied() {
    let e = env();
    let mut store = SettingsStore::load(e.paths.clone()).unwrap();
    store.add_exclude("gen/**").unwrap();
    assert!(store.index_dirty());
    assert!(globs(store.settings()).contains(&("gen/**", Layer::Personal)));
    store.set_view(View::Wiring, true);
    store.save_view().unwrap();
    let on_disk = SettingsStore::load(e.paths.clone()).unwrap();
    assert!(!globs(on_disk.settings()).iter().any(|(g, _)| *g == "gen/**"));
    assert!(on_disk.settings().view(View::Wiring).value);
    store.apply().unwrap();
    assert!(!store.index_dirty());
    let on_disk = SettingsStore::load(e.paths.clone()).unwrap();
    assert!(globs(on_disk.settings()).contains(&("gen/**", Layer::Personal)));
}

#[test]
fn removing_an_inherited_entry_records_a_removal_and_reset_brings_it_back() {
    let e = env();
    write(&e.paths.shared, "exclude = [\"b/**\"]\n");
    let mut store = SettingsStore::load(e.paths.clone()).unwrap();
    store.remove_exclude("b/**").unwrap();
    assert!(!globs(store.settings()).iter().any(|(g, _)| *g == "b/**"));
    store.reset(&Key::Exclude("b/**".into())).unwrap();
    assert!(globs(store.settings()).contains(&("b/**", Layer::Shared)));
    store.add_exclude("c/**").unwrap();
    store.remove_exclude("c/**").unwrap();
    assert!(!globs(store.settings()).iter().any(|(g, _)| *g == "c/**"));
    assert!(store.settings().removed.is_empty(), "{:?}", store.settings().removed);
}

#[test]
fn reset_falls_back_to_the_layer_below() {
    let e = env();
    write(&e.paths.shared, "[view]\ndetails = true\n");
    let mut store = SettingsStore::load(e.paths.clone()).unwrap();
    store.set_view(View::Details, false);
    assert_eq!(store.settings().view(View::Details).layer, Layer::Personal);
    store.reset(&Key::View(View::Details)).unwrap();
    assert_eq!(store.settings().view(View::Details), Sourced { value: true, layer: Layer::Shared });
}

#[test]
fn use_for_all_projects_moves_a_value_to_the_user_defaults() {
    let e = env();
    let mut store = SettingsStore::load(e.paths.clone()).unwrap();
    store.set_view(View::Wiring, true);
    store.promote(&Key::View(View::Wiring), Layer::User).unwrap();
    store.save_view().unwrap();
    assert_eq!(store.settings().view(View::Wiring), Sourced { value: true, layer: Layer::User });
    let user = std::fs::read_to_string(&e.paths.user).unwrap();
    assert!(user.contains("wiring = true"), "{user}");
    let personal = std::fs::read_to_string(&e.paths.personal).unwrap_or_default();
    assert!(!personal.contains("wiring"), "{personal}");
}

#[test]
fn save_to_the_shared_file_keeps_its_comments_and_other_keys() {
    let e = env();
    write(&e.paths.shared, "# Team settings, see the wiki\nname = \"Team\"   # shown in the title\n\n[packs]\ndisable = [\"libgdx\"]\n");
    let mut store = SettingsStore::load(e.paths.clone()).unwrap();
    store.add_exclude("gen/**").unwrap();
    store.promote(&Key::Exclude("gen/**".into()), Layer::Shared).unwrap();
    store.apply().unwrap();
    let shared = std::fs::read_to_string(&e.paths.shared).unwrap();
    assert!(shared.contains("# Team settings, see the wiki") && shared.contains("# shown in the title"), "{shared}");
    assert!(shared.contains("gen/**") && shared.contains("libgdx"), "{shared}");
    let cfg = ProjectConfig::load(&e.project, None).unwrap();
    assert!(cfg.exclude.contains(&"gen/**".to_string()));
    assert_eq!(cfg.name.as_deref(), Some("Team"));
    assert_eq!(store.settings().exclude.iter().find(|x| x.value == "gen/**").unwrap().layer, Layer::Shared);
}

#[test]
fn a_category_override_from_the_personal_layer_reaches_the_index_config() {
    let e = env();
    let mut store = SettingsStore::load(e.paths.clone()).unwrap();
    store.add_category(CategoryOverride { glob: "**/tools/**".into(), category: Category::DevTool }).unwrap();
    let cfg = store.config();
    assert_eq!(cfg.categories, [CategoryOverride { glob: "**/tools/**".into(), category: Category::DevTool }]);
}

#[test]
fn bad_values_are_reported_with_the_file_they_come_from() {
    let e = env();
    write(&e.paths.personal, "[[filter]]\nname = \"x\"\nnames = [\"(\"]\n");
    let err = SettingsStore::load(e.paths.clone()).err().unwrap().to_string();
    assert!(err.contains(&e.paths.personal.display().to_string()), "{err}");
    let e = env();
    let mut store = SettingsStore::load(e.paths.clone()).unwrap();
    assert!(store.put_filter(Filter { name: "x".into(), paths: vec![], names: vec!["(".into()], shown: true }).is_err());
    assert!(store.settings().filters.is_empty());
}

#[test]
fn the_legend_is_shown_by_default_and_remembered_when_hidden() {
    let e = env();
    let mut store = SettingsStore::load(e.paths.clone()).unwrap();
    assert_eq!(store.settings().view(View::Legend), Sourced { value: true, layer: Layer::Default });
    store.set_view(View::Legend, false);
    store.save_view().unwrap();
    let reloaded = SettingsStore::load(e.paths.clone()).unwrap();
    assert_eq!(reloaded.settings().view(View::Legend), Sourced { value: false, layer: Layer::Personal });
}

#[test]
fn quiet_arrows_are_on_by_default() {
    let e = env();
    let store = SettingsStore::load(e.paths.clone()).unwrap();
    assert_eq!(store.settings().view(View::QuietArrows), Sourced { value: true, layer: Layer::Default });
}
