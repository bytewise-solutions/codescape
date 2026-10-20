use codescape::style::{edge_alpha, edge_width};
use codescape_format::{Lang, SubKind};

#[test]
fn arrow_weight_follows_the_number_of_links() {
    assert!(edge_width(1, false) < 1.0, "a single link is a hairline");
    assert!(edge_alpha(1) < 0.6);
    let widths: Vec<f32> = [1u32, 2, 4, 16, 1000, u32::MAX].iter().map(|&l| edge_width(l, false)).collect();
    assert!(widths.windows(2).all(|w| w[0] <= w[1]), "{widths:?}");
    assert!(widths.iter().all(|w| w.is_finite() && *w > 0.0 && *w <= 4.0));
    assert!(edge_width(16, false) >= 2.0 * edge_width(1, false));
    assert!(edge_width(0, false) > 0.0, "implements-only or wiring-only arrows stay visible");
    assert_eq!(edge_alpha(1000), 1.0);
    assert!(edge_width(1, true) >= 2.0, "a highlighted arrow is always readable");
}

#[test]
fn legend_explains_every_style() {
    use codescape::style::{LegendSample, LegendShown, legend};
    let items = legend(LegendShown::all());
    let labels: Vec<&str> = items.iter().map(|i| i.label).collect();
    for needed in [
        "interface",
        "class / struct / package",
        "enum / record",
        "function / method",
        "constructor",
        "wiring (DI)",
        "error type",
        "calls / depends on",
        "upward dependency",
        "same layer",
        "undeclared dependency",
        "implements / extends",
        "wiring arrow",
        "test-only dependency",
        "selected",
        "the selection uses it",
        "it uses the selection",
        "has an entry point",
        "group",
        "used from outside its type",
        "public / protected / package / private",
        "complexity from 10",
        "complexity from 20",
        "unused code",
        "new",
        "changed",
        "reviewed",
        "arrow the change adds",
        "lines added / removed",
        "only guessed calls",
    ] {
        assert!(labels.contains(&needed), "legend misses {needed}");
    }
    assert!(items.iter().any(|i| matches!(i.sample, LegendSample::Arrow { dashed: true, hollow_head: true, .. })));
}

#[test]
fn quiet_arrows_stay_faint_until_lit() {
    use codescape::style::{EdgeLook, edge_look};
    let faint = edge_look(1, 20, false);
    let heaviest = edge_look(20, 20, false);
    let lit = edge_look(1, 20, true);
    assert!(faint.width <= 0.8 && faint.alpha <= 0.25, "{faint:?}");
    assert!(heaviest.alpha > faint.alpha && heaviest.alpha < 0.6, "the main flows show a little: {heaviest:?}");
    assert!(heaviest.width < lit.width.max(edge_look(20, 20, true).width));
    assert_eq!(lit.alpha, 1.0);
    assert!(edge_look(16, 20, true).width > lit.width, "lit arrows keep their weight");
    assert_eq!(edge_look(1, 0, false), EdgeLook { ..faint }, "an empty container does not divide by zero");
}

#[test]
fn uml_stereotypes_name_the_kind_of_type() {
    use codescape::style::stereotype;
    assert_eq!(stereotype(SubKind::Interface, Lang::Java), Some("«interface»"));
    assert_eq!(stereotype(SubKind::Interface, Lang::Rust), Some("«trait»"));
    assert_eq!(stereotype(SubKind::Enum, Lang::Go), Some("«enumeration»"));
    assert_eq!(stereotype(SubKind::Struct, Lang::Go), Some("«struct»"));
    assert_eq!(stereotype(SubKind::Class, Lang::Java), None, "a plain class has no stereotype in UML");
}

#[test]
fn complexity_hints_appear_from_ten_and_turn_red_from_twenty() {
    use codescape::style::{EDGE_SIDEWAYS, EDGE_UP, complexity_hint};
    assert_eq!(complexity_hint(9, 10, 20), None);
    assert_eq!(complexity_hint(10, 10, 20), Some(EDGE_SIDEWAYS));
    assert_eq!(complexity_hint(19, 10, 20), Some(EDGE_SIDEWAYS));
    assert_eq!(complexity_hint(20, 10, 20), Some(EDGE_UP));
}

