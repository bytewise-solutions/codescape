use srcrelief::style::edge_width;

#[test]
fn edge_width_is_visible_for_every_weight() {
    for w in [0u32, 1, 10, 1000, u32::MAX] {
        let width = edge_width(w, false);
        assert!(width.is_finite() && width >= 1.0, "weight {w} -> {width}");
    }
    assert!(edge_width(1000, false) > edge_width(1, false));
    assert!(edge_width(0, true) >= 2.0);
}

#[test]
fn legend_explains_every_style() {
    use srcrelief::style::{LegendSample, legend};
    let items = legend(true, true);
    let labels: Vec<&str> = items.iter().map(|i| i.label).collect();
    for needed in ["interface", "class / struct", "enum / record", "function / method", "constructor", "wiring (DI)", "error type", "calls / depends on", "upward dependency", "same layer", "undeclared dependency", "implements / extends", "wiring arrow", "test-only dependency", "selected", "calls (from selection)", "called by / sets up", "has an entry point", "group"] {
        assert!(labels.contains(&needed), "legend misses {needed}");
    }
    let lean = legend(false, false);
    assert!(lean.iter().all(|i| !i.label.starts_with("wiring") && i.label != "error type"));
    assert!(items.iter().any(|i| matches!(i.sample, LegendSample::Arrow { dashed: true, hollow_head: true, .. })));
}
