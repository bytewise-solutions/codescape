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

pub const WIRING: Color32 = Color32::from_rgb(150, 120, 88);
pub const INTERFACE: Color32 = Color32::from_rgb(38, 166, 154);
pub const ENUM: Color32 = Color32::from_rgb(149, 117, 205);
pub const ANNOTATION: Color32 = Color32::from_rgb(130, 120, 160);
pub const FUNCTION: Color32 = Color32::from_rgb(128, 136, 148);
pub const CONSTRUCTOR: Color32 = Color32::from_rgb(96, 118, 158);

pub fn kind_stroke(sub: srcrelief_format::SubKind) -> Color32 {
    use srcrelief_format::SubKind::*;
    match sub {
        Interface => INTERFACE,
        Enum | Record => ENUM,
        Annotation => ANNOTATION,
        Function | Method | Field => FUNCTION,
        Constructor => CONSTRUCTOR,
        _ => BOX_STROKE,
    }
}

pub fn tint(base: Color32, over: Color32, amount: f32) -> Color32 {
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * amount) as u8;
    Color32::from_rgb(mix(base.r(), over.r()), mix(base.g(), over.g()), mix(base.b(), over.b()))
}

pub const ERROR_TYPE: Color32 = Color32::from_rgb(200, 120, 120);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LegendSample {
    Box { stroke: Color32 },
    Outline { stroke: Color32, width: f32 },
    Dot { color: Color32 },
    Frame,
    Arrow { color: Color32, dashed: bool, dotted: bool, hollow_head: bool },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LegendItem {
    pub label: &'static str,
    pub sample: LegendSample,
}

const fn item(label: &'static str, sample: LegendSample) -> LegendItem {
    LegendItem { label, sample }
}

pub fn legend(show_wiring: bool, show_noise: bool) -> Vec<LegendItem> {
    use LegendSample::*;
    let mut v = vec![
        item("interface", Box { stroke: INTERFACE }),
        item("class / struct", Box { stroke: BOX_STROKE }),
        item("enum / record", Box { stroke: ENUM }),
        item("function / method", Box { stroke: FUNCTION }),
        item("constructor", Box { stroke: CONSTRUCTOR }),
    ];
    if show_wiring {
        v.push(item("wiring (DI)", Box { stroke: WIRING }));
    }
    if show_noise {
        v.push(item("error type", Box { stroke: ERROR_TYPE }));
    }
    v.extend([
        item("selected", Outline { stroke: ACCENT, width: 2.5 }),
        item("calls (from selection)", Outline { stroke: REACH, width: 2.0 }),
        item("called by / sets up", Outline { stroke: CALLER, width: 2.0 }),
        item("has an entry point", Dot { color: ENTRY }),
        item("group", Frame),
        item("calls / depends on", Arrow { color: EDGE, dashed: false, dotted: false, hollow_head: false }),
        item("upward dependency", Arrow { color: EDGE_UP, dashed: true, dotted: false, hollow_head: false }),
        item("same layer", Arrow { color: EDGE_SIDEWAYS, dashed: false, dotted: false, hollow_head: false }),
        item("undeclared dependency", Arrow { color: EDGE_SIDEWAYS, dashed: true, dotted: false, hollow_head: false }),
        item("implements / extends", Arrow { color: EDGE, dashed: true, dotted: false, hollow_head: true }),
        item("test-only dependency", Arrow { color: EDGE, dashed: true, dotted: false, hollow_head: false }),
    ]);
    if show_wiring {
        v.push(item("wiring arrow", Arrow { color: WIRING, dashed: false, dotted: true, hollow_head: false }));
    }
    v
}

pub const CALLER: Color32 = Color32::from_rgb(110, 165, 255);

pub fn edge_width(links: u32, marked: bool) -> f32 {
    let w = (0.6 + (links.max(1) as f32).log2() * 0.45).min(3.5);
    if marked { w.max(2.2) } else { w }
}

pub fn edge_alpha(links: u32) -> f32 {
    (0.4 + (links.max(1) as f32).log2() * 0.2).min(1.0)
}
