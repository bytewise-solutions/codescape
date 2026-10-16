use srcrelief_index::{Key, Settings, SettingsStore, View, filter_tag};
use srcrelief_layout::LayoutOptions;
use srcrelief_rules::VisualRules;

#[derive(Clone, Debug, PartialEq)]
pub struct ViewItem {
    pub key: Key,
    pub label: String,
    pub checked: bool,
}

pub fn layout_options(s: &Settings, visual: &VisualRules) -> LayoutOptions {
    let on = |v: View| s.view(v).value;
    let mut hidden_tags = visual.hide_tags.clone();
    for f in &s.filters {
        if !s.filter_shown(&f.value.name).is_some_and(|x| x.value) {
            hidden_tags.push(filter_tag(&f.value.name));
        }
    }
    LayoutOptions {
        include_test_edges: on(View::TestDeps),
        hidden_tags,
        show_tests: on(View::Tests),
        show_wiring: on(View::Wiring),
        show_noise: on(View::Errors),
        show_details: on(View::Details),
        ..LayoutOptions::default()
    }
}

pub fn view_items(s: &Settings) -> Vec<ViewItem> {
    let item = |v: View| ViewItem { key: Key::View(v), label: v.label().into(), checked: s.view(v).value };
    let mut out: Vec<ViewItem> = View::ALL.into_iter().filter(|v| !matches!(v, View::Matrix | View::QuietArrows | View::Legend)).map(item).collect();
    for f in &s.filters {
        let name = &f.value.name;
        out.push(ViewItem { key: Key::FilterShown(name.clone()), label: name.clone(), checked: s.filter_shown(name).is_some_and(|x| x.value) });
    }
    out.push(item(View::Matrix));
    out.push(item(View::QuietArrows));
    out.push(item(View::Legend));
    out
}

pub fn toggle(store: &mut SettingsStore, key: &Key, on: bool) {
    match key {
        Key::View(v) => store.set_view(*v, on),
        Key::FilterShown(n) => store.set_filter_shown(n, on),
        _ => {}
    }
}
