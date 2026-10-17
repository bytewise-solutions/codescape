use srcrelief_format::{Lang, SubKind};
use srcrelief::style::{edge_alpha, edge_width};

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
    use srcrelief::style::{LegendShown, LegendSample, legend};
    let all = LegendShown { wiring: true, noise: true, complexity: true, unused: true, review: true };
    let items = legend(all);
    let labels: Vec<&str> = items.iter().map(|i| i.label).collect();
    for needed in [
        "interface", "class / struct", "enum / record", "function / method", "constructor", "wiring (DI)", "error type", "calls / depends on",
        "upward dependency", "same layer", "undeclared dependency", "implements / extends", "wiring arrow", "test-only dependency", "selected",
        "the selection uses it", "it uses the selection", "has an entry point", "group", "used from outside its type",
        "public / protected / package / private", "complexity from 10", "complexity from 20", "unused code", "new", "changed",
        "reviewed", "arrow the change adds", "lines added / removed",
    ] {
        assert!(labels.contains(&needed), "legend misses {needed}");
    }
    let lean = legend(LegendShown::default());
    assert!(lean.iter().all(|i| !i.label.starts_with("wiring") && i.label != "error type"));
    assert!(lean.iter().all(|i| !["complexity from 10", "unused code", "new", "reviewed"].contains(&i.label)), "only what is on the map");
    assert!(items.iter().any(|i| matches!(i.sample, LegendSample::Arrow { dashed: true, hollow_head: true, .. })));
}

#[test]
fn quiet_arrows_stay_faint_until_lit() {
    use srcrelief::style::{EdgeLook, edge_look};
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
    use srcrelief::style::stereotype;
    assert_eq!(stereotype(SubKind::Interface, Lang::Java), Some("«interface»"));
    assert_eq!(stereotype(SubKind::Interface, Lang::Rust), Some("«trait»"));
    assert_eq!(stereotype(SubKind::Enum, Lang::Go), Some("«enumeration»"));
    assert_eq!(stereotype(SubKind::Struct, Lang::Go), Some("«struct»"));
    assert_eq!(stereotype(SubKind::Class, Lang::Java), None, "a plain class has no stereotype in UML");
}

#[test]
fn complexity_hints_appear_from_ten_and_turn_red_from_twenty() {
    use srcrelief::style::{EDGE_UP, EDGE_SIDEWAYS, complexity_hint};
    assert_eq!(complexity_hint(9), None);
    assert_eq!(complexity_hint(10), Some(EDGE_SIDEWAYS));
    assert_eq!(complexity_hint(19), Some(EDGE_SIDEWAYS));
    assert_eq!(complexity_hint(20), Some(EDGE_UP));
}

#[test]
fn what_the_selection_uses_is_not_coloured_like_new_code() {
    use srcrelief::style::{LegendSample, LegendShown, legend};
    let items = legend(LegendShown { review: true, ..Default::default() });
    let color = |label: &str| match items.iter().find(|i| i.label == label).unwrap().sample {
        LegendSample::Outline { stroke, .. } => stroke,
        LegendSample::Pill { color, .. } => color,
        other => panic!("{other:?}"),
    };
    assert_ne!(color("the selection uses it"), color("new"));
    assert_ne!(color("the selection uses it"), color("it uses the selection"));
}
