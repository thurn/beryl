//! The text field, driven through the `Page` test driver: the bug report's description field,
//! edited in place with typing, keys, the clipboard and an input method.

use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::Side;
use beryl_scene::Input;
use beryl_scene::agent::{Layout, Rule};
use beryl_scene::command::Request;
use beryl_scene::driver::{Duration, Page, Recording};
use beryl_scene::frame::Material;
use beryl_scene::input::{
    Button, KeyCode, Modifiers, Pointer, PointerKind, PointerPhase, Size, TextChange,
};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder};
use beryl_scene::tokens::color;
use glam::Vec2;

const FIELD: &str = "field \"description\"";

/// The sample table with the bug report describing the whole window, whose description field
/// is empty.
fn bug_report() -> Page {
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    let recording = Recording(vec![FixtureMessage::game_update(&view.update())]);
    let size = Size {
        width: 1600.0,
        height: 900.0,
    };
    let mut page = Page::new(recording.into(), Side::You, Layout::Desktop, size).unwrap();
    page.still();
    page.chord("Cmd+B");
    // The table freezes for its picture, then Space takes the whole window and the sheet
    // fades in.
    page.still();
    page.key(KeyCode::Space);
    page.still();
    page
}

/// Every request the client has made since the last call.
fn requests(page: &mut Page) -> Vec<Request> {
    std::iter::from_fn(|| page.next_request()).collect()
}

/// How many of the flat layer's panels are drawn in the color of type: the caret and the
/// underline of composing text. The text's glyphs, drawn in that color too, are not panels.
fn type_marks(page: &mut Page) -> usize {
    let frame = page.frame();
    (frame.draws[frame.layer as usize..].iter())
        .filter(|draw| draw.material == Material::Panel as u8 && draw.tint == color::TYPE)
        .count()
}

#[test]
fn backspace_after_moving_left_deletes_the_letter_before_the_caret() {
    let mut page = bug_report();
    page.type_text(FIELD, "abc");
    page.key(KeyCode::ArrowLeft);
    page.key(KeyCode::ArrowLeft);
    page.key(KeyCode::Backspace);
    let field = page.field("description");
    assert_eq!(field.text, "bc");
    assert_eq!(field.caret, 0, "the caret is at the text's start");
    assert_eq!(field.selection, None);
}

#[test]
fn cmd_x_copies_the_selection_and_leaves_the_rest() {
    let mut page = bug_report();
    page.type_text(FIELD, "abc");
    page.chord("Shift+ArrowLeft");
    page.chord("Shift+ArrowLeft");
    assert_eq!(page.field("description").selection, Some(1..3));
    requests(&mut page);
    page.chord("Cmd+X");
    assert_eq!(requests(&mut page), vec![Request::Copy("bc".to_owned())]);
    assert_eq!(page.field("description").text, "a");
}

#[test]
fn a_paste_inserts_the_clipboard_at_the_caret() {
    let mut page = bug_report();
    page.type_text(FIELD, "ab");
    page.key(KeyCode::ArrowLeft);
    requests(&mut page);
    page.chord("Cmd+V");
    assert_eq!(requests(&mut page), vec![Request::Paste]);
    page.give_input(Input::Paste("xy".to_owned()));
    let field = page.field("description");
    assert_eq!(field.text, "axyb");
    assert_eq!(field.caret, 3, "the caret is after the pasted text");
}

#[test]
fn composing_text_is_underlined_and_joins_the_text_when_committed() {
    let mut page = bug_report();
    page.type_text(FIELD, "a");
    assert_eq!(type_marks(&mut page), 1, "the caret");
    page.give_input(Input::Text(TextChange::Preedit("ka".to_owned())));
    assert_eq!(type_marks(&mut page), 2, "the caret and the underline");
    assert_eq!(
        page.field("description").text,
        "a",
        "not yet part of the text"
    );
    page.give_input(Input::Text(TextChange::Insert("か".to_owned())));
    assert_eq!(page.field("description").text, "aか");
    assert_eq!(type_marks(&mut page), 1, "the underline is gone");
}

#[test]
fn the_field_focused_as_the_form_opens_begins_text_entry_over_it() {
    let mut page = bug_report();
    let field = page.find(FIELD);
    let begun = requests(&mut page)
        .into_iter()
        .find_map(|request| match request {
            Request::BeginTextEntry { rect, text, .. } => Some((rect, text)),
            _ => None,
        });
    let (rect, text) = begun.expect("text entry begins");
    assert_eq!(text, "");
    assert!(
        (rect.x - field.rect().x).abs() < 0.01 && (rect.width - field.rect().width).abs() < 0.01,
        "over the field: {rect:?} and {:?}",
        field.rect()
    );
}

/// The primary button going `phase` at `point`.
fn mouse(phase: PointerPhase, point: Vec2) -> Input {
    Input::Pointer(Pointer {
        id: 0,
        kind: PointerKind::Mouse,
        phase,
        position: point,
        button: Button::Primary,
        wheel: Vec2::ZERO,
        modifiers: Modifiers::default(),
    })
}

/// A point on the first line of the field's text, `x` points in from its start: inside the
/// field's padding of 12 points at the side and 10 above.
fn on_text(page: &mut Page, x: f32) -> Vec2 {
    let field = page.find(FIELD).rect();
    Vec2::new(field.x + 12.0 + x, field.y + 10.0 + 8.0)
}

#[test]
fn a_double_click_selects_a_word_and_a_drag_selects_what_it_crosses() {
    let mut page = bug_report();
    page.type_text(FIELD, "one two");
    let start = on_text(&mut page, 2.0);
    for _ in 0..2 {
        page.give_input(mouse(PointerPhase::Down, start));
        page.give_input(mouse(PointerPhase::Up, start));
    }
    assert_eq!(page.field("description").selection, Some(0..3), "the word");
    page.step(Duration::from_millis(600));
    page.give_input(mouse(PointerPhase::Down, start));
    let end = on_text(&mut page, 400.0);
    page.give_input(mouse(PointerPhase::Move, end));
    page.give_input(mouse(PointerPhase::Up, end));
    let field = page.field("description");
    assert_eq!(
        field.selection,
        Some(0..7),
        "from the word's start to the end"
    );
}

#[test]
fn home_end_and_select_all() {
    let mut page = bug_report();
    page.type_text(FIELD, "one two");
    page.key(KeyCode::Home);
    assert_eq!(page.field("description").caret, 0);
    page.chord("Shift+End");
    assert_eq!(page.field("description").selection, Some(0..7));
    page.chord("Cmd+A");
    page.key(KeyCode::Delete);
    assert_eq!(page.field("description").text, "");
}

#[test]
fn a_field_is_named_and_its_text_fits() {
    let mut page = bug_report();
    page.type_text(
        FIELD,
        "The opponent's Opt never resolved after I passed priority.",
    );
    let faults: Vec<String> = (page.faults().iter())
        .filter(|fault| matches!(fault.rule, Rule::Named | Rule::TextFits))
        .map(|fault| fault.message.clone())
        .collect();
    assert_eq!(faults, Vec::<String>::new());
}
