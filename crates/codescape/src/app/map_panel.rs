use super::Project;
use crate::style;
use crate::view::{block_path, name_suffix, short_path, supertypes};
use codescape_format::{ArchivedGraph, NodeFlags, NodeId, NodeKind};
use codescape_index::{Key as SettingKey, Layer, Threshold, View};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum HideRequest {
    Block(String),
    Names(String),
    Supertypes(String),
    ShowAgain(String),
    AlwaysShow(String),
    StopAlwaysShowing(String),
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FlagCounts {
    tests: usize,
    generated: usize,
    errors: usize,
    logging: usize,
    wiring: usize,
    details: usize,
}

fn layer_name(l: Layer) -> &'static str {
    match l {
        Layer::Default => "built in",
        Layer::User => "all projects",
        Layer::Shared => "codescape.toml",
        Layer::Personal => "yours",
    }
}

pub(super) fn hide_menu(g: &ArchivedGraph, ui: &mut egui::Ui, n: NodeId) -> Option<HideRequest> {
    let mut pick = None;
    ui.separator();
    if ui.button("Hide this").clicked() {
        pick = Some(HideRequest::Block(block_path(g, n)));
    }
    let package = g.path(n).into_iter().rev().skip(1).find(|&p| g.kind(p) == NodeKind::Package);
    if let Some(p) = package
        && ui.button(format!("Hide package {}", g.name(p))).clicked()
    {
        pick = Some(HideRequest::Block(block_path(g, p)));
    }
    if let Some(suffix) = name_suffix(g.name(n))
        && ui.button(format!("Hide every name ending in {suffix}")).clicked()
    {
        pick = Some(HideRequest::Names(format!("{suffix}$")));
    }
    if g.kind(n) == NodeKind::Type {
        let simple = |name: &str| name.rsplit('.').next().unwrap_or(name).to_string();
        let exact = |name: &str| format!("(^|\\.){}$", regex::escape(&simple(name)));
        let sups = supertypes(g, n);
        let mut offers: Vec<(String, String)> = vec![(format!("Hide every type extending {}", g.name(n)), exact(g.name(n)))];
        for sup in sups.iter().take(2) {
            offers.push((format!("Hide every type extending {}", simple(sup)), exact(sup)));
        }
        if let Some(family) = sups.first().and_then(|s| name_suffix(&simple(s)).map(str::to_string)) {
            offers.push((format!("Hide every type extending …{family}"), format!("{family}$")));
        }
        for (label, pattern) in offers {
            if ui.button(label).clicked() {
                pick = Some(HideRequest::Supertypes(pattern));
            }
        }
    }
    pick
}

impl Project {
    fn flag_counts(&mut self) -> FlagCounts {
        if let Some(c) = self.flag_counts {
            return c;
        }
        let g = self.file.graph();
        let mut c = FlagCounts::default();
        for n in 0..g.node_count() as NodeId {
            let counted = g.kind(n) == NodeKind::Type || (g.kind(n) == NodeKind::Member && g.parent(n).is_some_and(|p| g.kind(p) == NodeKind::Package));
            if !counted {
                continue;
            }
            let f = g.flags(n);
            c.tests += usize::from(f.intersects(NodeFlags::TEST | NodeFlags::MOCK));
            c.generated += usize::from(f.contains(NodeFlags::GENERATED));
            c.errors += usize::from(f.contains(NodeFlags::NOISE));
            c.logging += usize::from(f.contains(NodeFlags::LOGGING));
            c.wiring += usize::from(f.contains(NodeFlags::WIRING));
            c.details += usize::from(f.contains(NodeFlags::DETAIL));
        }
        self.flag_counts = Some(c);
        c
    }

    pub(super) fn apply_hide(&mut self, request: HideRequest) {
        let done = match &request {
            HideRequest::Block(p) => self.settings.hide(p),
            HideRequest::Names(p) => self.settings.hide_names(p),
            HideRequest::Supertypes(p) => self.settings.hide_supertypes(p),
            HideRequest::ShowAgain(p) => self.settings.unhide(p),
            HideRequest::AlwaysShow(p) => self.settings.always_show(p),
            HideRequest::StopAlwaysShowing(p) => self.settings.stop_always_showing(p),
        };
        if let Err(e) = done {
            self.form.error = Some(format!("{e:#}"));
            return;
        }
        self.apply_display();
        if let Some(n) = self.selected
            && self.cache.options().hides(self.file.graph(), n)
        {
            self.selected = None;
            self.clear_focus();
        }
    }

