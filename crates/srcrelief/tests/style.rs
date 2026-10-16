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
