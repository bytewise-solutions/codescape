use egui::Color32;

pub const BG: Color32 = Color32::from_rgb(27, 29, 34);
pub const PANEL: Color32 = Color32::from_rgb(32, 35, 41);
pub const BAR: Color32 = Color32::from_rgb(36, 39, 46);
pub const TEXT: Color32 = Color32::from_rgb(216, 219, 226);
pub const MUTED: Color32 = Color32::from_rgb(125, 133, 148);
pub const ACCENT: Color32 = Color32::from_rgb(233, 162, 59);
pub const BOX_FILL: Color32 = Color32::from_rgb(39, 48, 63);
pub const BOX_OPEN_FILL: Color32 = Color32::from_rgb(35, 42, 54);
pub const BOX_STROKE: Color32 = Color32::from_rgb(92, 141, 214);
pub const BOX_TITLE: Color32 = Color32::from_rgb(143, 179, 234);
pub const EXTERNAL: Color32 = Color32::from_rgb(119, 119, 119);
pub const EDGE: Color32 = Color32::from_rgb(125, 133, 148);
pub const EDGE_UP: Color32 = Color32::from_rgb(224, 93, 93);
pub const EDGE_SIDEWAYS: Color32 = Color32::from_rgb(233, 162, 59);
pub const REACH: Color32 = Color32::from_rgb(165, 214, 167);
pub const ENTRY: Color32 = Color32::from_rgb(111, 191, 115);
pub const FRAME: Color32 = Color32::from_rgb(85, 91, 102);

pub fn depth_fill(depth: u32, open: bool) -> Color32 {
    let base = if open { BOX_OPEN_FILL } else { BOX_FILL };
    let lift = (depth.saturating_sub(1) * 6).min(30) as u8;
    Color32::from_rgb(base.r().saturating_add(lift), base.g().saturating_add(lift), base.b().saturating_add(lift))
}

pub fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = BAR;
    v.extreme_bg_color = BG;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = Color32::from_rgb(46, 125, 50);
    v
}