    pub(super) fn map_panel(&mut self, ctx: &egui::Context) {
        if !self.map_panel_open {
            return;
        }
        let counts = self.flag_counts();
        let mut open = true;
        let mut switches: Vec<(SettingKey, bool)> = Vec::new();
        let mut filter_switches: Vec<(SettingKey, bool)> = Vec::new();
        let mut levels: Vec<(Threshold, u32)> = Vec::new();
        let mut request: Option<HideRequest> = None;
        let mut pattern = std::mem::take(&mut self.hide_pattern);
        egui::Window::new("What's on the map")
            .id(super::windows::Win::MapPanel.id())
            .open(&mut open)
            .auto_sized()
            .anchor(egui::Align2::RIGHT_TOP, [-20.0, 70.0])
            .show(ctx, |ui| {
                let s = self.settings.settings();
                let mut switch = |ui: &mut egui::Ui, v: View, count: Option<usize>| {
                    let mut on = s.view(v).value;
                    let label = match count {
                        Some(n) => format!("{}  ·  {n}", v.label()),
                        None => v.label().to_string(),
                    };
                    if ui.checkbox(&mut on, label).changed() {
                        switches.push((SettingKey::View(v), on));
                    }
                };
                let heading = |ui: &mut egui::Ui, text: &str| {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(text).small().strong().color(style::MUTED));
                };
                egui::ScrollArea::vertical().max_height(ctx.content_rect().height() - 140.0).show(ui, |ui| {
                    heading(ui, "CODE");
                    switch(ui, View::Tests, Some(counts.tests));
                    switch(ui, View::TestDeps, None);
                    switch(ui, View::Generated, Some(counts.generated));
                    switch(ui, View::Errors, Some(counts.errors));
                    switch(ui, View::Logging, Some(counts.logging));
                    switch(ui, View::Wiring, Some(counts.wiring));
                    switch(ui, View::Details, Some(counts.details));
                    heading(ui, "CARDS");
                    for v in [View::PrivateMembers, View::Fields, View::Accessors, View::Constructors] {
                        switch(ui, v, None);
                    }
                    heading(ui, "ARROWS");
                    for v in [View::QuietArrows, View::GuessedCalls, View::ImplementsArrows, View::ImportArrows] {
                        switch(ui, v, None);
                    }
                    heading(ui, "HINTS");
                    switch(ui, View::Complexity, None);
                    switch(ui, View::Unused, None);
                    for t in Threshold::ALL {
                        ui.horizontal(|ui| {
                            let mut value = s.threshold(t).value;
                            ui.label(t.label());
                            if ui.add(egui::DragValue::new(&mut value).range(1..=999)).changed() {
                                levels.push((t, value));
                            }
                        });
                    }
                    if !s.filters.is_empty() {
                        heading(ui, "FILTERS");
                        for f in &s.filters {
                            let mut on = s.filter_shown(&f.value.name).is_some_and(|x| x.value);
                            if ui.checkbox(&mut on, &f.value.name).changed() {
                                filter_switches.push((SettingKey::FilterShown(f.value.name.clone()), on));
                            }
                        }
                    }
                    heading(ui, "HIDDEN BY YOU");
                    let hidden = s
                        .hidden
                        .iter()
                        .map(|h| (h.value.clone(), h.layer, None))
                        .chain(s.hidden_names.iter().map(|h| (h.value.clone(), h.layer, Some("names matching"))))
                        .chain(s.hidden_supertypes.iter().map(|h| (h.value.clone(), h.layer, Some("types extending"))));
                    let mut any = false;
                    for (entry, layer, kind) in hidden {
                        any = true;
                        ui.horizontal(|ui| {
                            let text = match kind {
                                Some(k) => format!("{k} {entry}"),
                                None => short_path(&entry),
                            };
                            ui.label(egui::RichText::new(text).monospace()).on_hover_text(&entry);
                            ui.label(egui::RichText::new(layer_name(layer)).small().color(style::MUTED));
                            if ui.small_button("show again").clicked() {
                                request = Some(HideRequest::ShowAgain(entry.clone()));
                            }
                        });
                    }
                    if !any {
                        ui.label(egui::RichText::new("Nothing yet: right-click a box, then Hide this").small().color(style::MUTED));
                    }
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut pattern).hint_text("Exception$, ^Mock, Dto$…").desired_width(180.0));
                        if ui.button("Hide names").on_hover_text("Every block whose name matches").clicked() && !pattern.trim().is_empty() {
                            request = Some(HideRequest::Names(pattern.trim().to_string()));
                        }
                        if ui.button("Hide types extending").on_hover_text("Every type with a known supertype whose name matches").clicked()
                            && !pattern.trim().is_empty()
                        {
                            request = Some(HideRequest::Supertypes(pattern.trim().to_string()));
                        }
                    });
                    if !s.always_shown.is_empty() {
                        heading(ui, "ALWAYS SHOWN");
                        for a in &s.always_shown {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(short_path(&a.value)).monospace()).on_hover_text(&a.value);
                                if ui.small_button("stop").clicked() {
                                    request = Some(HideRequest::StopAlwaysShowing(a.value.clone()));
                                }
                            });
                        }
                    }
                });
            });
        self.hide_pattern = if matches!(request, Some(HideRequest::Names(_) | HideRequest::Supertypes(_))) { String::new() } else { pattern };
        self.map_panel_open = open;
        for (key, on) in switches.into_iter().chain(filter_switches) {
            self.set_display(&key, on);
        }
        if !levels.is_empty() {
            for (t, v) in levels {
                self.settings.set_threshold(t, v);
            }
            self.apply_display();
        }
        if let Some(r) = request {
            self.apply_hide(r);
        }
    }
}
