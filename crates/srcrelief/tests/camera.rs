use srcrelief::camera::Camera;
use srcrelief_layout::Rect;

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

fn close32(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

const VIEW: Rect = Rect { x: 100.0, y: 50.0, w: 800.0, h: 600.0 };

#[test]
fn world_screen_roundtrip() {
    let cam = Camera { center: [10.0, 20.0], zoom: 2.5 };
    let p = [33.0, -7.0];
    assert!(close(cam.screen_to_world(cam.world_to_screen(p, VIEW), VIEW), p));
    assert!(close32(cam.world_to_screen(cam.center, VIEW), [500.0, 350.0]));
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

#[test]
fn deep_zoom_stays_precise() {
    let target = Rect::new(2734.5, 1912.25, 0.002, 0.00125);
    let cam = Camera::fit(target, VIEW);
    assert!(cam.zoom > 100_000.0, "a 0.002-unit box must be reachable, zoom {}", cam.zoom);
    let tl = cam.world_to_screen([target.x as f64, target.y as f64], VIEW);
    let br = cam.world_to_screen([target.x as f64 + target.w as f64, target.y as f64 + target.h as f64], VIEW);
    let width = br[0] - tl[0];
    assert!((width - VIEW.w * 0.9).abs() < 2.0, "box spans {width} px");
}

#[test]
fn reading_a_tall_card_zooms_until_its_file_opens_with_its_top_in_view() {
    let card = Rect::new(0.0, 0.0, 100.0, 1000.0);
    assert!(100.0 * Camera::fit(card, VIEW).zoom < (VIEW.w * srcrelief::canvas::FILE_VIEW_SHARE) as f64, "fitting alone leaves it too narrow");
    let cam = Camera::reading(card, VIEW);
    let r = cam.world_to_screen_rect(card.into(), VIEW);
    assert!(r.w >= VIEW.w * srcrelief::canvas::FILE_VIEW_SHARE, "{} px wide", r.w);
    assert!(r.y >= VIEW.y && r.y < VIEW.y + VIEW.h * 0.1, "top at {}", r.y);
    assert!((r.center()[0] - VIEW.center()[0]).abs() < 1.0);
    let wide = Rect::new(0.0, 0.0, 1000.0, 300.0);
    assert_eq!(Camera::reading(wide, VIEW).zoom, Camera::fit(wide, VIEW).zoom, "a wide box just fits");
}
