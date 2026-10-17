use codescape_format::NodeId;
use codescape_layout::Rect;

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

pub const CHEATSHEET: &[(&str, &[(&str, &str)])] = &[
    (
        "Move around",
        &[
            ("Wheel", "scroll up and down"),
            ("Shift + wheel", "scroll sideways"),
            ("Ctrl + wheel", "zoom around the pointer (pinch on a trackpad)"),
            ("Drag", "pan"),
            ("+ −", "zoom in / out"),
            ("F", "fit the whole project"),
            ("Double-click", "zoom into a box"),
            ("Enter", "zoom into the selected box"),
            ("Backspace", "go up to the parent"),
        ],
    ),
    (
        "Select and follow",
        &[
            ("Click", "select a box or a method row; its links light up"),
            ("← → ↑ ↓", "select the nearest box in that direction"),
            ("1 2 3", "show neighbours / everything it reaches / everywhere it's used"),
            ("Alt+←", "back to the previous selection"),
            ("Esc", "clear the selection"),
            ("Ctrl+K", "jump to a name (Up / Down to pick, Enter to go)"),
            ("S", "sequence diagram of the selected method (also on right-click)"),
        ],
    ),
    (
        "Code",
        &[
            ("Zoom in close", "a type or function opens its whole file; zoom out to close it"),
            ("Double-click a method row", "open the file at that method"),
            ("Wheel / Shift + wheel", "scroll the file down / sideways"),
            ("Page Up Page Down Home End", "scroll the file under the pointer or the selected one"),
            ("Drag the scrollbar", "move through the file"),
        ],
    ),
    (
        "Review",
        &[
            ("Review (top right)", "compare two versions: pick base and target in the review bar"),
            ("N / P", "next / previous change"),
            ("R", "mark the current change reviewed (again: unmark)"),
            ("[ ]", "previous / next commit, commit by commit"),
        ],
    ),
    (
        "Windows",
        &[
            ("Ctrl+O", "open a folder"),
            ("Ctrl+,", "project settings"),
            ("Ctrl+B", "hide / show the left panel"),
            ("Ctrl+I", "hide / show the right panel"),
            ("F11", "hide / show both: the whole window for the map"),
            ("? or F1", "this cheatsheet"),
        ],
    ),
];
