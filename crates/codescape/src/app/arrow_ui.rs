use super::Project;
use crate::links::{CallLink, calls_between};
use crate::style;
use codescape_format::NodeId;
use egui::Align2;

pub(super) struct ArrowWin {
    a: NodeId,
    b: NodeId,
    links: Vec<CallLink>,
}

impl Project {
    pub(super) fn open_arrow(&mut self, a: NodeId, b: NodeId) {
        let links = calls_between(self.file.graph(), a, b, self.cache.options());
        self.arrow = Some(ArrowWin { a, b, links });
    }

    pub(super) fn arrow_window(&mut self, ctx: &egui::Context) {
        let Some(w) = &self.arrow else { return };
        let g = self.file.graph();
        let mut open = true;
        let mut open_at: Option<CallLink> = None;
        let mut go: Option<NodeId> = None;
        egui::Window::new(format!("{} → {}", g.name(w.a), g.name(w.b)))
            .id(super::windows::Win::Arrow.id())
            .open(&mut open)
            .collapsible(false)
            .default_width(380.0)
            .anchor(Align2::CENTER_TOP, [0.0, 80.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(format!("Go to {}", g.name(w.a))).clicked() {
                        go = Some(w.a);
                    }
                    if ui.button(format!("Go to {}", g.name(w.b))).clicked() {
                        go = Some(w.b);
                    }
                });
                ui.add_space(4.0);
                if w.links.is_empty() {
                    ui.label(egui::RichText::new("No calls to list; this arrow shows another kind of dependency.").small().color(style::MUTED));
                }
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for l in &w.links {
                        let line = l.line.map(|n| format!("  :{n}")).unwrap_or_default();
                        let text = format!("{} → {}{line}", g.name(l.from), g.name(l.to));
                        if ui.add(egui::Button::new(text).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            open_at = Some(*l);
                        }
                    }
                });
            });
        if !open {
            self.arrow = None;
        }
        if let Some(n) = go {
            self.set_focus(n, None, true, true);
        }
        if let Some(l) = open_at {
            self.open_member_at(l.from, l.line);
        }
    }
}
