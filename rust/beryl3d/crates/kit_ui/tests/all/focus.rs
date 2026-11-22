//! The focus order: Tab and Shift+Tab move the focus in tree order, and stay inside a sheet or
//! menu that holds the focus.

use kit_ui::element::{Element, column, row};
use kit_ui::layout::Layer;
use kit_ui::widgets::button;

/// A button named `name` whose press says its name.
fn pressable(name: &str) -> Element {
    button(name).name(name).on_press(name.to_owned())
}

/// The name the focused element's press says, as `pressable` gives it.
fn focused(layer: &Layer) -> Option<String> {
    let press = layer.focused_press()?;
    press.downcast_ref::<String>().cloned()
}

/// Presses Tab, or Shift+Tab if not `forward`, `times` times, and returns each element the
/// focus went to.
fn tab(layer: &mut Layer, forward: bool, times: usize) -> Vec<String> {
    (0..times)
        .map(|_| {
            layer.move_focus(forward);
            focused(layer).unwrap_or_default()
        })
        .collect()
}

fn screen(sheet: Option<Element>) -> Element {
    column()
        .name("screen")
        .child(pressable("one"))
        .child(
            row()
                .name("row")
                .child(pressable("two"))
                .child(pressable("three")),
        )
        .children(sheet)
}

#[test]
fn tab_moves_the_focus_in_tree_order_and_goes_round() {
    let mut layer = Layer::new();
    layer.show(Some(screen(None)));
    assert_eq!(tab(&mut layer, true, 4), ["one", "two", "three", "one"]);
    assert_eq!(tab(&mut layer, false, 2), ["three", "two"]);
}

#[test]
fn a_disabled_button_is_left_out() {
    let mut layer = Layer::new();
    let screen = column()
        .name("screen")
        .child(pressable("one"))
        .child(pressable("off").enabled(false))
        .child(pressable("two"));
    layer.show(Some(screen));
    assert_eq!(tab(&mut layer, true, 3), ["one", "two", "one"]);
}

#[test]
fn a_sheet_that_holds_the_focus_keeps_tab_inside_it() {
    let mut layer = Layer::new();
    layer.show(Some(screen(None)));
    assert_eq!(tab(&mut layer, true, 1), ["one"]);
    let sheet = column()
        .name("sheet")
        .holds_focus()
        .child(pressable("save"))
        .child(pressable("close"));
    layer.show(Some(screen(Some(sheet))));
    // The focus was outside the sheet, so Tab goes to its first element and then stays inside.
    assert_eq!(tab(&mut layer, true, 3), ["save", "close", "save"]);
    assert_eq!(tab(&mut layer, false, 2), ["close", "save"]);
    // Once the sheet closes, Tab moves through the whole screen again.
    layer.show(Some(screen(None)));
    assert_eq!(tab(&mut layer, true, 3), ["one", "two", "three"]);
}
