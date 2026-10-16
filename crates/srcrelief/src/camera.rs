use srcrelief_layout::Rect;

pub const MIN_ZOOM: f32 = 1e-5;
pub const MAX_ZOOM: f32 = 40.0;
const FIT_MARGIN: f32 = 0.9;
const SPEED: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub center: [f32; 2],
    pub zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { center: [0.0, 0.0], zoom: 1.0 }
    }
}

impl Camera {
    pub fn world_to_screen(&self, p: [f32; 2], view: Rect) -> [f32; 2] {
        let c = view.center();
        [(p[0] - self.center[0]) * self.zoom + c[0], (p[1] - self.center[1]) * self.zoom + c[1]]
    }

    pub fn screen_to_world(&self, p: [f32; 2], view: Rect) -> [f32; 2] {
        let c = view.center();
        [(p[0] - c[0]) / self.zoom + self.center[0], (p[1] - c[1]) / self.zoom + self.center[1]]
    }

    pub fn world_to_screen_rect(&self, r: Rect, view: Rect) -> Rect {
        let tl = self.world_to_screen([r.x, r.y], view);
        Rect::new(tl[0], tl[1], r.w * self.zoom, r.h * self.zoom)
    }

    pub fn screen_to_world_rect(&self, r: Rect, view: Rect) -> Rect {
        let tl = self.screen_to_world([r.x, r.y], view);
        Rect::new(tl[0], tl[1], r.w / self.zoom, r.h / self.zoom)
    }

    pub fn zoom_at(&mut self, screen: [f32; 2], factor: f32, view: Rect) {
        let anchor = self.screen_to_world(screen, view);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let c = view.center();
        self.center = [anchor[0] - (screen[0] - c[0]) / self.zoom, anchor[1] - (screen[1] - c[1]) / self.zoom];
    }

    pub fn pan_by(&mut self, screen_delta: [f32; 2]) {
        self.center[0] -= screen_delta[0] / self.zoom;
        self.center[1] -= screen_delta[1] / self.zoom;
    }

    pub fn fit(target: Rect, view: Rect) -> Camera {
        let zx = view.w / target.w.max(1e-6);
        let zy = view.h / target.h.max(1e-6);
        Camera { center: target.center(), zoom: (zx.min(zy) * FIT_MARGIN).clamp(MIN_ZOOM, MAX_ZOOM) }
    }

    pub fn animate_to(&mut self, target: &Camera, dt: f32) -> bool {
        let t = 1.0 - (-dt * SPEED).exp();
        let log_zoom = self.zoom.ln() + (target.zoom.ln() - self.zoom.ln()) * t;
        self.zoom = log_zoom.exp();
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
