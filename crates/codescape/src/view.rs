use codescape_format::{ArchivedGraph, NodeFlags, NodeId, NodeKind};
use codescape_index::{Key, Settings, SettingsStore, View, filter_tag};
use codescape_layout::{CardOptions, LayoutOptions};
use codescape_rules::VisualRules;
use std::collections::HashSet;

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
        show_generated: on(View::Generated),
        card: CardOptions { private: on(View::PrivateMembers), fields: on(View::Fields), accessors: on(View::Accessors), constructors: on(View::Constructors) },
        implements_arrows: on(View::ImplementsArrows),
        import_arrows: on(View::ImportArrows),
        guessed_calls: on(View::GuessedCalls),
        ..LayoutOptions::default()
    }
}

pub fn map_options(g: &ArchivedGraph, s: &Settings, visual: &VisualRules) -> LayoutOptions {
    let mut opts = layout_options(s, visual);
    (opts.hidden_nodes, opts.always_shown) = resolve_hidden(g, s);
    opts
}

pub fn block_path(g: &ArchivedGraph, n: NodeId) -> String {
    g.path(n).iter().skip(1).map(|&p| g.name(p)).collect::<Vec<_>>().join("/")
}

pub fn resolve_hidden(g: &ArchivedGraph, s: &Settings) -> (HashSet<NodeId>, HashSet<NodeId>) {
    let patterns: Vec<regex::Regex> = s.hidden_names.iter().filter_map(|p| regex::Regex::new(&p.value).ok()).collect();
    let paths: HashSet<&str> = s.hidden.iter().map(|h| h.value.as_str()).collect();
    let always: HashSet<&str> = s.always_shown.iter().map(|h| h.value.as_str()).collect();
    let (mut hidden, mut shown) = (HashSet::new(), HashSet::new());
    for n in (0..g.node_count() as NodeId).filter(|&n| n != g.root() && !g.flags(n).contains(NodeFlags::EXTERNAL)) {
        let named = matches!(g.kind(n), NodeKind::Module | NodeKind::Package | NodeKind::Type | NodeKind::Member);
        if !named {
            continue;
        }
        let path = (!paths.is_empty() || !always.is_empty()).then(|| block_path(g, n));
        if path.as_deref().is_some_and(|p| paths.contains(p)) || patterns.iter().any(|r| r.is_match(g.name(n))) {
            hidden.insert(n);
        }
        if path.as_deref().is_some_and(|p| always.contains(p)) {
            shown.insert(n);
        }
    }
    (hidden, shown)
}

pub fn view_items(s: &Settings) -> Vec<ViewItem> {
    let item = |v: View| ViewItem { key: Key::View(v), label: v.label().into(), checked: s.view(v).value };
    let in_menu = [View::Tests, View::TestDeps, View::Details, View::Errors, View::Wiring, View::Complexity, View::Unused];
    let mut out: Vec<ViewItem> = in_menu.into_iter().map(item).collect();
    for f in &s.filters {
        let name = &f.value.name;
        out.push(ViewItem { key: Key::FilterShown(name.clone()), label: name.clone(), checked: s.filter_shown(name).is_some_and(|x| x.value) });
    }
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
