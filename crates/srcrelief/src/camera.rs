use srcrelief_layout::Rect;

pub const MIN_ZOOM: f64 = 1e-6;
pub const MAX_ZOOM: f64 = 1e8;
const FIT_MARGIN: f64 = 0.9;
const SPEED: f64 = 12.0;
const READING_SHARE: f64 = 0.6;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl WRect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> WRect {
        WRect { x, y, w, h }
    }

    pub fn center(&self) -> [f64; 2] {
        [self.x + self.w / 2.0, self.y + self.h / 2.0]
    }
}

impl From<Rect> for WRect {
    fn from(r: Rect) -> WRect {
        WRect::new(r.x as f64, r.y as f64, r.w as f64, r.h as f64)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub center: [f64; 2],
    pub zoom: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { center: [0.0, 0.0], zoom: 1.0 }
    }
}

fn view_center(view: Rect) -> [f64; 2] {
    let c = view.center();
    [c[0] as f64, c[1] as f64]
}

impl Camera {
    pub fn world_to_screen(&self, p: [f64; 2], view: Rect) -> [f32; 2] {
        let c = view_center(view);
        [((p[0] - self.center[0]) * self.zoom + c[0]) as f32, ((p[1] - self.center[1]) * self.zoom + c[1]) as f32]
    }

    pub fn screen_to_world(&self, p: [f32; 2], view: Rect) -> [f64; 2] {
        let c = view_center(view);
        [(p[0] as f64 - c[0]) / self.zoom + self.center[0], (p[1] as f64 - c[1]) / self.zoom + self.center[1]]
    }

    pub fn world_to_screen_rect(&self, r: WRect, view: Rect) -> Rect {
        let tl = self.world_to_screen([r.x, r.y], view);
        Rect::new(tl[0], tl[1], (r.w * self.zoom) as f32, (r.h * self.zoom) as f32)
    }

    pub fn screen_to_world_rect(&self, r: Rect, view: Rect) -> WRect {
        let tl = self.screen_to_world([r.x, r.y], view);
        WRect::new(tl[0], tl[1], r.w as f64 / self.zoom, r.h as f64 / self.zoom)
    }

    pub fn zoom_at(&mut self, screen: [f32; 2], factor: f64, view: Rect) {
        let anchor = self.screen_to_world(screen, view);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let c = view_center(view);
        self.center = [anchor[0] - (screen[0] as f64 - c[0]) / self.zoom, anchor[1] - (screen[1] as f64 - c[1]) / self.zoom];
    }

    pub fn pan_by(&mut self, screen_delta: [f32; 2]) {
        self.center[0] -= screen_delta[0] as f64 / self.zoom;
        self.center[1] -= screen_delta[1] as f64 / self.zoom;
    }

    pub fn fit(target: impl Into<WRect>, view: Rect) -> Camera {
        let t: WRect = target.into();
        let zx = view.w as f64 / t.w.max(1e-12);
        let zy = view.h as f64 / t.h.max(1e-12);
        Camera { center: t.center(), zoom: (zx.min(zy) * FIT_MARGIN).clamp(MIN_ZOOM, MAX_ZOOM) }
    }

    pub fn reading(target: impl Into<WRect>, view: Rect) -> Camera {
        let t: WRect = target.into();
        let fit = Camera::fit(t, view);
        let wide = view.w as f64 * READING_SHARE / t.w.max(1e-12);
        if fit.zoom >= wide {
            return fit;
        }
        let zoom = wide.clamp(MIN_ZOOM, MAX_ZOOM);
        let half_h = view.h as f64 / 2.0 / zoom;
        let top = t.y + half_h - view.h as f64 * 0.04 / zoom;
        Camera { center: [t.center()[0], top.min(t.center()[1])], zoom }
    }

    pub fn animate_to(&mut self, target: &Camera, dt: f32) -> bool {
        let t = 1.0 - (-(dt as f64) * SPEED).exp();
        self.zoom = (self.zoom.ln() + (target.zoom.ln() - self.zoom.ln()) * t).exp();
        self.center[0] += (target.center[0] - self.center[0]) * t;
        self.center[1] += (target.center[1] - self.center[1]) * t;
        let dx = (target.center[0] - self.center[0]) * self.zoom;
        let dy = (target.center[1] - self.center[1]) * self.zoom;
        let settled = (self.zoom / target.zoom - 1.0).abs() < 1e-4 && dx.abs() < 0.25 && dy.abs() < 0.25;
        if settled {
            *self = *target;
        }
        !settled
    }
}
