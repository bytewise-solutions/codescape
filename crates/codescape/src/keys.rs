use egui::{Key, KeyboardShortcut, Modifiers};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    OpenFolder,
    Settings,
    Jump,
    Find,
    FindNext,
    FindPrevious,
    Structure,
    LeftPanel,
    RightPanel,
    BothPanels,
}

impl Action {
    pub const ALL: [Action; 10] = [
        Action::Jump,
        Action::Find,
        Action::FindNext,
        Action::FindPrevious,
        Action::Structure,
        Action::LeftPanel,
        Action::RightPanel,
        Action::BothPanels,
        Action::OpenFolder,
        Action::Settings,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Action::OpenFolder => "open_folder",
            Action::Settings => "settings",
            Action::Jump => "jump",
            Action::Find => "find",
            Action::FindNext => "find_next",
            Action::FindPrevious => "find_previous",
            Action::Structure => "structure",
            Action::LeftPanel => "left_panel",
            Action::RightPanel => "right_panel",
            Action::BothPanels => "both_panels",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::OpenFolder => "Open a folder",
            Action::Settings => "Project settings",
            Action::Jump => "Jump to a name",
            Action::Find => "Find in the open file",
            Action::FindNext => "Next match",
            Action::FindPrevious => "Previous match",
            Action::Structure => "File structure",
            Action::LeftPanel => "Hide or show the left panel",
            Action::RightPanel => "Hide or show the right panel",
            Action::BothPanels => "Map only",
        }
    }

    pub fn default_key(self) -> KeyboardShortcut {
        let ctrl = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
        match self {
            Action::OpenFolder => ctrl(Key::O),
            Action::Settings => ctrl(Key::Comma),
            Action::Jump => ctrl(Key::K),
            Action::Find => ctrl(Key::F),
            Action::FindNext => KeyboardShortcut::new(Modifiers::NONE, Key::F3),
            Action::FindPrevious => KeyboardShortcut::new(Modifiers::SHIFT, Key::F3),
            Action::Structure => KeyboardShortcut::new(Modifiers { shift: true, ..Modifiers::COMMAND }, Key::O),
            Action::LeftPanel => ctrl(Key::B),
            Action::RightPanel => ctrl(Key::I),
            Action::BothPanels => KeyboardShortcut::new(Modifiers::NONE, Key::F11),
        }
    }
}

pub fn parse(text: &str) -> Option<KeyboardShortcut> {
    let mut modifiers = Modifiers::NONE;
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" | "command" => modifiers |= Modifiers::COMMAND,
            "shift" => modifiers |= Modifiers::SHIFT,
            "alt" | "option" => modifiers |= Modifiers::ALT,
            "" if text.ends_with('+') && key.is_none() => key = Some(Key::Plus),
            "" => return None,
            _ if key.is_some() => return None,
            _ => key = Some(Key::from_name(part).or_else(|| Key::from_name(&part.to_ascii_uppercase()))?),
        }
    }
    key.map(|k| KeyboardShortcut::new(modifiers, k))
}

pub fn format(s: &KeyboardShortcut) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if s.modifiers.command || s.modifiers.ctrl || s.modifiers.mac_cmd {
        parts.push("Ctrl");
    }
    if s.modifiers.shift {
        parts.push("Shift");
    }
    if s.modifiers.alt {
        parts.push("Alt");
    }
    parts.push(s.logical_key.symbol_or_name());
    parts.join("+")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keymap {
    changed: BTreeMap<Action, Option<KeyboardShortcut>>,
}

impl Keymap {
    pub fn get(&self, a: Action) -> Option<KeyboardShortcut> {
        self.changed.get(&a).copied().unwrap_or(Some(a.default_key()))
    }

    pub fn is_default(&self, a: Action) -> bool {
        !self.changed.contains_key(&a)
    }

    pub fn bind(&mut self, a: Action, s: KeyboardShortcut) -> Option<Action> {
        let taken = Action::ALL.into_iter().find(|&o| o != a && self.get(o) == Some(s));
        if let Some(o) = taken {
            self.set(o, None);
        }
        self.set(a, Some(s));
        taken
    }

    pub fn unbind(&mut self, a: Action) {
        self.set(a, None);
    }

    pub fn reset(&mut self, a: Action) {
        self.changed.remove(&a);
    }

    fn set(&mut self, a: Action, s: Option<KeyboardShortcut>) {
        if s == Some(a.default_key()) {
            self.changed.remove(&a);
        } else {
            self.changed.insert(a, s);
        }
    }

    pub fn check_order(&self) -> Vec<Action> {
        let weight = |a: &Action| self.get(*a).map_or(0, |s| u8::from(s.modifiers.shift) + u8::from(s.modifiers.alt) + u8::from(s.modifiers.command));
        let mut order = Action::ALL.to_vec();
        order.sort_by_key(|a| std::cmp::Reverse(weight(a)));
        order
    }

    pub fn to_toml(&self) -> String {
        let mut out = String::from("[keys]\n");
        for (a, s) in &self.changed {
            out.push_str(&std::format!("{} = \"{}\"\n", a.id(), s.as_ref().map(format).unwrap_or_default()));
        }
        out
    }

    pub fn from_toml(text: &str) -> Keymap {
        #[derive(serde::Deserialize, Default)]
        struct File {
            #[serde(default)]
            keys: BTreeMap<String, String>,
        }
        let file: File = toml::from_str(text).unwrap_or_default();
        let mut map = Keymap::default();
        for a in Action::ALL {
            match file.keys.get(a.id()).map(String::as_str) {
                Some("") => map.unbind(a),
                Some(text) => {
                    if let Some(s) = parse(text) {
                        map.set(a, Some(s));
                    }
                }
                None => {}
            }
        }
        map
    }
}
