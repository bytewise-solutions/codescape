use super::Project;
use crate::insights::{Flag, FlagKind, Insights, InsightsKey, Row, compute, flags};
use crate::style;
use codescape_format::{ArchivedGraph, NodeId, NodeKind};
use codescape_index::churn::file_changes;
use codescape_index::review::is_git;
use egui::{CollapsingHeader, RichText};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

pub(super) struct ChurnState {
    days: u32,
    data: Option<Option<Arc<HashMap<String, u32>>>>,
    job: Option<Receiver<Option<HashMap<String, u32>>>>,
}

impl ChurnState {
    pub(super) fn new() -> Self {
        ChurnState { days: 0, data: None, job: None }
    }
}

struct Section {
    title: &'static str,
    kinds: &'static [FlagKind],
    rows: fn(&Insights) -> &[Row],
}

const SECTIONS: [Section; 7] = [
    Section { title: "Most complex", kinds: &[FlagKind::Complex], rows: |i| &i.complex },
    Section { title: "Hotspots", kinds: &[FlagKind::Hotspot], rows: |i| &i.hotspots },
    Section { title: "Most depended on", kinds: &[], rows: |i| &i.depended },
    Section { title: "Upward dependencies", kinds: &[FlagKind::Upward], rows: |i| &i.upward },
    Section { title: "Dead code", kinds: &[FlagKind::Unused, FlagKind::NoCaller, FlagKind::OnlyTests], rows: |i| &i.dead },
    Section { title: "Untested public code", kinds: &[FlagKind::Untested], rows: |i| &i.untested },
    Section { title: "Duplicates", kinds: &[FlagKind::Duplicate], rows: |i| &i.duplicates },
];

fn flag_color(kind: FlagKind) -> egui::Color32 {
    match kind {
        FlagKind::Complex | FlagKind::Upward | FlagKind::Unused | FlagKind::NoCaller => style::EDGE_UP,
        FlagKind::Hotspot | FlagKind::OnlyTests | FlagKind::Untested | FlagKind::Duplicate => style::ACCENT,
    }
}

pub(super) fn flags_ui(ui: &mut egui::Ui, g: &ArchivedGraph, flags: &[Flag]) -> Option<NodeId> {
    let mut go = None;
    if flags.is_empty() {
        return go;
    }
    ui.add_space(8.0);
    ui.label(RichText::new("FLAGS").small().strong().color(style::MUTED));
    for f in flags {
        ui.label(RichText::new(&f.short).color(flag_color(f.kind)));
        ui.label(RichText::new(&f.explanation).small().color(style::MUTED));
        for &l in &f.links {
            let name = match g.parent(l).filter(|&p| g.kind(l) == NodeKind::Member && g.kind(p) == NodeKind::Type) {
                Some(t) => format!("{}.{}", g.name(t), g.name(l)),
                None => g.name(l).to_string(),
            };
            let name = match g.path(l).get(1) {
                Some(&module) if module != l => format!("{name}  ({})", g.name(module)),
                _ => name,
            };
            if ui.selectable_label(false, name).on_hover_text(super::full_path(g, l)).clicked() {
                go = Some(l);
            }
        }
    }
    go
}

pub(super) struct Picked {
    pub node: Option<NodeId>,
    pub selection: Option<bool>,
}

impl Project {
    pub(super) fn poll_churn(&mut self, ctx: &egui::Context) {
        let days = self.limits().hotspot_days;
        if self.review.is_some() {
            return;
        }
        if self.churn.days != days {
            let (tx, rx) = channel();
            let dir = self.dir.clone();
            std::thread::spawn(move || {
                let _ = tx.send(if is_git(&dir) { file_changes(&dir, days).ok() } else { None });
            });
            self.churn = ChurnState { days, data: None, job: Some(rx) };
        }
        let Some(rx) = &self.churn.job else { return };
        match rx.try_recv() {
            Ok(found) => {
                self.churn.data = Some(found.map(Arc::new));
                self.churn.job = None;
            }
            Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            Err(TryRecvError::Disconnected) => {
                self.churn.data = Some(None);
                self.churn.job = None;
            }
        }
    }

    fn insights_scope(&self) -> Option<NodeId> {
        self.insights_selection.then_some(self.selected).flatten()
    }

