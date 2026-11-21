//! The phone's hand and stack scrolling (docs/beryl-3d.md, "Objects and placement"): a hand too
//! large for the fans that fit under the top of the window, or a stack too long for its one row,
//! shows as many cards as keep a strip to press, counts the rest off each end, and scrolls under
//! the wheel and a drag as a battlefield row does.

use app_client::driver::{Duration, Page, Recording};
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{Side, ZoneName};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use glam::Vec2;
use kit_frame::frame::{Frame, Place, Role};
use kit_probe::agent::{Layout, Rule};
use kit_scene::hit::uncovered;
use kit_ui::input::Size;

const SMALL: Size = Size {
    width: 375.0,
    height: 647.0,
};

const MIDDLE: Size = Size {
    width: 393.0,
    height: 759.0,
};

/// One notch of a wheel turned down, toward a line's right end.
const NOTCH_DOWN: Vec2 = Vec2::new(0.0, -16.0);

/// A table whose viewer has `count` cards named "`name` 0" onward in `zone`.
fn table(zone: ZoneName, name: &str, count: usize) -> ViewBuilder {
    let mut view = ViewBuilder::new();
    for index in 0..count {
        view.card(zone, Side::You, &format!("{name} {index}"));
    }
    view.script(ScriptBuilder::reset().build());
    view
}

fn page(view: &ViewBuilder, size: Size) -> Page {
    let messages = vec![FixtureMessage::game_update(&view.update())];
    let mut page = Page::new(Recording(messages).into(), Side::You, Layout::Phone, size)
        .expect("the page opens");
    page.still();
    page
}

/// The numbers of the viewer's cards in `zone` drawn in `frame`, in order of their numbers,
/// each with its element's index.
fn drawn(frame: &Frame, zone: ZoneName) -> Vec<(usize, u32)> {
    let mut cards: Vec<(usize, u32)> = (frame.resolved.iter().enumerate())
        .filter(|(_, element)| {
            element.role == Role::Card
                && element.opacity > 0.0
                && matches!(element.place, Place::Card { zone: at, side: Side::You, .. } if at == zone)
        })
        .map(|(index, element)| {
            let name = frame.name(element.name);
            let number = name.rsplit(' ').next().and_then(|n| n.parse().ok());
            (
                number.expect("a card's number"),
                u32::try_from(index).expect("few elements"),
            )
        })
        .collect();
    cards.sort_unstable();
    cards
}

/// The count shown off `line`'s `end` ("left" or "right"), if any.
fn count(page: &mut Page, line: &str, end: &str) -> Option<usize> {
    let prefix = format!("{line} {end} ");
    let frame = page.frame();
    frame
        .resolved
        .iter()
        .find_map(|element| frame.name(element.name).strip_prefix(&prefix)?.parse().ok())
}

/// The middle of the uncovered part of the card drawn at `at`, counting from the lowest number.
fn on_card(page: &mut Page, zone: ZoneName, at: usize) -> Vec2 {
    let frame = page.frame();
    let index = drawn(frame, zone)[at].1;
    let rect = uncovered(frame, index).expect("part of the card is uncovered");
    Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

fn unreachable(page: &mut Page) -> usize {
    (page.faults().iter())
        .filter(|fault| fault.rule == Rule::Reachable)
        .count()
}

#[test]
fn a_hand_too_large_for_its_fans_shows_what_fits_and_scrolls_under_a_sideways_drag() {
    let mut page = page(&table(ZoneName::Hand, "Card", 60), SMALL);
    let shown = drawn(page.frame(), ZoneName::Hand);
    assert!(shown.len() < 60, "{} of 60 cards in view", shown.len());
    assert_eq!(shown[0].0, 0, "the hand starts at its left end");
    assert_eq!(
        count(&mut page, "Your hand", "right"),
        Some(60 - shown.len())
    );
    assert_eq!(count(&mut page, "Your hand", "left"), None);
    assert_eq!(unreachable(&mut page), 0);
    // A drag up the hand, as one that plays a card, does not scroll it.
    let from = on_card(&mut page, ZoneName::Hand, 3);
    page.drag_between(from, from - Vec2::new(10.0, 120.0));
    page.still();
    assert_eq!(count(&mut page, "Your hand", "left"), None);
    // A drag to the left brings in the cards off the right end.
    let from = on_card(&mut page, ZoneName::Hand, 3);
    page.drag_between(from, from - Vec2::new(200.0, 0.0));
    page.still();
    let left = count(&mut page, "Your hand", "left").expect("cards off the left end");
    assert_eq!(drawn(page.frame(), ZoneName::Hand)[0].0, left);
    assert_eq!(unreachable(&mut page), 0);
}

#[test]
fn a_long_stack_shows_its_top_items_in_one_row_and_scrolls_under_the_wheel() {
    let mut page = page(&table(ZoneName::Stack, "Spell", 20), MIDDLE);
    let shown = drawn(page.frame(), ZoneName::Stack);
    assert!(shown.len() < 20, "{} of 20 items in view", shown.len());
    assert_eq!(
        shown.last().map(|card| card.0),
        Some(19),
        "the top item is in view"
    );
    assert_eq!(count(&mut page, "Stack", "left"), Some(20 - shown.len()));
    assert_eq!(count(&mut page, "Stack", "right"), None);
    assert_eq!(unreachable(&mut page), 0);
    // A wheel turned up scrolls toward the bottom of the stack, at its left.
    let point = on_card(&mut page, ZoneName::Stack, 0);
    page.wheel(point, -NOTCH_DOWN * 2.0);
    page.still();
    assert_eq!(count(&mut page, "Stack", "right"), Some(2));
    assert_eq!(
        count(&mut page, "Stack", "left"),
        Some(20 - shown.len() - 2)
    );
}

#[test]
fn a_touch_fling_advances_the_group_window_after_release() {
    use app_client::Input;
    use kit_ui::input::{Button, Modifiers, Pointer, PointerKind, PointerPhase};

    let mut page = page(&table(ZoneName::Hand, "Card", 60), SMALL);
    let start = on_card(&mut page, ZoneName::Hand, 3);
    let end = start - Vec2::new(70.0, 0.0);
    let finger = |phase, position| {
        Input::Pointer(Pointer {
            id: 1,
            kind: PointerKind::Touch,
            phase,
            position,
            button: Button::Primary,
            wheel: Vec2::ZERO,
            modifiers: Modifiers::default(),
        })
    };
    page.give_input(finger(PointerPhase::Down, start));
    page.step(Duration::from_millis(32));
    page.give_input(finger(PointerPhase::Move, end));
    page.give_input(finger(PointerPhase::Up, end));
    assert_eq!(count(&mut page, "Your hand", "left"), Some(1));

    page.still();
    let left = count(&mut page, "Your hand", "left").expect("cards off the left end");
    assert!(
        left > 1,
        "the window coasted past the group reached by the drag"
    );
    assert_eq!(drawn(page.frame(), ZoneName::Hand)[0].0, left);
    assert_eq!(unreachable(&mut page), 0);
}
