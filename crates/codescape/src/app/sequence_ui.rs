use super::Project;
use crate::sequence::{CallIndex, Diagram, Query, diagram};
use crate::style;
use codescape_format::{NodeId, NodeKind};
use codescape_index::review::Status;
use egui::{Align2, Color32, FontId, Pos2, Sense, Shape, Stroke, StrokeKind};
use std::rc::Rc;

const LANE_W: f32 = 190.0;
const HEAD_H: f32 = 46.0;
const ROW_H: f32 = 34.0;
const BAR_W: f32 = 10.0;
const MARGIN: f32 = 24.0;

pub(super) struct SequenceView {
    query: Query,
    diagram: Diagram,
    whole: bool,
}

enum Act {
    Expand(NodeId),
    Choose(NodeId, NodeId),
    Recentre(NodeId),
    Chain(usize, NodeId),
    Code(NodeId, Option<u32>),
    Map(NodeId),
    Whole,
    Close,
}

impl Project {
    fn calls(&mut self) -> Rc<CallIndex> {
        if self.calls.is_none() {
            self.calls = Some(Rc::new(CallIndex::new(self.file.graph())));
        }
        self.calls.clone().unwrap()
    }

    pub(super) fn open_sequence(&mut self, focus: NodeId) {
        let calls = self.calls();
        let g = self.file.graph();
        let focus = match g.kind(focus) {
            NodeKind::Member => focus,
            NodeKind::Type => match calls.lead_method(g, focus) {
                Some(m) => m,
                None => return,
            },
            _ => return,
        };
        let whole = self.sequence.as_ref().is_some_and(|s| s.whole);
        let chain = if whole { calls.default_chain(g, focus) } else { calls.one_level(g, focus) };
        let query = Query::new(focus, chain);
        let diagram = diagram(g, &calls, &query);
        self.sequence = Some(SequenceView { query, diagram, whole });
    }

    fn rebuild_sequence(&mut self) {
        let calls = self.calls();
        if let Some(s) = &mut self.sequence {
            s.diagram = diagram(self.file.graph(), &calls, &s.query);
        }
    }

    fn message_color(&self, target: NodeId) -> Option<Color32> {
        let c = self.review.as_ref()?.review.change(target);
        match c.status {
            Status::New => Some(style::ADDED),
            Status::Changed => Some(style::EDGE_SIDEWAYS),
            Status::Unchanged => None,
        }
    }

