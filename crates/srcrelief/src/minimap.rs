use crate::camera::Camera;
use crate::style;
use egui::{Color32, Painter, Pos2, Stroke, StrokeKind};
use srcrelief_layout::{ContainerLayout, Rect};

pub const SIZE: [f32; 2] = [200.0, 130.0];
const MARGIN: f32 = 12.0;

pub fn area(view: Rect) -> Rect {
    Rect::new(view.right() - SIZE[0] - MARGIN, view.bottom() - SIZE[1] - MARGIN, SIZE[0], SIZE[1])
}

fn scale(root: &ContainerLayout, area: Rect) -> (f32, f32, f32) {
    let s = ((area.w - 8.0) / root.size[0].max(1.0)).min((area.h - 8.0) / root.size[1].max(1.0));
    let dx = area.x + (area.w - root.size[0] * s) / 2.0;
    let dy = area.y + (area.h - root.size[1] * s) / 2.0;
    (s, dx, dy)
}

fn erect(r: Rect) -> egui::Rect {
    egui::Rect::from_min_size(Pos2::new(r.x, r.y), egui::vec2(r.w, r.h))
}

pub fn draw(painter: &Painter, root: &ContainerLayout, cam: &Camera, view: Rect) {
    let a = area(view);
    painter.rect_filled(erect(a), 4.0, style::BAR);
    painter.rect_stroke(erect(a), 4.0, Stroke::new(1.0, Color32::from_rgb(58, 62, 71)), StrokeKind::Inside);
    let (s, dx, dy) = scale(root, a);
    for c in &root.children {
        let r = Rect::new(dx + c.rect.x * s, dy + c.rect.y * s, (c.rect.w * s).max(1.0), (c.rect.h * s).max(1.0));
        painter.rect_filled(erect(r), 1.0, style::BOX_STROKE.gamma_multiply(0.5));
    }
    let world = cam.screen_to_world_rect(view, view);
    let v = Rect::new(dx + world.x * s, dy + world.y * s, world.w * s, world.h * s);
    let clipped = painter.with_clip_rect(erect(a));
    clipped.rect_stroke(erect(v), 0.0, Stroke::new(1.5, style::ACCENT), StrokeKind::Inside);
}

pub fn world_at(root: &ContainerLayout, view: Rect, p: [f32; 2]) -> Option<[f32; 2]> {
    let a = area(view);
    if !a.contains(p) {
        return None;
    }
    let (s, dx, dy) = scale(root, a);
    Some([(p[0] - dx) / s, (p[1] - dy) / s])
}
