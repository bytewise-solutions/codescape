use crate::style;
use crate::view::toggle;
use codescape_format::Category;
use codescape_index::{CategoryOverride, Filter, Key, Layer, SettingsStore, Sourced, Threshold, View};
use std::collections::HashMap;

pub struct SettingsForm {
    new_exclude: String,
    new_test: String,
    new_category: String,
    new_category_kind: Category,
    new_filter: String,
    filter_text: HashMap<String, (String, String)>,
    pub error: Option<String>,
}

impl Default for SettingsForm {
    fn default() -> Self {
        SettingsForm {
            new_exclude: String::new(),
            new_test: String::new(),
            new_category: String::new(),
            new_category_kind: Category::DevTool,
            new_filter: String::new(),
            filter_text: HashMap::new(),
            error: None,
        }
    }
}

#[derive(Default)]
pub struct Outcome {
    pub view_changed: bool,
    pub apply: bool,
}

enum Op {
    Toggle(Key, bool),
    Reset(Key),
    Promote(Key, Layer),
    RemoveExclude(String),
    RemoveTest(String),
    RemoveCategory(String),
    RemoveFilter(String),
    AddExclude(String),
    AddTest(String),
    AddCategory(CategoryOverride),
    PutFilter(Filter),
    Pack(String, bool),
    Discard,
}

fn origin(ui: &mut egui::Ui, layer: Layer, key: &Key, ops: &mut Vec<Op>) {
    ui.label(egui::RichText::new(layer.label()).small().color(style::MUTED));
    if layer != Layer::Personal {
        return;
    }
    if ui.small_button("reset").on_hover_text("forget this project's value and use the one below it").clicked() {
        ops.push(Op::Reset(key.clone()));
    }
    if ui.small_button("use for all projects").on_hover_text("move it to ~/.config/codescape/settings.toml").clicked() {
        ops.push(Op::Promote(key.clone(), Layer::User));
    }
    if ui.small_button("save to codescape.toml").on_hover_text("move it to the project's codescape.toml, shared with the team").clicked() {
        ops.push(Op::Promote(key.clone(), Layer::Shared));
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(text).small().color(style::MUTED));
}

fn split(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect()
}

fn add_row(ui: &mut egui::Ui, buf: &mut String, hint: &str) -> Option<String> {
    let mut out = None;
    ui.horizontal(|ui| {
        let edit = ui.add(egui::TextEdit::singleline(buf).hint_text(hint).desired_width(260.0));
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Add").clicked() || enter) && !buf.trim().is_empty() {
            out = Some(std::mem::take(buf).trim().to_string());
        }
    });
    out
}

fn glob_list(ui: &mut egui::Ui, id: &str, items: &[Sourced<String>], key: fn(String) -> Key, remove: fn(String) -> Op, ops: &mut Vec<Op>) {
    egui::Grid::new(id).num_columns(3).striped(true).show(ui, |ui| {
        for e in items {
            ui.monospace(&e.value);
            if ui.small_button("×").on_hover_text("leave this out").clicked() {
                ops.push(remove(e.value.clone()));
            }
            ui.horizontal(|ui| origin(ui, e.layer, &key(e.value.clone()), ops));
            ui.end_row();
        }
    });
}

fn removed_list(ui: &mut egui::Ui, store: &SettingsStore, matches: fn(&Key) -> Option<&str>, ops: &mut Vec<Op>) {
    for r in &store.settings().removed {
        let Some(text) = matches(&r.value) else { continue };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(text).monospace().strikethrough().color(style::MUTED));
            ui.label(egui::RichText::new(format!("removed in {}", r.layer.label())).small().color(style::MUTED));
            if r.layer == Layer::Personal && ui.small_button("reset").clicked() {
                ops.push(Op::Reset(r.value.clone()));
            }
        });
    }
}

