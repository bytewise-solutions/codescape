const WHEEL_ZOOM: f32 = 0.0025;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Nav {
    Zoom(f64),
    Pan([f32; 2]),
    None,
}

pub fn interpret(ctrl: bool, shift: bool, scroll: [f32; 2], zoom_delta: f32) -> Nav {
    if (zoom_delta - 1.0).abs() > 1e-4 {
        return Nav::Zoom(zoom_delta as f64);
    }
    if ctrl {
        let amount = if scroll[1] != 0.0 { scroll[1] } else { scroll[0] };
        return if amount == 0.0 { Nav::None } else { Nav::Zoom((amount * WHEEL_ZOOM).exp() as f64) };
    }
    if shift {
        let amount = if scroll[0] != 0.0 { scroll[0] } else { scroll[1] };
        return if amount == 0.0 { Nav::None } else { Nav::Pan([amount, 0.0]) };
    }
    if scroll == [0.0, 0.0] { Nav::None } else { Nav::Pan(scroll) }
}