    pub(super) fn refresh_insights(&mut self) {
        let Some(analysis) = self.analysis.clone() else { return };
        let churn = self.churn.data.clone().flatten();
        let key = InsightsKey {
            analysis: Arc::as_ptr(&analysis) as usize,
            churn: churn.as_ref().map_or(0, |c| Arc::as_ptr(c) as usize),
            opts: self.cache.options().clone(),
            limits: self.limits(),
            scope: self.insights_scope(),
        };
        let (g, limits, scope) = (self.file.graph(), key.limits, key.scope);
        let opts = key.opts.clone();
        self.insights.get(key, || compute(g, &analysis, churn.as_deref().unwrap_or(&HashMap::new()), &opts, &limits, scope));
    }

    pub(super) fn refresh_flags(&mut self) {
        let (Some(analysis), Some(n)) = (self.analysis.clone(), self.selected) else { return };
        let churn = self.churn.data.clone().flatten();
        let key = InsightsKey {
            analysis: Arc::as_ptr(&analysis) as usize,
            churn: churn.as_ref().map_or(0, |c| Arc::as_ptr(c) as usize),
            opts: self.cache.options().clone(),
            limits: self.limits(),
            scope: Some(n),
        };
        let (g, limits, opts) = (self.file.graph(), key.limits, key.opts.clone());
        self.flags.get(key, || flags(g, &analysis, churn.as_deref().unwrap_or(&HashMap::new()), &opts, &limits, n));
    }

    pub(super) fn insights_ui(&self, ui: &mut egui::Ui) -> Picked {
        let mut picked = Picked { node: None, selection: None };
        ui.add_space(12.0);
        ui.label(RichText::new("INSIGHTS").small().color(style::MUTED));
        let mut selection = self.insights_selection;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut selection, false, "Whole project");
            let scope = self.selected.map_or("Selection".to_string(), |s| format!("In {}", self.file.graph().name(s)));
            ui.selectable_value(&mut selection, true, scope).on_hover_text("Only what is inside the selected block");
        });
        if selection != self.insights_selection {
            picked.selection = Some(selection);
        }
        let Some(insights) = self.insights.current() else {
            ui.label(RichText::new("analysing…").small().color(style::MUTED));
            return picked;
        };
        let g = self.file.graph();
        for (si, section) in SECTIONS.iter().enumerate() {
            let rows = (section.rows)(insights);
            let hotspots = section.kinds == [FlagKind::Hotspot];
            let note = match (&self.churn.data, hotspots) {
                (None, true) => Some("loading…"),
                (Some(None), true) => Some("needs git"),
                _ => None,
            };
            let count = if note.is_some() { String::new() } else { format!(" ({})", rows.len()) };
            CollapsingHeader::new(format!("{}{count}", section.title)).id_salt(("insight", si)).default_open(si == 0).show(ui, |ui| {
                if let Some(note) = note {
                    ui.label(RichText::new(note).small().color(style::MUTED));
                    return;
                }
                if rows.is_empty() {
                    ui.label(RichText::new("nothing found").small().color(style::MUTED));
                }
                for row in rows {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let name = match g.parent(row.node).filter(|&p| g.kind(row.node) == NodeKind::Member && g.kind(p) == NodeKind::Type) {
                            Some(t) => format!("{}.{}", g.name(t), g.name(row.node)),
                            None => g.name(row.node).to_string(),
                        };
                        let hover = self.row_hover(section, row);
                        if ui.selectable_label(self.selected == Some(row.node), name).on_hover_text(hover).clicked() {
                            picked.node = Some(row.node);
                        }
                        ui.horizontal(|ui| {
                            ui.add_space(6.0);
                            ui.label(RichText::new(&row.reason).small().color(style::MUTED));
                        });
                    });
                }
            });
        }
        picked
    }

    fn row_hover(&self, section: &Section, row: &Row) -> String {
        let g = self.file.graph();
        let path = super::full_path(g, row.node);
        let Some(analysis) = &self.analysis else { return path };
        let churn = self.churn.data.clone().flatten().unwrap_or_default();
        let explained: Vec<String> = flags(g, analysis, &churn, self.cache.options(), &self.limits(), row.node)
            .into_iter()
            .filter(|f| section.kinds.contains(&f.kind))
            .map(|f| f.explanation)
            .collect();
        if explained.is_empty() { format!("{path}\n{}", row.reason) } else { format!("{path}\n{}", explained.join("\n")) }
    }

    pub(super) fn go_to_insight(&mut self, n: NodeId) {
        if self.file.graph().kind(n) == NodeKind::Member {
            self.open_member_at(n, None);
        } else {
            self.set_focus(n, None, true, true);
        }
    }
}
