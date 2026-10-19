use codescape::window_stack::WindowStack;
use egui::{Context, Id, Order, RawInput};

#[derive(Clone, Copy, PartialEq, Debug)]
enum W {
    A,
    B,
}

#[test]
fn newest_window_is_on_top() {
    let mut s = WindowStack::default();
    s.sync(&[W::A], None);
    s.sync(&[W::A, W::B], None);
    assert_eq!(s.pop(), Some(W::B));
    assert_eq!(s.pop(), Some(W::A));
    assert_eq!(s.pop(), None);
}

#[test]
fn raised_window_goes_on_top() {
    let mut s = WindowStack::default();
    s.sync(&[W::A, W::B], Some(W::A));
    assert_eq!(s.pop(), Some(W::A));
}

#[test]
fn window_closed_by_its_button_leaves_the_stack() {
    let mut s = WindowStack::default();
    s.sync(&[W::A, W::B], None);
    s.sync(&[W::A], None);
    assert_eq!(s.pop(), Some(W::A));
    assert_eq!(s.pop(), None);
}

fn top_layer_after_showing(configure: impl Fn(egui::Window<'static>) -> egui::Window<'static>) -> Option<Id> {
    let ctx = Context::default();
    let mut top = None;
    for _ in 0..2 {
        let _ = ctx.run_ui(RawInput::default(), |ui| {
            configure(egui::Window::new("Shortcuts")).show(ui.ctx(), |ui| {
                ui.label("x");
            });
            top = ui.ctx().memory(|m| m.areas().top_layer_id(Order::Middle)).map(|l| l.id);
        });
    }
    top
}

#[test]
fn a_window_without_an_explicit_id_is_not_found_by_a_unique_id() {
    let top = top_layer_after_showing(|w| w);
    assert_ne!(top, Some(Id::unique("Shortcuts")));
}

#[test]
fn a_window_with_an_explicit_id_is_the_top_layer_under_that_id() {
    let id = Id::unique("Shortcuts");
    let top = top_layer_after_showing(move |w| w.id(id));
    assert_eq!(top, Some(id));
}

#[test]
fn esc_closes_a_window_then_the_diagram_then_the_file_then_the_selection() {
    use codescape::window_stack::{Escape, escape};
    assert_eq!(escape(true, true, true), Escape::CloseWindow);
    assert_eq!(escape(false, true, true), Escape::CloseSequence);
    assert_eq!(escape(false, false, true), Escape::LeaveReading);
    assert_eq!(escape(false, false, false), Escape::ClearSelection);
}
