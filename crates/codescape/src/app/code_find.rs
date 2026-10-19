use super::Project;
use crate::find::{Hit, find_in, hit_from, step_hit};
use crate::search::{file_structure, filter_structure, step_selection};
use crate::source::advance;
use crate::style;
use codescape_format::{NodeId, NodeKind};
use egui::{Align2, Key};

#[derive(Default)]
pub(super) struct FindBar {
    node: Option<NodeId>,
    text: String,
    hits: Vec<Hit>,
    current: Option<usize>,
    focus: bool,
}

pub(super) struct Structure {
    owner: NodeId,
    all: Vec<NodeId>,
    text: String,
    shown: Vec<NodeId>,
    cursor: usize,
}

impl Project {
    fn is_open_file(&self, n: NodeId) -> bool {
        self.scene.boxes.iter().any(|b| b.node == n && b.source)
    }

    fn active_file(&self) -> Option<NodeId> {
        let g = self.file.graph();
        self.pointer.and_then(|p| self.file_page_at(p)).or(self.selected.and_then(|s| g.path(s).into_iter().find(|&p| self.is_open_file(p)))).or(self
            .scene
            .boxes
            .iter()
            .find(|b| b.source)
            .map(|b| b.node))
    }

    fn file_owner(&self, n: NodeId) -> Option<NodeId> {
        let g = self.file.graph();
        let file = g.file(n)?;
        g.path(n).into_iter().find(|&p| g.file(p) == Some(file) && matches!(g.kind(p), NodeKind::Type | NodeKind::Member))
    }

    pub(super) fn open_find(&mut self) {
        let node = self.active_file();
        let bar = self.find.get_or_insert_with(FindBar::default);
        bar.focus = true;
        if bar.node != node {
            bar.node = node;
            self.refresh_find();
        }
    }

    pub(super) fn start_find(&mut self, text: &str) {
        self.find = Some(FindBar { text: text.to_string(), ..FindBar::default() });
    }

    fn refresh_find(&mut self) {
        let g = self.file.graph();
        let Some(bar) = self.find.as_mut() else { return };
        let lines = bar.node.and_then(|n| self.source.borrow_mut().file(g.file(n)?));
        bar.hits = lines.map(|l| find_in(&l, &bar.text)).unwrap_or_default();
        let top = bar.node.and_then(|n| self.file_scroll.get(&n)).map_or(0, |s| s.max(0.0) as usize);
        bar.current = hit_from(&bar.hits, top);
        self.show_hit();
    }

    fn show_hit(&mut self) {
        let Some((node, hit)) = self.find.as_ref().and_then(|b| Some((b.node?, *b.hits.get(b.current?)?))) else { return };
        if let Some((_, _, visible)) = self.file_metrics(node) {
            let top = self.file_scroll.get(&node).copied().unwrap_or(0.0);
            let line = hit.line as f32;
            if line < top || line > top + visible as f32 - 3.0 {
                self.file_scroll.insert(node, (line - 4.0).max(0.0));
            }
        }
        let Some(b) = self.scene.boxes.iter().find(|b| b.node == node && b.source) else { return };
        let Some(size) = self.page_font(b) else { return };
        let total = self.file_metrics(node).map_or(0, |m| m.1);
        let width = self.file_text_area(b, size, total).width();
        let (start, end) = (hit.col as f32 * advance(size), (hit.col + hit.len) as f32 * advance(size));
        let h = self.file_hscroll.get(&node).copied().unwrap_or(0.0);
        if start < h || end > h + width {
            self.file_hscroll.insert(node, (start - width / 3.0).max(0.0));
        }
    }

    pub(super) fn step_find(&mut self, forward: bool) {
        let Some(bar) = self.find.as_mut() else { return };
        bar.current = step_hit(bar.hits.len(), bar.current, forward);
        self.show_hit();
    }

    pub(super) fn find_highlights(&self, node: NodeId) -> Option<(&[Hit], Option<usize>)> {
        self.find.as_ref().filter(|f| f.node == Some(node)).map(|f| (f.hits.as_slice(), f.current))
    }

