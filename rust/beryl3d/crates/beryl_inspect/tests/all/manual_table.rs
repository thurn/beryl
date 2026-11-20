//! The manual table's selection and its drags to a library, on `screen/table` (the hand-built
//! table of `beryl_inspect::screens`), driven through the same actions as `just b3d tree --do`:
//! a sweep from the empty table draws a rectangle and selects the permanents it touches, a
//! summary stands over them with their count and total power and toughness, a drag held over a
//! library shows its Top and Bottom halves, and the selected cards dropped on the Top half open
//! the order tray before anything moves. Pointing at a face-down card shows its face only to a
//! viewer who knows it.

use beryl_inspect::pages;
use beryl_protocol::protocol::{Side, ZoneName};
use beryl_scene::agent::Layout;
use beryl_scene::driver::Page;
use beryl_scene::frame::{Place, Resolved, Role, StateBits};
use beryl_scene::input::Size;
use glam::Vec2;

/// A sweep from the empty table above your Serra Angel to below your Pacifism, which touches
/// those two permanents and no land.
const SWEEP: &str = "drag 560,410 to 1000,600";

/// `screen/table` at rest, at its end.
fn table() -> Page {
    let size = Size {
        width: 1600.0,
        height: 900.0,
    };
    let mut page = pages::page("screen/table", Side::You, Layout::Desktop, size)
        .unwrap_or_else(|why| panic!("{why}"));
    page.seek(&"end".parse().expect("end is a moment"))
        .unwrap_or_else(|why| panic!("{why}"));
    page.still();
    page
}

/// The text of each text element directly inside the element `within` names, in drawing order.
fn texts(page: &mut Page, within: &str) -> Vec<String> {
    let parent = page.find(within).index;
    let frame = page.frame();
    (frame.resolved.iter())
        .filter(|element| element.parent == parent && element.role == Role::Text)
        .map(|element| frame.name(element.name).to_owned())
        .collect()
}

/// Your permanent named `name` in the present frame. Both players have the same cards, so a
/// locator by role and name alone would match two.
fn yours(page: &mut Page, name: &str) -> Resolved {
    let frame = page.frame();
    *(frame.resolved.iter())
        .find(|element| {
            element.role == Role::Card
                && frame.name(element.name) == name
                && matches!(
                    element.place,
                    Place::Card {
                        zone: ZoneName::Battlefield,
                        side: Side::You,
                        ..
                    }
                )
        })
        .unwrap_or_else(|| panic!("you have no {name} on the battlefield"))
}

/// The centre of your permanent named `name`, as an action's point `<x>,<y>`.
fn at(page: &mut Page, name: &str) -> String {
    let centre = yours(page, name).corners.iter().copied().sum::<Vec2>() / 4.0;
    format!("{},{}", centre.x, centre.y)
}

/// Whether your permanent named `name` wears the `selected` state.
fn selected(page: &mut Page, name: &str) -> bool {
    yours(page, name).state.0 & StateBits::SELECTED != 0
}

/// The highest `y` and the middle `x` of your permanent named `name`'s box.
fn extent(page: &mut Page, name: &str) -> (f32, f32) {
    let corners = yours(page, name).corners;
    let top = (corners.iter()).fold(f32::INFINITY, |top, corner| top.min(corner.y));
    let middle = corners.iter().map(|corner| corner.x).sum::<f32>() / 4.0;
    (top, middle)
}

#[test]
fn a_sweep_from_the_empty_table_draws_a_rectangle_and_selects_what_it_touches() {
    let mut page = table();
    page.run("hold 560,410 over 1000,600");
    assert!(
        page.try_find("region \"selection-rectangle\"").is_ok(),
        "no rectangle mid-sweep:\n{}",
        page.tree()
    );
    let mut page = table();
    page.run(&format!("{SWEEP}\nstill"));
    for card in ["Serra Angel", "Pacifism"] {
        assert!(selected(&mut page, card), "your {card} is not selected");
    }
    for card in ["Island", "Plains"] {
        assert!(!selected(&mut page, card), "your {card} is selected");
    }
    assert!(page.try_find("region \"selection-rectangle\"").is_err());
    assert_eq!(texts(&mut page, "region \"Selection\""), ["2 cards", "4/4"]);
}

#[test]
fn the_summary_stands_over_the_highest_selected_permanent() {
    let mut page = table();
    page.run(&format!("{SWEEP}\nstill"));
    let summary = page.find("region \"Selection\"").rect;
    let (top, centre) = extent(&mut page, "Serra Angel");
    let middle = summary.x + summary.width / 2.0;
    assert!(
        (middle - centre).abs() < 1.0,
        "the summary is centred at {middle}, not over Serra Angel at {centre}"
    );
    assert!(
        summary.y + summary.height <= top,
        "the summary {summary:?} is not above Serra Angel's top at {top}"
    );
}

#[test]
fn the_selection_dropped_on_a_librarys_top_opens_the_order_tray() {
    let mut page = table();
    page.run(&format!("{SWEEP}\nstill"));
    let angel = at(&mut page, "Serra Angel");
    page.run(&format!("hold {angel} over pile \"Your library\""));
    for half in ["text \"Top\"", "text \"Bottom\""] {
        assert!(page.try_find(half).is_ok(), "no {half}:\n{}", page.tree());
    }
    let top = page.find("region \"Top\"").rect;
    let top = Vec2::new(top.x + top.width / 2.0, top.y + top.height / 2.0);

    let mut page = table();
    page.run(&format!("{SWEEP}\nstill"));
    page.run(&format!("drag {angel} to {},{}\nstill", top.x, top.y));
    assert!(
        page.try_find("text \"Order Cards\"").is_ok(),
        "no order tray:\n{}",
        page.tree()
    );
    assert!(
        page.try_find("region \"Selection\"").is_err(),
        "the summary shows over the order tray"
    );
}

/// The face-down card on `side`'s battlefield, as an action's point `<x>,<y>`.
fn face_down(page: &mut Page, side: Side) -> String {
    let frame = page.frame();
    let card = (frame.resolved.iter())
        .find(|element| {
            element.role == Role::Card
                && element.state.has(StateBits::FACE_DOWN)
                && matches!(
                    element.place,
                    Place::Card {
                        zone: ZoneName::Battlefield,
                        side: placed,
                        ..
                    } if placed == side
                )
        })
        .unwrap_or_else(|| panic!("{side:?} has no face-down card on the battlefield"));
    let centre = card.corners.iter().copied().sum::<Vec2>() / 4.0;
    format!("{},{}", centre.x, centre.y)
}

/// UI Conventions, "Inspecting a face-down manual card": "Pointing at the card opens a face
/// preview only if the viewer knows the card's identity." The viewer's own face-down card is
/// named in their view; the opponent's is not.
#[test]
fn pointing_at_a_face_down_card_shows_its_face_only_to_a_viewer_who_knows_it() {
    let mut page = table();
    let yours = face_down(&mut page, Side::You);
    page.run(&format!("move {yours}\nstill"));
    assert!(
        page.try_find("region \"Serra Angel, inspected\"").is_ok(),
        "your face-down Serra Angel shows no face:\n{}",
        page.tree()
    );
    let mut page = table();
    let theirs = face_down(&mut page, Side::Opponent);
    page.run(&format!("move {theirs}\nstill"));
    let tree = page.tree();
    assert!(
        !tree.contains(", inspected\""),
        "the opponent's face-down card shows a face:\n{tree}"
    );
}
