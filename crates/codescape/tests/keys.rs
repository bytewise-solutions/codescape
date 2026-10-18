use codescape::keys::{Action, Keymap, format, parse};
use egui::{Key, KeyboardShortcut, Modifiers};

const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };

#[test]
fn shortcuts_read_and_write_as_text() {
    assert_eq!(parse("Ctrl+Shift+O"), Some(KeyboardShortcut::new(CTRL_SHIFT, Key::O)));
    assert_eq!(parse("shift+f3"), Some(KeyboardShortcut::new(Modifiers::SHIFT, Key::F3)));
    assert_eq!(parse("F11"), Some(KeyboardShortcut::new(Modifiers::NONE, Key::F11)));
    assert_eq!(parse("Ctrl+Nope"), None);
    assert_eq!(parse(""), None);
    for text in ["Ctrl+Shift+O", "Alt+F3", "F3", "Ctrl+,"] {
        assert_eq!(format(&parse(text).unwrap()), text);
    }
}

#[test]
fn defaults_keep_ctrl_f12_free_and_step_matches_with_f3() {
    let k = Keymap::default();
    assert_eq!(k.get(Action::Structure), parse("Ctrl+Shift+O"));
    assert_eq!(k.get(Action::FindNext), parse("F3"));
    assert_eq!(k.get(Action::FindPrevious), parse("Shift+F3"));
    assert!(Action::ALL.iter().all(|a| k.get(*a).is_some()), "every action has a default key");
}

#[test]
fn rebinding_takes_the_keys_away_from_the_action_that_had_them() {
    let mut k = Keymap::default();
    let taken = k.bind(Action::Structure, parse("Ctrl+K").unwrap());
    assert_eq!(taken, Some(Action::Jump));
    assert_eq!(k.get(Action::Structure), parse("Ctrl+K"));
    assert_eq!(k.get(Action::Jump), None);
    k.reset(Action::Jump);
    assert_eq!(k.get(Action::Jump), parse("Ctrl+K"));
}

#[test]
fn only_changes_are_saved_and_unknown_lines_are_ignored() {
    let mut k = Keymap::default();
    k.bind(Action::Structure, parse("Alt+S").unwrap());
    let text = k.to_toml();
    assert!(text.contains("structure = \"Alt+S\""), "{text}");
    assert!(!text.contains("jump"), "defaults are not written: {text}");
    let back = Keymap::from_toml(&format!("{text}\nnot_an_action = \"F1\"\n"));
    assert_eq!(back.get(Action::Structure), parse("Alt+S"));
    assert_eq!(back.get(Action::Jump), parse("Ctrl+K"));
    assert_eq!(Keymap::from_toml("[keys]\njump = \"\"\n").get(Action::Jump), None, "an empty string unbinds");
}

#[test]
fn a_shortcut_with_more_modifiers_is_checked_before_the_one_it_contains() {
    let k = Keymap::default();
    let order = k.check_order();
    let at = |a: Action| order.iter().position(|&x| x == a).unwrap();
    assert!(at(Action::FindPrevious) < at(Action::FindNext));
    assert!(at(Action::Structure) < at(Action::OpenFolder));
}
