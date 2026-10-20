use super::Project;
use crate::keys::{Action, Capture, Keymap, capture, format, held};
use crate::style;
use egui::Align2;
use std::path::PathBuf;

fn keys_file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("codescape").join("keys.toml"))
}

pub(super) fn load_keys() -> Keymap {
    keys_file().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| Keymap::from_toml(&s)).unwrap_or_default()
}

fn save_keys(keys: &Keymap) -> Result<(), String> {
    let path = keys_file().ok_or("no config folder")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, keys.to_toml()).map_err(|e| format!("{}: {e}", path.display()))
}

pub(super) fn shown(keys: &Keymap, a: Action) -> String {
    keys.get(a).map(|s| format(&s)).unwrap_or_default()
}

pub(super) fn take_pressed(ctx: &egui::Context, keys: &Keymap) -> Vec<Action> {
    ctx.input_mut(|i| keys.check_order().into_iter().filter(|&a| keys.get(a).is_some_and(|s| i.consume_shortcut(&s))).collect())
}

impl Project {
    pub(super) fn capture_key(&mut self, ctx: &egui::Context) -> bool {
        let Some(action) = self.capturing else { return false };
        let result = ctx.input_mut(|i| {
            let result = capture(&i.events);
            i.events.retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
            result
        });
        match result {
            Capture::Cancelled => self.capturing = None,
            Capture::Bound(shortcut) => {
                let taken = self.keys.bind(action, shortcut);
                self.keys_note = taken.map(|o| format!("{} was on {}; it has no key now", o.label(), shown(&self.keys, action)));
                self.capturing = None;
                if let Err(e) = save_keys(&self.keys) {
                    self.keys_note = Some(e);
                }
            }
            Capture::Waiting => {}
        }
        true
    }

    pub(super) fn keys_window(&mut self, ctx: &egui::Context) {
        if !self.keys_open {
            return;
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new("Keyboard shortcuts")
            .id(super::windows::Win::Keys.id())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, [0.0, 60.0])
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("Click a shortcut, then press the new keys (Esc cancels). Saved for every project.").small().color(style::MUTED));
                ui.add_space(6.0);
                egui::Grid::new("keys").num_columns(3).spacing([14.0, 6.0]).show(ui, |ui| {
                    for a in Action::ALL {
                        ui.label(a.label());
                        let text = if self.capturing == Some(a) {
                            ui.input(|i| if i.modifiers.is_none() { "press keys…".to_string() } else { held(i.modifiers) })
                        } else {
                            Some(shown(&self.keys, a)).filter(|s| !s.is_empty()).unwrap_or_else(|| "none".to_string())
                        };
                        if ui.add(egui::Button::new(egui::RichText::new(text).monospace().color(style::ACCENT)).min_size(egui::vec2(130.0, 0.0))).clicked() {
                            self.capturing = Some(a);
                            self.keys_note = None;
                        }
                        ui.horizontal(|ui| {
                            if !self.keys.is_default(a) && ui.small_button("reset").on_hover_text(format(&a.default_key())).clicked() {
                                let taken = self.keys.reset(a);
                                self.keys_note = taken.map(|o| format!("{} was on {}; it has no key now", o.label(), shown(&self.keys, a)));
                                changed = true;
                            }
                            if self.keys.get(a).is_some() && ui.small_button("remove").clicked() {
                                self.keys.unbind(a);
                                changed = true;
                            }
                        });
                        ui.end_row();
                    }
                });
                if let Some(note) = &self.keys_note {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(note).small().color(style::ACCENT));
                }
            });
        if changed && let Err(e) = save_keys(&self.keys) {
            self.keys_note = Some(e);
        }
        if !open {
            self.keys_open = false;
            self.capturing = None;
        }
    }
}