    pub(super) fn find_bar(&mut self, ctx: &egui::Context) {
        if self.find.as_ref().is_some_and(|f| !f.node.is_some_and(|n| self.is_open_file(n))) {
            let next = self.active_file();
            if let Some(bar) = self.find.as_mut().filter(|f| f.node != next) {
                bar.node = next;
                self.refresh_find();
            }
        }
        let Some(bar) = self.find.as_mut() else { return };
        let (mut open, mut changed, mut step) = (true, false, None);
        egui::Window::new("Find in file")
            .id(super::windows::Win::Find.id())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::RIGHT_TOP, [-20.0, 70.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let edit = ui.add(egui::TextEdit::singleline(&mut bar.text).hint_text("text to find").desired_width(220.0));
                    if std::mem::take(&mut bar.focus) {
                        edit.request_focus();
                    }
                    changed = edit.changed();
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        step = Some(!ui.input(|i| i.modifiers.shift));
                        edit.request_focus();
                    }
                    let count = match (bar.node, bar.current) {
                        (None, _) => "no file open".to_string(),
                        (_, Some(c)) => format!("{}/{}", c + 1, bar.hits.len()),
                        _ if bar.text.is_empty() => String::new(),
                        _ => "no match".to_string(),
                    };
                    ui.label(egui::RichText::new(count).small().color(style::MUTED));
                    if ui.small_button("Prev").on_hover_text("Shift+Enter").clicked() {
                        step = Some(false);
                    }
                    if ui.small_button("Next").on_hover_text("Enter").clicked() {
                        step = Some(true);
                    }
                });
            });
        if let Some(forward) = step {
            self.step_find(forward);
        }
        if changed {
            self.refresh_find();
        }
        if !open {
            self.find = None;
        }
    }

    pub(super) fn open_structure(&mut self) {
        let Some(owner) = self.active_file().or(self.selected.and_then(|s| self.file_owner(s))) else {
            self.search_open = true;
            return;
        };
        let all = file_structure(self.file.graph(), owner);
        self.structure = Some(Structure { owner, shown: all.clone(), all, text: String::new(), cursor: 0 });
    }

    fn go_to_member(&mut self, owner: NodeId, n: NodeId) {
        if self.is_open_file(owner) {
            let line = self.file.graph().span(n).0;
            self.file_scroll.insert(owner, line.saturating_sub(4) as f32);
            self.file_hscroll.insert(owner, 0.0);
            self.set_focus(n, None, false, true);
        } else {
            self.open_member(n);
        }
    }

    pub(super) fn structure_window(&mut self, ctx: &egui::Context) {
        let g = self.file.graph();
        let Some(st) = self.structure.as_mut() else { return };
        let (mut open, mut pick) = (true, None);
        let title = g.file(st.owner).and_then(|f| f.rsplit('/').next()).unwrap_or(g.name(st.owner)).to_string();
        egui::Window::new(format!("Structure of {title}"))
            .id(super::windows::Win::Structure.id())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, [0.0, 60.0])
            .show(ctx, |ui| {
                let (down, up) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, Key::ArrowDown), i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)));
                let moved = down || up;
                if moved {
                    st.cursor = step_selection(st.cursor, st.shown.len(), down);
                }
                let edit = ui.add(egui::TextEdit::singleline(&mut st.text).hint_text("method, field or type name").desired_width(380.0));
                edit.request_focus();
                if edit.changed() {
                    st.shown = filter_structure(g, &st.all, &st.text);
                    st.cursor = 0;
                }
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    pick = st.shown.get(st.cursor).copied();
                }
                let depth = g.path(st.owner).len();
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    for (i, &n) in st.shown.iter().enumerate() {
                        let indent = if st.text.trim().is_empty() { g.path(n).len().saturating_sub(depth + 1) } else { 0 };
                        let row = ui
                            .horizontal(|ui| {
                                ui.add_space(14.0 * indent as f32);
                                let name = egui::RichText::new(g.name(n)).color(if g.kind(n) == NodeKind::Type { style::BOX_TITLE } else { style::TEXT });
                                let row = ui.selectable_label(i == st.cursor, name);
                                ui.label(egui::RichText::new(format!(":{}", g.span(n).0)).small().color(style::MUTED));
                                row
                            })
                            .inner;
                        if moved && i == st.cursor {
                            row.scroll_to_me(None);
                        }
                        if row.clicked() {
                            pick = Some(n);
                        }
                    }
                });
            });
        let owner = st.owner;
        if let Some(n) = pick {
            self.structure = None;
            self.go_to_member(owner, n);
        } else if !open {
            self.structure = None;
        }
    }
}
