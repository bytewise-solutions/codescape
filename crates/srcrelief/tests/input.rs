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

#[test]
fn arrow_keys_pick_the_nearest_box_in_that_direction() {
    use srcrelief::input::{Step, nearest_in};
    use srcrelief_layout::Rect;
    let at = |x: f32, y: f32| Rect::new(x, y, 40.0, 20.0);
    let boxes = [(1, at(100.0, 100.0)), (2, at(200.0, 100.0)), (3, at(300.0, 100.0)), (4, at(110.0, 200.0)), (5, at(400.0, 210.0)), (6, at(190.0, 20.0))];
    let from = boxes[0].1;
    assert_eq!(nearest_in(Step::Right, 1, from, &boxes), Some(2), "next in the row, not the far one");
    assert_eq!(nearest_in(Step::Down, 1, from, &boxes), Some(4), "straight below beats diagonal");
    assert_eq!(nearest_in(Step::Up, 1, from, &boxes), Some(6));
    assert_eq!(nearest_in(Step::Left, 1, from, &boxes), None);
    assert_eq!(nearest_in(Step::Left, 3, boxes[2].1, &boxes), Some(2));
}

#[test]
fn the_cheatsheet_covers_keys_and_mouse_by_task() {
    let sheet = srcrelief::input::CHEATSHEET;
    let sections: Vec<&str> = sheet.iter().map(|(s, _)| *s).collect();
    assert_eq!(sections, ["Move around", "Select and follow", "Code", "Review", "Windows"]);
    let keys: Vec<&str> = sheet.iter().flat_map(|(_, rows)| rows.iter().map(|(k, _)| *k)).collect();
    for k in ["← → ↑ ↓", "Enter", "Backspace", "Esc", "1 2 3", "+ −", "F", "Alt+←", "Ctrl+K", "Ctrl+O", "Ctrl+,", "? or F1", "Ctrl + wheel", "Shift + wheel", "Drag", "Double-click", "Page Up Page Down Home End", "N / P", "R", "[ ]", "S"] {
        assert!(keys.contains(&k), "cheatsheet misses {k}");
    }
}