pub fn show(ctx: &egui::Context, open: &mut bool, store: &mut SettingsStore, packs: &[String], form: &mut SettingsForm) -> Outcome {
    let mut ops: Vec<Op> = Vec::new();
    let mut apply = false;
    egui::Window::new("Project settings").id(crate::app::windows::Win::Settings.id()).open(open).default_width(660.0).default_height(800.0).vscroll(true).show(
        ctx,
        |ui| {
            let s = store.settings();
            ui.label(egui::RichText::new(format!("This project: {}", store.paths.personal.display())).small().color(style::MUTED));
            if let Some(e) = &form.error {
                ui.colored_label(style::EDGE_UP, e);
            }
            ui.heading("Display");
            ui.label(egui::RichText::new("Applies at once and is remembered for this project.").small().color(style::MUTED));
            egui::Grid::new("display").num_columns(2).show(ui, |ui| {
                for v in View::ALL {
                    let cur = s.view(v);
                    let mut on = cur.value;
                    if ui.checkbox(&mut on, v.label()).changed() {
                        ops.push(Op::Toggle(Key::View(v), on));
                    }
                    ui.horizontal(|ui| origin(ui, cur.layer, &Key::View(v), &mut ops));
                    ui.end_row();
                }
                for f in &s.filters {
                    let name = &f.value.name;
                    let Some(cur) = s.filter_shown(name) else { continue };
                    let mut on = cur.value;
                    if ui.checkbox(&mut on, format!("Filter: {name}")).changed() {
                        ops.push(Op::Toggle(Key::FilterShown(name.clone()), on));
                    }
                    ui.horizontal(|ui| {
                        if s.filter_views.contains_key(name) {
                            origin(ui, cur.layer, &Key::FilterShown(name.clone()), &mut ops);
                        } else {
                            ui.label(egui::RichText::new(format!("filter default ({})", cur.layer.label())).small().color(style::MUTED));
                        }
                    });
                    ui.end_row();
                }
                for t in Threshold::ALL {
                    let cur = s.threshold(t);
                    ui.label(format!("{} {}", t.label(), cur.value));
                    ui.horizontal(|ui| origin(ui, cur.layer, &Key::Threshold(t), &mut ops));
                    ui.end_row();
                }
                for h in &s.hidden {
                    ui.label(format!("Hidden: {}", h.value));
                    ui.horizontal(|ui| origin(ui, h.layer, &Key::Hidden(h.value.clone()), &mut ops));
                    ui.end_row();
                }
                for h in &s.hidden_names {
                    ui.label(format!("Hidden names: {}", h.value));
                    ui.horizontal(|ui| origin(ui, h.layer, &Key::HiddenName(h.value.clone()), &mut ops));
                    ui.end_row();
                }
                for h in &s.hidden_supertypes {
                    ui.label(format!("Hidden types extending: {}", h.value));
                    ui.horizontal(|ui| origin(ui, h.layer, &Key::HiddenSupertype(h.value.clone()), &mut ops));
                    ui.end_row();
                }
                for a in &s.always_shown {
                    ui.label(format!("Always shown: {}", a.value));
                    ui.horizontal(|ui| origin(ui, a.layer, &Key::AlwaysShown(a.value.clone()), &mut ops));
                    ui.end_row();
                }
            });

            ui.separator();
            ui.heading("Index");
            ui.label(egui::RichText::new("Changes here take effect when you apply them, which re-indexes the project.").small().color(style::MUTED));

            heading(ui, "LEFT OUT (glob on the path relative to the project)");
            glob_list(ui, "exclude", &s.exclude, Key::Exclude, Op::RemoveExclude, &mut ops);
            removed_list(ui, store, |k| if let Key::Exclude(g) = k { Some(g) } else { None }, &mut ops);
            if let Some(g) = add_row(ui, &mut form.new_exclude, "**/generated/**") {
                ops.push(Op::AddExclude(g));
            }

            heading(ui, "ALSO TEST CODE (glob), on top of the rule packs' test detection");
            glob_list(ui, "tests", &s.test_paths, Key::TestPath, Op::RemoveTest, &mut ops);
            removed_list(ui, store, |k| if let Key::TestPath(g) = k { Some(g) } else { None }, &mut ops);
            if let Some(g) = add_row(ui, &mut form.new_test, "**/it/**") {
                ops.push(Op::AddTest(g));
            }

            heading(ui, "RULE PACKS");
            egui::Grid::new("packs").num_columns(2).show(ui, |ui| {
                for name in packs {
                    let cur = s.pack(name).unwrap_or(Sourced { value: true, layer: Layer::Default });
                    let mut on = cur.value;
                    if ui.checkbox(&mut on, name).changed() {
                        ops.push(Op::Pack(name.clone(), on));
                    }
                    ui.horizontal(|ui| origin(ui, cur.layer, &Key::Pack(name.clone()), &mut ops));
                    ui.end_row();
                }
            });

            heading(ui, "CATEGORIES (entry points under a glob)");
            egui::Grid::new("categories").num_columns(4).striped(true).show(ui, |ui| {
                for c in &s.categories {
                    ui.monospace(&c.value.glob);
                    ui.label(c.value.category.label());
                    if ui.small_button("×").clicked() {
                        ops.push(Op::RemoveCategory(c.value.glob.clone()));
                    }
                    ui.horizontal(|ui| origin(ui, c.layer, &Key::Category(c.value.glob.clone()), &mut ops));
                    ui.end_row();
                }
            });
            removed_list(ui, store, |k| if let Key::Category(g) = k { Some(g) } else { None }, &mut ops);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut form.new_category).hint_text("**/cmd/migrate/**").desired_width(220.0));
                egui::ComboBox::from_id_salt("new-category").selected_text(form.new_category_kind.label()).show_ui(ui, |ui| {
                    for c in [Category::Application, Category::DevTool, Category::Test] {
                        ui.selectable_value(&mut form.new_category_kind, c, c.label());
                    }
                });
                if ui.button("Add").clicked() && !form.new_category.trim().is_empty() {
                    ops.push(Op::AddCategory(CategoryOverride {
                        glob: std::mem::take(&mut form.new_category).trim().into(),
                        category: form.new_category_kind,
                    }));
                }
            });

            heading(ui, "FILTERS (paths: globs, names: regexes on type and function names, comma separated)");
            for f in &s.filters {
                let fl = &f.value;
                let text = form.filter_text.entry(fl.name.clone()).or_insert_with(|| (fl.paths.join(", "), fl.names.join(", ")));
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(&fl.name);
                        let mut shown = fl.shown;
                        let shown_changed = ui.checkbox(&mut shown, "shown by default").changed();
                        if ui.small_button("×").on_hover_text("remove this filter").clicked() {
                            ops.push(Op::RemoveFilter(fl.name.clone()));
                        }
                        origin(ui, f.layer, &Key::Filter(fl.name.clone()), &mut ops);
                        if shown_changed {
                            ops.push(Op::PutFilter(Filter { shown, ..fl.clone() }));
                        }
                    });
                    let paths = ui.horizontal(|ui| {
                        ui.label("paths");
                        ui.add(egui::TextEdit::singleline(&mut text.0).hint_text("**/migrations/**").desired_width(420.0)).changed()
                    });
                    let names = ui.horizontal(|ui| {
                        ui.label("names");
                        ui.add(egui::TextEdit::singleline(&mut text.1).hint_text("Migration$").desired_width(420.0)).changed()
                    });
                    if paths.inner || names.inner {
                        ops.push(Op::PutFilter(Filter { name: fl.name.clone(), paths: split(&text.0), names: split(&text.1), shown: fl.shown }));
                    }
                });
            }
            removed_list(ui, store, |k| if let Key::Filter(n) = k { Some(n) } else { None }, &mut ops);
            if let Some(name) = add_row(ui, &mut form.new_filter, "new filter name, e.g. migrations") {
                ops.push(Op::PutFilter(Filter { name, paths: Vec::new(), names: Vec::new(), shown: true }));
            }

            ui.separator();
            ui.horizontal(|ui| {
                let dirty = store.index_dirty();
                if ui.add_enabled(dirty, egui::Button::new("Apply and re-index")).clicked() {
                    apply = true;
                }
                if ui.add_enabled(dirty, egui::Button::new("Discard changes")).clicked() {
                    ops.push(Op::Discard);
                }
                if dirty {
                    ui.label(egui::RichText::new("unsaved index changes").color(style::EDGE_SIDEWAYS));
                }
            });
        },
    );
    let mut out = Outcome { apply, ..Default::default() };
    for op in ops {
        let display = match &op {
            Op::Toggle(k, _) | Op::Reset(k) | Op::Promote(k, _) => k.is_display(),
            _ => false,
        };
        let result = match op {
            Op::Toggle(k, on) => {
                toggle(store, &k, on);
                Ok(())
            }
            Op::Reset(k) => store.reset(&k),
            Op::Promote(k, to) => store.promote(&k, to),
            Op::RemoveExclude(g) => store.remove_exclude(&g),
            Op::RemoveTest(g) => store.remove_test_path(&g),
            Op::RemoveCategory(g) => store.remove_category(&g),
            Op::RemoveFilter(n) => {
                form.filter_text.remove(&n);
                store.remove_filter(&n)
            }
            Op::AddExclude(g) => store.add_exclude(&g),
            Op::AddTest(g) => store.add_test_path(&g),
            Op::AddCategory(c) => store.add_category(c),
            Op::PutFilter(f) => store.put_filter(f),
            Op::Pack(n, on) => store.set_pack(&n, on),
            Op::Discard => {
                store.discard();
                form.filter_text.clear();
                Ok(())
            }
        };
        form.error = result.err().map(|e| format!("{e:#}"));
        out.view_changed |= display;
    }
    out
}
