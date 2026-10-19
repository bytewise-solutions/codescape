use crate::canvas::{FILE_OPEN_PX, Scene};
use codescape_format::NodeId;
use codescape_layout::Rect;

pub const READING_OPEN_PT: f32 = 13.0;
pub const READING_MIN_PT: f32 = 6.5;
pub const READING_MAX_PT: f32 = 32.0;

pub fn zoom_text(font: f32, factor: f32) -> Option<f32> {
    let next = font * factor;
    (next >= READING_MIN_PT).then(|| next.min(READING_MAX_PT))
}

pub fn leave_zoom(box_screen_w: f32) -> f64 {
    0.7 * FILE_OPEN_PX as f64 / box_screen_w.max(1.0) as f64
}

pub fn fill_view(scene: &mut Scene, reading: Option<(NodeId, f32)>, view: Rect) -> Option<(NodeId, f32)> {
    let i = scene.boxes.iter().position(|b| b.source)?;
    let mut b = scene.boxes.swap_remove(i);
    let font = match reading {
        Some((n, f)) if n == b.node => f,
        _ => READING_OPEN_PT,
    };
    b.rect = view;
    scene.boxes = vec![b];
    scene.edges.clear();
    scene.frames.clear();
    scene.labels.clear();
    Some((scene.boxes[0].node, font))
}
