use srcrelief::camera::Camera;
use srcrelief_layout::Rect;

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

const VIEW: Rect = Rect { x: 100.0, y: 50.0, w: 800.0, h: 600.0 };

#[test]
fn world_screen_roundtrip() {
    let cam = Camera { center: [10.0, 20.0], zoom: 2.5 };
    let p = [33.0, -7.0];
    assert!(close(cam.screen_to_world(cam.world_to_screen(p, VIEW), VIEW), p));
    assert!(close(cam.world_to_screen(cam.center, VIEW), [500.0, 350.0]));
}

#[test]
fn zoom_at_keeps_point_fixed() {
    let mut cam = Camera { center: [0.0, 0.0], zoom: 1.0 };
    let cursor = [700.0, 200.0];
    let before = cam.screen_to_world(cursor, VIEW);
    cam.zoom_at(cursor, 3.0, VIEW);
    assert!((cam.zoom - 3.0).abs() < 1e-6);
    assert!(close(cam.screen_to_world(cursor, VIEW), before));
}

#[test]
fn zoom_is_clamped() {
    let mut cam = Camera { center: [0.0, 0.0], zoom: 1.0 };
    cam.zoom_at([500.0, 350.0], 1e12, VIEW);
    assert!(cam.zoom <= srcrelief::camera::MAX_ZOOM);
    cam.zoom_at([500.0, 350.0], 1e-30, VIEW);
    assert!(cam.zoom >= srcrelief::camera::MIN_ZOOM);
}

#[test]
fn fit_centers_rect_with_margin() {
    let target = Rect::new(0.0, 0.0, 400.0, 100.0);
    let cam = Camera::fit(target, VIEW);
    assert!(close(cam.center, [200.0, 50.0]));
    let tl = cam.world_to_screen([0.0, 0.0], VIEW);
    let br = cam.world_to_screen([400.0, 100.0], VIEW);
    assert!(tl[0] > VIEW.x && br[0] < VIEW.right(), "margin on both sides: {tl:?} {br:?}");
    assert!((br[0] - tl[0]) > VIEW.w * 0.8);
}

#[test]
fn animation_reaches_target() {
    let mut cam = Camera { center: [0.0, 0.0], zoom: 1.0 };
    let target = Camera { center: [1000.0, -500.0], zoom: 8.0 };
    let mut steps = 0;
    while cam.animate_to(&target, 1.0 / 60.0) {
        steps += 1;
        assert!(steps < 120, "animation never settles");
    }
    assert!(close(cam.center, target.center) && (cam.zoom - target.zoom).abs() < 1e-3);
    assert!(steps > 3, "animation should take several frames, took {steps}");
}