#[test]
fn what_the_selection_uses_is_not_coloured_like_new_code() {
    use codescape::style::{LegendSample, LegendShown, legend};
    let items = legend(LegendShown::all());
    let color = |label: &str| match items.iter().find(|i| i.label == label).unwrap().sample {
        LegendSample::Outline { stroke, .. } => stroke,
        LegendSample::Pill { color, .. } => color,
        other => panic!("{other:?}"),
    };
    assert_ne!(color("the selection uses it"), color("new"));
    assert_ne!(color("the selection uses it"), color("it uses the selection"));
}

#[test]
fn the_legend_lists_only_what_is_on_screen() {
    use codescape::style::{LegendShown, legend};
    assert!(legend(LegendShown::default()).is_empty());
    let with = |set: fn(&mut LegendShown)| {
        let mut shown = LegendShown::default();
        set(&mut shown);
        legend(shown).into_iter().map(|i| i.label).collect::<Vec<_>>()
    };
    assert_eq!(with(|s| s.interface = true), ["interface"]);
    assert_eq!(with(|s| s.arrow_up = true), ["upward dependency"]);
    assert_eq!(with(|s| s.containers = true), ["module / package"]);
    assert_eq!(with(|s| s.classes = true), ["class / struct"]);
    assert_eq!(
        with(|s| {
            s.containers = true;
            s.classes = true;
        }),
        ["class / struct / package"]
    );
    assert_eq!(with(|s| s.added = true), ["new"]);
}

#[test]
fn complexity_hints_use_the_given_levels() {
    use codescape::style::{EDGE_SIDEWAYS, EDGE_UP, complexity_hint};
    assert_eq!(complexity_hint(9, 10, 20), None);
    assert_eq!(complexity_hint(10, 10, 20), Some(EDGE_SIDEWAYS));
    assert_eq!(complexity_hint(20, 10, 20), Some(EDGE_UP));
    assert_eq!(complexity_hint(12, 15, 30), None);
}

#[test]
fn no_interface_text_is_smaller_than_eleven_px() {
    use codescape::style::text_styles;
    let styles = text_styles();
    assert!(styles.values().all(|f| f.size >= 11.0), "{styles:?}");
    assert!(styles[&egui::TextStyle::Small].size < styles[&egui::TextStyle::Body].size);
}

#[test]
fn complexity_ramp_is_neutral_below_the_hint_then_amber_to_red() {
    use codescape::style::complexity_ramp;
    let (at_hint, at_high) = (complexity_ramp(10, 10, 20).unwrap(), complexity_ramp(20, 10, 20).unwrap());
    for v in [0, 1, 5, 9] {
        assert_eq!(complexity_ramp(v, 10, 20), None, "{v} is below the hint and stays neutral");
    }
    assert!(at_hint.r() > at_hint.b() && at_hint.g() > at_high.g(), "the hint is amber: {at_hint:?}");
    assert!(at_high.r() > at_high.g() && at_high.r() > at_high.b(), "the high level is red: {at_high:?}");
    assert_eq!(complexity_ramp(500, 10, 20), Some(at_high), "beyond high stays at the hot end");
    let greens: Vec<u8> = (10..=20).map(|v| complexity_ramp(v, 10, 20).unwrap().g()).collect();
    assert!(greens.windows(2).all(|w| w[0] >= w[1]), "between hint and high the colour only heats up: {greens:?}");
    let _ = complexity_ramp(5, 20, 10);
    let _ = complexity_ramp(5, 0, 0);
}

#[test]
fn the_legend_shows_the_complexity_ramp_only_when_the_map_is_tinted() {
    use codescape::style::{LegendSample, LegendShown, legend};
    assert!(legend(LegendShown::all()).iter().any(|i| matches!(i.sample, LegendSample::Ramp)));
    assert!(legend(LegendShown { complexity_colour: true, ..Default::default() }).iter().any(|i| matches!(i.sample, LegendSample::Ramp)));
    assert!(legend(LegendShown::default()).is_empty());
}

#[test]
fn panel_text_scales_every_text_style() {
    use codescape::style::{scaled_text_styles, text_styles};
    let (base, big) = (text_styles(), scaled_text_styles(1.5));
    for (style, font) in &base {
        assert_eq!(big[style].size, font.size * 1.5, "{style:?}");
    }
}