    pub(super) fn sequence_ui(&mut self, ui: &mut egui::Ui) {
        let Some(view) = &self.sequence else { return };
        let g = self.file.graph();
        let calls = self.calls.clone().unwrap();
        let mut act: Option<Act> = None;
        let q = &view.query;
        let d = &view.diagram;
        egui::Frame::NONE.fill(style::PANEL).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("SEQUENCE").strong().color(style::BG).background_color(style::BOX_TITLE));
                let chain = if q.chain.last() == Some(&q.focus) { q.chain.clone() } else { vec![q.focus] };
                for (i, &n) in chain.iter().enumerate() {
                    if i > 0 {
                        ui.label(egui::RichText::new("›").color(style::MUTED));
                    }
                    let name = qualified(g, n);
                    if n == q.focus {
                        ui.label(egui::RichText::new(name).strong().color(style::ACCENT));
                        continue;
                    }
                    let others = calls.callers(chain[i + 1]);
                    if others.len() > 1 {
                        egui::ComboBox::from_id_salt(("chain", i)).selected_text(name).show_ui(ui, |ui| {
                            for &c in others {
                                if ui.selectable_label(c == n, qualified(g, c)).clicked() && c != n {
                                    act = Some(Act::Chain(i, c));
                                }
                            }
                        });
                    } else if ui.link(name).on_hover_text("Double-click to tell this method instead").double_clicked() {
                        act = Some(Act::Recentre(n));
                    }
                }
                if chain.len() == 1 && !calls.callers(q.focus).is_empty() {
                    ui.label(egui::RichText::new(format!("({} callers, no entry point found)", calls.callers(q.focus).len())).small().color(style::MUTED));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if view.whole { "One level" } else { "Whole chain" };
                    if ui.button(label).clicked() {
                        act = Some(Act::Whole);
                    }
                    if ui.button("Back to map").on_hover_text("Esc").clicked() {
                        act = Some(Act::Close);
                    }
                    let owner = g.parent(q.focus).filter(|&p| g.kind(p) == NodeKind::Type);
                    let siblings = owner.map(|o| calls.methods(g, o)).unwrap_or_default();
                    if siblings.len() > 1 {
                        egui::ComboBox::from_id_salt("sibling").selected_text(g.name(q.focus)).show_ui(ui, |ui| {
                            for &m in &siblings {
                                if ui.selectable_label(m == q.focus, g.name(m)).clicked() && m != q.focus {
                                    act = Some(Act::Recentre(m));
                                }
                            }
                        });
                        ui.label(egui::RichText::new(format!("method of {}", owner.map_or("", |o| g.name(o)))).small().color(style::MUTED));
                    }
                });
            });
        });
        let lanes = d.lifelines.len().max(1);
        let rows = d.messages.len() + 2;
        let size = egui::vec2(MARGIN * 2.0 + lanes as f32 * LANE_W, HEAD_H + rows as f32 * ROW_H + MARGIN);
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(size.max(ui.available_size()), Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, style::BG);
            let lane_x = |i: usize| rect.left() + MARGIN + i as f32 * LANE_W + LANE_W / 2.0;
            let row_y = |k: usize| rect.top() + HEAD_H + MARGIN + (k as f32 + 0.5) * ROW_H;
            let bottom = row_y(d.messages.len()) + ROW_H / 2.0;
            for (i, l) in d.lifelines.iter().enumerate() {
                let x = lane_x(i);
                painter.extend(Shape::dashed_line(&[Pos2::new(x, rect.top() + HEAD_H), Pos2::new(x, bottom)], Stroke::new(1.0, style::FRAME), 5.0, 4.0));
                let head = egui::Rect::from_center_size(Pos2::new(x, rect.top() + MARGIN / 2.0 + HEAD_H / 2.0), egui::vec2(LANE_W - 24.0, HEAD_H - 8.0));
                let stroke = if g.kind(l.node) == NodeKind::Type { style::kind_stroke(g.sub_kind(l.node)) } else { style::FUNCTION };
                painter.rect_filled(head, 4.0, style::tint(style::BAR, stroke, 0.18));
                painter.rect_stroke(head, 4.0, Stroke::new(1.0, stroke), StrokeKind::Inside);
                if let Some(st) = style::stereotype(g.sub_kind(l.node), g.lang(l.node)) {
                    painter.text(head.center_top() + egui::vec2(0.0, 9.0), Align2::CENTER_CENTER, st, FontId::proportional(9.5), style::MUTED);
                }
                painter.text(
                    head.center() + egui::vec2(0.0, 5.0),
                    Align2::CENTER_CENTER,
                    crate::canvas::fit_label(&l.label, head.width() - 10.0, 13.0),
                    FontId::proportional(13.0),
                    style::BOX_TITLE,
                );
                let hit = ui.interact(head, ui.scope_id().with(("lane", i)), Sense::click());
                if hit.on_hover_text("Double-click: show it on the map").double_clicked() {
                    act = Some(Act::Map(l.node));
                }
            }
            let first_depth = d.messages.first().map_or(0, |m| m.depth);
            let mut open: Vec<(u32, usize, usize, bool)> = Vec::new();
            let mut bars: Vec<(usize, usize, usize, usize, bool)> = Vec::new();
            let root_lane = d.messages.first().map_or(0, |m| m.from);
            open.push((first_depth, root_lane, 0, d.messages.first().is_none_or(|m| m.caller == q.focus)));
            for (k, m) in d.messages.iter().enumerate() {
                while open.last().is_some_and(|o| o.0 > m.depth) {
                    let (_, lane, start, focus) = open.pop().unwrap();
                    let stack = open.iter().filter(|o| o.1 == lane).count();
                    bars.push((lane, start, k, stack, focus));
                }
                open.push((m.depth + 1, m.to, k, m.focus));
            }
            while let Some((_, lane, start, focus)) = open.pop() {
                let stack = open.iter().filter(|o| o.1 == lane).count();
                bars.push((lane, start, d.messages.len(), stack, focus));
            }
            for (lane, start, end, stack, focus) in bars {
                let top = if start == 0 && end > 0 && lane == root_lane && stack == 0 { row_y(0) - ROW_H / 2.0 } else { row_y(start) };
                let bar = egui::Rect::from_min_max(
                    Pos2::new(lane_x(lane) - BAR_W / 2.0 + stack as f32 * 4.0, top),
                    Pos2::new(lane_x(lane) + BAR_W / 2.0 + stack as f32 * 4.0, row_y(end.max(start + 1) - 1) + ROW_H * 0.4),
                );
                painter.rect_filled(bar, 1.0, if focus { style::tint(style::BAR, style::ACCENT, 0.5) } else { style::BAR });
                painter.rect_stroke(bar, 1.0, Stroke::new(1.0, if focus { style::ACCENT } else { style::FRAME }), StrokeKind::Inside);
            }
            for (k, m) in d.messages.iter().enumerate() {
                let y = row_y(k);
                let (x0, x1) = (lane_x(m.from) + BAR_W / 2.0, lane_x(m.to) - BAR_W / 2.0);
                let color = if m.focus { style::ACCENT } else { self.message_color(m.target).unwrap_or(style::TEXT) };
                let width = if m.focus { 2.0 } else { 1.2 };
                let label = if m.create { format!("«create» {}", g.name(m.callee)) } else { format!("{}()", g.name(m.callee)) };

                let (label_rect, hit_rect) = if m.from == m.to {
                    let x = lane_x(m.from) + BAR_W / 2.0;
                    let pts = vec![Pos2::new(x, y - 6.0), Pos2::new(x + 28.0, y - 6.0), Pos2::new(x + 28.0, y + 6.0), Pos2::new(x + 2.0, y + 6.0)];
                    painter.add(Shape::line(pts, Stroke::new(width, color)));
                    arrow_head(&painter, Pos2::new(x + 2.0, y + 6.0), -1.0, color);
                    (
                        painter.text(Pos2::new(x + 34.0, y), Align2::LEFT_CENTER, &label, FontId::proportional(12.0), color),
                        egui::Rect::from_min_size(Pos2::new(x, y - 10.0), egui::vec2(LANE_W - 10.0, 20.0)),
                    )
                } else {
                    let dir = if x1 > x0 { 1.0 } else { -1.0 };
                    let (a, b) = if dir > 0.0 { (x0, x1) } else { (lane_x(m.from) - BAR_W / 2.0, lane_x(m.to) + BAR_W / 2.0) };
                    if m.create {
                        painter.extend(Shape::dashed_line(&[Pos2::new(a, y), Pos2::new(b, y)], Stroke::new(width, color), 6.0, 3.0));
                    } else {
                        painter.line_segment([Pos2::new(a, y), Pos2::new(b, y)], Stroke::new(width, color));
                    }
                    arrow_head(&painter, Pos2::new(b, y), dir, color);
                    let (at, align) =
                        if dir > 0.0 { (Pos2::new(a + 8.0, y - 8.0), Align2::LEFT_CENTER) } else { (Pos2::new(a - 8.0, y - 8.0), Align2::RIGHT_CENTER) };
                    (
                        painter.text(at, align, &label, FontId::proportional(12.0), color),
                        egui::Rect::from_min_max(Pos2::new(a.min(b), y - 16.0), Pos2::new(a.max(b), y + 6.0)),
                    )
                };
                let mut chips_x = label_rect.right() + 8.0;
                let chip = |text: &str, x: f32, fill: Color32| {
                    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(10.5), style::BG);
                    let r = egui::Rect::from_min_size(Pos2::new(x, y - 9.0), galley.size() + egui::vec2(10.0, 4.0));
                    painter.rect_filled(r, 7.0, fill);
                    painter.galley(r.min + egui::vec2(5.0, 2.0), galley, style::BG);
                    r
                };
                if m.on_stack {
                    chips_x = chip("recursive", chips_x, style::MUTED).right() + 4.0;
                }
                if m.more {
                    let r = chip("+ open", chips_x, style::BOX_TITLE);
                    if ui.interact(r, ui.scope_id().with(("more", k)), Sense::click()).on_hover_text("Show what it calls").clicked() {
                        act = Some(Act::Expand(m.target));
                    }
                    chips_x = r.right() + 4.0;
                }
                if m.implementations.len() > 1 || (m.implementations.len() == 1 && m.target == m.callee) {
                    let text = format!("{} implementations", m.implementations.len());
                    let r = chip(&text, chips_x, style::INTERFACE);
                    let resp = ui.interact(r, ui.scope_id().with(("impl", k)), Sense::click());
                    egui::Popup::menu(&resp).show(|ui| {
                        for &i in &m.implementations {
                            let owner = g.parent(i).map_or("", |p| g.name(p));
                            if ui.selectable_label(m.target == i, owner).clicked() {
                                act = Some(Act::Choose(m.callee, i));
                            }
                        }
                    });
                }
                let hit = ui.interact(hit_rect, ui.scope_id().with(("msg", k)), Sense::click());
                let place = g.file(m.caller).map(|f| format!("{f}:{}", m.line.map_or("?".into(), |l| l.to_string()))).unwrap_or_default();
                let hit = hit.on_hover_text(format!("{place}\nClick: open the call in the code · Double-click: tell {} instead", g.name(m.target)));
                if hit.double_clicked() {
                    act = Some(Act::Recentre(m.target));
                } else if hit.clicked() {
                    act = Some(Act::Code(m.caller, m.line));
                }
            }
        });
        match act {
            Some(Act::Expand(n)) => {
                if let Some(s) = &mut self.sequence {
                    s.query.expanded.insert(n);
                }
                self.rebuild_sequence();
            }
            Some(Act::Choose(callee, imp)) => {
                if let Some(s) = &mut self.sequence {
                    s.query.chosen.insert(callee, imp);
                }
                self.rebuild_sequence();
            }
            Some(Act::Recentre(n)) => self.open_sequence(n),
            Some(Act::Chain(i, c)) => {
                let g = self.file.graph();
                if let Some(s) = &mut self.sequence {
                    let mut chain = if s.whole { calls.default_chain(g, c) } else { vec![c] };
                    chain.extend_from_slice(&s.query.chain[i + 1..]);
                    s.query.chain = chain;
                }
                self.rebuild_sequence();
            }
            Some(Act::Code(caller, line)) => {
                self.sequence = None;
                self.open_member_at(caller, line);
            }
            Some(Act::Map(n)) => {
                self.sequence = None;
                self.set_focus(n, None, true, true);
            }
            Some(Act::Whole) => {
                if let Some(s) = &mut self.sequence {
                    s.whole = !s.whole;
                    let focus = s.query.focus;
                    let g = self.file.graph();
                    s.query.chain = if s.whole { calls.default_chain(g, focus) } else { calls.one_level(g, focus) };
                }
                self.rebuild_sequence();
            }
            Some(Act::Close) => self.sequence = None,
            None => {}
        }
    }
}

fn qualified(g: &codescape_format::ArchivedGraph, n: NodeId) -> String {
    match g.parent(n).filter(|&p| g.kind(p) == NodeKind::Type) {
        Some(t) => format!("{}.{}", g.name(t), g.name(n)),
        None => g.name(n).to_string(),
    }
}

fn arrow_head(painter: &egui::Painter, tip: Pos2, dir: f32, color: Color32) {
    let pts = vec![tip, tip + egui::vec2(-8.0 * dir, -4.5), tip + egui::vec2(-8.0 * dir, 4.5)];
    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}
