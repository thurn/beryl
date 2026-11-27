//! The phone's hand and stack scrolling (docs/beryl-3d.md, "Objects and placement"): a hand too
//! large for the fans that fit under the top of the window, or a stack too long for its one row,
//! shows as many cards as keep a strip to press, counts the rest off each end, and scrolls under
//! the wheel and a drag as a battlefield row does.

use app_client::driver::{Duration, Page, Recording};
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{Side, ZoneName};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use game_table::table::Place;
use glam::Vec2;
use kit_frame::frame::{Frame, Role};
use kit_probe::agent::checks::on_screen;
use kit_probe::agent::{Context, Fault, Rule};
use kit_scene::hit::uncovered;
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Insets, Size};

const SMALL: Size = Size {
    width: 375.0,
    height: 647.0,
};

const MIDDLE: Size = Size {
    width: 393.0,
    height: 759.0,
};

const LARGE: Size = Size {
    width: 430.0,
    height: 932.0,
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
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        size,
    )
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
                && matches!(Place::in_frame(element.place), Some(Place::Card { zone: at, side: Side::You, .. }) if at == zone)
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

/// How many cards `line` has scrolled off its `end` ("left" or "right"), if any.
fn count(page: &mut Page, line: &str, end: &str) -> Option<usize> {
    page.frame();
    let scroller = match line {
        "Your hand" => game_table::table::Scroller::Hand,
        "Stack" => game_table::table::Scroller::Stack,
        _ => panic!("no line {line:?}"),
    };
    let window = page.table().row_window(scroller);
    let off = if end == "left" {
        window.before()
    } else {
        window.after()
    };
    (off > 0).then_some(off)
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
        Input::Device(DeviceInput::Pointer(Pointer {
            id: 1,
            kind: PointerKind::Touch,
            phase,
            position,
            button: Button::Primary,
            wheel: Vec2::ZERO,
            modifiers: Modifiers::default(),
        }))
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

fn on_screen_card_faults(page: &mut Page) -> Vec<Fault> {
    let data = page.data().clone();
    let profile = data.profiles.for_size(page.size());
    let context = Context {
        size_class: profile.size_class,
        checks: profile.checks,
        audit: data.audit,
        size: page.size(),
        safe_area: Insets::default(),
        at: page.now(),
    };
    on_screen(page.frame(), &context)
        .into_iter()
        .filter(|fault| {
            fault
                .elements
                .iter()
                .any(|subject| subject.locator.starts_with("card "))
        })
        .collect()
}

#[test]
fn a_raised_hand_card_stays_within_the_safe_area_sides_on_phones() {
    for size in [SMALL, MIDDLE, LARGE] {
        let mut page = page(&table(ZoneName::Hand, "Card", 12), size);
        let in_view = drawn(page.frame(), ZoneName::Hand).len();
        for index in [0, in_view - 1] {
            let point = on_card(&mut page, ZoneName::Hand, index);
            page.move_to(point);
            page.still();
            let faults = on_screen_card_faults(&mut page);
            assert!(
                faults.is_empty(),
                "raised card {index} in {}x{} has on-screen faults: {faults:?}",
                size.width,
                size.height
            );
        }
    }
}
