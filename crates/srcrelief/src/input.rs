use srcrelief_format::NodeId;
use srcrelief_layout::Rect;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Left,
    Right,
    Up,
    Down,
}

pub fn nearest_in(step: Step, from_id: NodeId, from: Rect, candidates: &[(NodeId, Rect)]) -> Option<NodeId> {
    let [fx, fy] = from.center();
    candidates
        .iter()
        .filter(|(id, _)| *id != from_id)
        .filter_map(|&(id, r)| {
            let [cx, cy] = r.center();
            let (primary, cross) = match step {
                Step::Right => (cx - fx, cy - fy),
                Step::Left => (fx - cx, cy - fy),
                Step::Down => (cy - fy, cx - fx),
                Step::Up => (fy - cy, cx - fx),
            };
            (primary > 1.0).then_some((primary + 2.0 * cross.abs(), id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

pub const SHORTCUTS: &[(&str, &str)] = &[
    ("← → ↑ ↓", "select the nearest box in that direction"),
    ("Enter", "zoom into the selected box"),
    ("Backspace", "go up to the parent"),
    ("Esc", "clear the selection"),
    ("1 2 3", "show neighbours / everything it reaches / everywhere it's used"),
    ("+ −", "zoom in / out"),
    ("F", "fit the whole project"),
    ("Page Up Page Down Home End", "scroll the file under the pointer or the selected one"),
    ("Alt+←", "back to the previous selection"),
    ("Ctrl+K", "jump to a name"),
    ("?", "this list"),
];
