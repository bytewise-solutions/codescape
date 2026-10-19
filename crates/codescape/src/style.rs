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
pub const ADDED: Color32 = Color32::from_rgb(165, 214, 167);
pub const USES: Color32 = Color32::from_rgb(222, 120, 200);
pub const ENTRY: Color32 = Color32::from_rgb(111, 191, 115);
pub const FRAME: Color32 = Color32::from_rgb(85, 91, 102);

pub fn depth_fill(depth: u32, open: bool) -> Color32 {
    let base = if open { BOX_OPEN_FILL } else { BOX_FILL };
    let lift = (depth.saturating_sub(1) * 6).min(30) as u8;
    Color32::from_rgb(base.r().saturating_add(lift), base.g().saturating_add(lift), base.b().saturating_add(lift))
}

pub fn text_styles() -> std::collections::BTreeMap<egui::TextStyle, egui::FontId> {
    use egui::{
        FontFamily::{Monospace, Proportional},
        FontId, TextStyle,
    };
    [
        (TextStyle::Small, FontId::new(11.5, Proportional)),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, Monospace)),
        (TextStyle::Heading, FontId::new(18.0, Proportional)),
    ]
    .into()
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

pub fn kind_stroke(sub: codescape_format::SubKind) -> Color32 {
    use codescape_format::SubKind::*;
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
    Pill { color: Color32, text: &'static str },
    Text { text: &'static str },
    Lines,
    Ramp,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LegendShown {
    pub interface: bool,
    pub classes: bool,
    pub containers: bool,
    pub enumeration: bool,
    pub function: bool,
    pub constructor: bool,
    pub wiring: bool,
    pub error: bool,
    pub visibility: bool,
    pub used_outside: bool,
    pub complexity: bool,
    pub complexity_high: bool,
    pub complexity_colour: bool,
    pub unused: bool,
    pub added: bool,
    pub changed: bool,
    pub reviewed: bool,
    pub added_arrow: bool,
    pub lines: bool,
    pub selected: bool,
    pub uses: bool,
    pub used_by: bool,
    pub entry: bool,
    pub group: bool,
    pub arrow_calls: bool,
    pub arrow_up: bool,
    pub arrow_same_layer: bool,
    pub arrow_undeclared: bool,
    pub arrow_implements: bool,
    pub arrow_test: bool,
    pub arrow_wiring: bool,
    pub arrow_guessed: bool,
}

impl LegendShown {
    pub fn all() -> LegendShown {
        LegendShown {
            interface: true,
            classes: true,
            containers: true,
            enumeration: true,
            function: true,
            constructor: true,
            wiring: true,
            error: true,
            visibility: true,
            used_outside: true,
            complexity: true,
            complexity_high: true,
            complexity_colour: true,
            unused: true,
            added: true,
            changed: true,
            reviewed: true,
            added_arrow: true,
            lines: true,
            selected: true,
            uses: true,
            used_by: true,
            entry: true,
            group: true,
            arrow_calls: true,
            arrow_up: true,
            arrow_same_layer: true,
            arrow_undeclared: true,
            arrow_implements: true,
            arrow_test: true,
            arrow_wiring: true,
            arrow_guessed: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LegendItem {
    pub label: &'static str,
    pub sample: LegendSample,
}

const fn item(label: &'static str, sample: LegendSample) -> LegendItem {
    LegendItem { label, sample }
}

pub fn legend(on: LegendShown) -> Vec<LegendItem> {
    use LegendSample::*;
    let arrow = |color, dashed, dotted, hollow_head| Arrow { color, dashed, dotted, hollow_head };
    let blue = match (on.classes, on.containers) {
        (true, true) => Some("class / struct / package"),
        (true, false) => Some("class / struct"),
        (false, true) => Some("module / package"),
        (false, false) => None,
    };
    let candidates = [
        (on.interface, item("interface", Box { stroke: INTERFACE })),
        (blue.is_some(), item(blue.unwrap_or_default(), Box { stroke: BOX_STROKE })),
        (on.enumeration, item("enum / record", Box { stroke: ENUM })),
        (on.function, item("function / method", Box { stroke: FUNCTION })),
        (on.constructor, item("constructor", Box { stroke: CONSTRUCTOR })),
        (on.wiring, item("wiring (DI)", Box { stroke: WIRING })),
        (on.error, item("error type", Box { stroke: ERROR_TYPE })),
        (on.visibility, item("public / protected / package / private", Text { text: "+#~-" })),
        (on.used_outside, item("used from outside its type", Dot { color: MUTED })),
        (on.complexity, item("complexity from 10", Pill { color: EDGE_SIDEWAYS, text: "10" })),
        (on.complexity_high, item("complexity from 20", Pill { color: EDGE_UP, text: "20" })),
        (on.complexity_colour, item("box tint: complexity", Ramp)),
        (on.unused, item("unused code", Outline { stroke: EDGE_UP, width: 1.5 })),
        (on.added, item("new", Pill { color: ADDED, text: "NEW" })),
        (on.changed, item("changed", Pill { color: EDGE_SIDEWAYS, text: "+1" })),
        (on.reviewed, item("reviewed", Pill { color: MUTED, text: "ok" })),
        (on.added_arrow, item("arrow the change adds", arrow(ADDED, false, false, false))),
        (on.lines, item("lines added / removed", Lines)),
        (on.selected, item("selected", Outline { stroke: ACCENT, width: 2.5 })),
        (on.uses, item("the selection uses it", Outline { stroke: USES, width: 2.0 })),
        (on.used_by, item("it uses the selection", Outline { stroke: CALLER, width: 2.0 })),
        (on.entry, item("has an entry point", Dot { color: ENTRY })),
        (on.group, item("group", Frame)),
        (on.arrow_calls, item("calls / depends on", arrow(EDGE, false, false, false))),
        (on.arrow_up, item("upward dependency", arrow(EDGE_UP, true, false, false))),
        (on.arrow_same_layer, item("same layer", arrow(EDGE_SIDEWAYS, false, false, false))),
        (on.arrow_undeclared, item("undeclared dependency", arrow(EDGE_SIDEWAYS, true, false, false))),
        (on.arrow_implements, item("implements / extends", arrow(EDGE, true, false, true))),
        (on.arrow_test, item("test-only dependency", arrow(EDGE, true, false, false))),
        (on.arrow_wiring, item("wiring arrow", arrow(WIRING, false, true, false))),
        (on.arrow_guessed, item("only guessed calls", arrow(EDGE, false, true, false))),
    ];
    candidates.into_iter().filter(|(shown, _)| *shown).map(|(_, it)| it).collect()
}

pub const CODE_KEYWORD: Color32 = Color32::from_rgb(198, 146, 233);
pub const CODE_STRING: Color32 = Color32::from_rgb(152, 195, 121);

pub const CALLER: Color32 = Color32::from_rgb(110, 165, 255);

pub fn edge_width(links: u32, marked: bool) -> f32 {
    let w = (0.6 + (links.max(1) as f32).log2() * 0.45).min(3.5);
    if marked { w.max(2.2) } else { w }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeLook {
    pub width: f32,
    pub alpha: f32,
}

pub fn edge_look(links: u32, max_links: u32, lit: bool) -> EdgeLook {
    if lit {
        return EdgeLook { width: edge_width(links, false).max(1.4), alpha: 1.0 };
    }
    let share = if max_links > 1 { (links.max(1) - 1) as f32 / (max_links - 1) as f32 } else { 0.0 };
    let share = share.clamp(0.0, 1.0);
    EdgeLook { width: 0.7 + 0.5 * share, alpha: 0.18 + 0.32 * share }
}

pub fn edge_alpha(links: u32) -> f32 {
    (0.4 + (links.max(1) as f32).log2() * 0.2).min(1.0)
}

pub fn stereotype(sub: codescape_format::SubKind, lang: codescape_format::Lang) -> Option<&'static str> {
    use codescape_format::{Lang, SubKind};
    match sub {
        SubKind::Interface if lang == Lang::Rust => Some("«trait»"),
        SubKind::Interface => Some("«interface»"),
        SubKind::Enum => Some("«enumeration»"),
        SubKind::Record => Some("«record»"),
        SubKind::Struct => Some("«struct»"),
        SubKind::Annotation => Some("«annotation»"),
        _ => None,
    }
}

pub fn complexity_hint(value: u16, hint: u16, high: u16) -> Option<Color32> {
    match value {
        v if v >= high => Some(EDGE_UP),
        v if v >= hint => Some(EDGE_SIDEWAYS),
        _ => None,
    }
}

pub const RAMP_COOL: Color32 = Color32::from_rgb(70, 130, 200);

pub fn complexity_ramp(value: u16, hint: u16, high: u16) -> Color32 {
    let hint = hint.max(2);
    let high = high.max(hint + 1);
    if value < hint {
        let t = f32::from(value.saturating_sub(1)) / f32::from(hint - 1);
        return tint(RAMP_COOL, EDGE_SIDEWAYS, t);
    }
    let t = f32::from((value - hint).min(high - hint)) / f32::from(high - hint);
    tint(EDGE_SIDEWAYS, EDGE_UP, t)
}

pub const RAMP_TINT: f32 = 0.3;
