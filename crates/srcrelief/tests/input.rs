use srcrelief::input::{Nav, interpret};

#[test]
fn ctrl_wheel_zooms_wheel_scrolls_shift_scrolls_sideways() {
    match interpret(true, false, [0.0, 40.0], 1.0) {
        Nav::Zoom(f) => assert!(f > 1.0),
        other => panic!("ctrl+wheel up should zoom in, got {other:?}"),
    }
    match interpret(true, false, [0.0, -40.0], 1.0) {
        Nav::Zoom(f) => assert!(f < 1.0),
        other => panic!("ctrl+wheel down should zoom out, got {other:?}"),
    }
    assert_eq!(interpret(false, false, [0.0, 40.0], 1.0), Nav::Pan([0.0, 40.0]));
    assert_eq!(interpret(false, true, [0.0, 40.0], 1.0), Nav::Pan([40.0, 0.0]));
    assert_eq!(interpret(false, true, [40.0, 0.0], 1.0), Nav::Pan([40.0, 0.0]), "some systems already turn shift+wheel into x");
    assert_eq!(interpret(false, false, [12.0, -7.0], 1.0), Nav::Pan([12.0, -7.0]), "trackpad swipe pans both ways");
    assert_eq!(interpret(false, false, [0.0, 0.0], 1.0), Nav::None);
}

#[test]
fn pinch_zooms() {
    assert_eq!(interpret(false, false, [0.0, 0.0], 1.25), Nav::Zoom(1.25));
    assert_eq!(interpret(true, false, [0.0, 0.0], 0.8), Nav::Zoom(0.800000011920929));
}
