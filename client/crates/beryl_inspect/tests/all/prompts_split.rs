//! The prompt shapes `Split` and the pile of identical tokens on their recordings
//! (docs/beryl-3d.md, "The flat layer"): a split's two piles held up in boxes under the session's
//! names for them, a card clicked to the other pile or carried into one, and a click on a pile's
//! top token. Each test plays the recording to a prompt, acts as the player did, and lets the
//! driver answer each click with the session's recorded reply, which fails the test for any
//! click the player did not make: a drop into another pile, or before another card, among them.

use std::str::FromStr;

use app_client::driver::{Duration, Moment, Page};
use beryl_inspect::pages::page;
use beryl_protocol::protocol::{Side, ZoneName};
use game_table::table::Place;
use glam::Vec2;
use kit_base::data::profiles::SizeClass;
use kit_frame::frame::{ElementId, Role, StateBits};
use kit_ui::input::{Rect, Size};

const SIZE: Size = Size {
    width: 1600.0,
    height: 900.0,
};

/// `name` played from the viewer's seat to `prompt:<view>`, when the table has come to rest
/// under it, and on for as long as its own marks and tray take to come up. The recorded answer
/// to that prompt is held back, so that the test answers it.
fn at_prompt(name: &str, view: usize) -> Page {
    let mut page =
        page(name, Side::You, SizeClass::Regular, SIZE).unwrap_or_else(|why| panic!("{why}"));
    page.set_reply_delay(view, Duration::from_millis(60_000));
    let at = format!("prompt:{view}");
    let moment = Moment::from_str(&at).unwrap_or_else(|why| panic!("{at}: {why:?}"));
    page.seek(&moment).unwrap_or_else(|why| panic!("{why}"));
    page.step(Duration::from_millis(400));
    page
}

/// Whether `inner`'s centre lies inside `outer`.
fn inside(inner: Rect, outer: Rect) -> bool {
    let centre = Vec2::new(inner.x + inner.width / 2.0, inner.y + inner.height / 2.0);
    (outer.x..=outer.x + outer.width).contains(&centre.x)
        && (outer.y..=outer.y + outer.height).contains(&centre.y)
}

/// The middle of `rect`.
fn middle(rect: Rect) -> Vec2 {
    Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

/// Carries the card `card` into the box `pile` and lets it go at the box's left edge, before
/// every card in it, then waits for the reply to be drawn.
fn drop_first(page: &mut Page, card: &str, pile: &str) {
    let from = middle(page.find(card).rect());
    let to = page.find(pile).rect();
    page.drag_between(from, Vec2::new(to.x + 4.0, to.y + to.height / 2.0));
    page.step(Duration::from_millis(600));
}

/// `shape-split`'s third prompt, at its third view, is a scry 2 of a Plains, #4, and an Island,
/// #16: the session's two piles are "BOTTOM" and "TOP", and both cards open in the second.
const SCRY: usize = 3;

#[test]
fn a_splits_two_pile_boxes_carry_the_sessions_labels_round_their_cards() {
    let mut page = at_prompt("shape-split", SCRY);
    let bottom = page.find("region \"BOTTOM\"").rect();
    let top = page.find("region \"TOP\"").rect();
    assert!(
        bottom.x + bottom.width <= top.x,
        "the first pile's box lies left of the second's: {bottom:?} and {top:?}"
    );
    for card in ["card #4", "card #16"] {
        let rect = page.find(card).rect();
        assert!(
            inside(rect, top),
            "{card} is held up in TOP: {rect:?} in {top:?}"
        );
    }
}

#[test]
fn a_card_clicked_goes_to_the_other_pile_and_one_carried_is_dropped_where_it_lands() {
    let mut page = at_prompt("shape-split", SCRY);
    let bottom = page.find("region \"BOTTOM\"").rect();
    let top = page.find("region \"TOP\"").rect();
    // As the player did: a click on the Plains sends it to the bottom.
    page.press("card #4");
    page.step(Duration::from_millis(600));
    let plains = page.find("card #4").rect();
    assert!(
        inside(plains, bottom),
        "the reply puts it in BOTTOM: {plains:?}"
    );
    // Then the Plains carried back to the top, before the Island: `Click::Drop` into pile 1 at
    // 0, the only drop the recording answers there.
    drop_first(&mut page, "card #4", "region \"TOP\"");
    let plains = page.find("card #4").rect();
    let island = page.find("card #16").rect();
    assert!(
        inside(plains, top),
        "the drop put it back in TOP: {plains:?}"
    );
    assert!(
        plains.x < island.x,
        "before the Island: {plains:?} and {island:?}"
    );
    // And the Island carried to the bottom: pile 0 at 0.
    drop_first(&mut page, "card #16", "region \"BOTTOM\"");
    let island = page.find("card #16").rect();
    assert!(
        inside(island, bottom),
        "the drop put it in BOTTOM: {island:?}"
    );
}

#[test]
fn a_card_let_go_outside_both_boxes_goes_back_and_sends_nothing() {
    let mut page = at_prompt("shape-split", SCRY);
    let before = page.find("card #4").rect();
    // Over the table below the tray: the driver would fail on any click it sent.
    page.drag_between(middle(before), Vec2::new(200.0, 760.0));
    page.step(Duration::from_millis(600));
    let after = page.find("card #4").rect();
    assert!(
        middle(after).distance(middle(before)) < 1.0,
        "it fell back to its place in the tray: {before:?} then {after:?}"
    );
}

#[test]
fn a_click_on_a_pile_of_identical_tokens_is_on_its_top_token() {
    // `shape-pile`'s ninth prompt, at its ninth view, is a priority with two Map tokens, #512
    // and #513, on the battlefield. The token on top of a pile of them is the one with the
    // lower slot, the one the session offers, and the recording holds the player's click on it
    // and no other.
    let mut page = at_prompt("shape-pile", 9);
    page.press("card #512");
    page.step(Duration::from_millis(600));
    assert!(
        page.try_find("ability \"Map Token\"").is_ok(),
        "the reply puts the token's ability on the stack"
    );
}

/// `shape-split`'s fifth prompt is Stock Up: of the five cards looked at, put two into the hand
/// and the rest on the bottom. The session's piles are "Hand" and "Bottom", and the five cards
/// open in "Bottom": the Plains #4, Mana Drain #10, Oust #20, Damn #19 and Reprieve #5.
const STOCK_UP: usize = 5;

#[test]
fn a_look_that_takes_cards_out_of_the_library_holds_them_in_one_fan_with_no_boxes() {
    let mut page = at_prompt("shape-split", STOCK_UP);
    for pile in ["region \"Hand\"", "region \"Bottom\""] {
        assert!(page.try_find(pile).is_err(), "no box for {pile}");
    }
    let cards = ["#4", "#10", "#20", "#19", "#5"].map(|slot| {
        let locator = format!("card {slot}");
        (page.find(&locator).rect(), locator)
    });
    let mut by_x = cards.clone();
    by_x.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
    for pair in by_x.windows(2) {
        let ((left, a), (right, b)) = (&pair[0], &pair[1]);
        assert!(
            right.x < left.x + left.width,
            "{b} overlaps {a}, its neighbor in the fan: {left:?} and {right:?}"
        );
        assert!(
            (middle(*right).y - middle(*left).y).abs() < left.height / 4.0,
            "the fan lies in one row: {left:?} and {right:?}"
        );
    }
    // Clicked, a card is picked where it lies: the fan keeps its order.
    page.press("card #10");
    page.step(Duration::from_millis(600));
    for (before, locator) in &cards {
        let after = page.find(locator).rect();
        assert!(
            middle(after).distance(middle(*before)) < 1.0,
            "{locator} stays in its place in the fan: {before:?} then {after:?}"
        );
    }
}

/// `shape-split`'s seventh prompt is Jace's Brainstorm: put two cards from the hand on top of
/// the library. The session's piles are "Top", empty, and "Hand", which holds the six cards in
/// hand: Counterspell #3, Tundra #6, Counterspell #9, Mana Drain #10, Island #17 and Oust #20.
const BRAINSTORM: usize = 7;

/// Projected corners of the board cards and piles outside the viewer's chooser.
fn background(page: &mut Page) -> Vec<(ElementId, [Vec2; 4])> {
    let mut elements: Vec<_> = page
        .frame()
        .resolved
        .iter()
        .filter(|element| {
            matches!(element.role, Role::Card | Role::Pile)
                && matches!(
                    Place::in_frame(element.place),
                    Some(
                        Place::Card {
                            zone: ZoneName::Battlefield
                                | ZoneName::Library
                                | ZoneName::Graveyard
                                | ZoneName::Exile,
                            ..
                        } | Place::Card {
                            zone: ZoneName::Hand,
                            side: Side::Opponent,
                            ..
                        }
                    )
                )
        })
        .map(|element| (element.id, element.corners))
        .collect();
    elements.sort_unstable_by_key(|(id, _)| id.0);
    elements
}

#[test]
fn a_put_back_drag_leaves_the_background_still() {
    let mut page = at_prompt("shape-split", BRAINSTORM);
    page.press("card #3");
    page.move_to(Vec2::new(SIZE.width / 2.0, 20.0));
    page.step(Duration::from_millis(600));

    let board = background(&mut page);
    let given = page.view_given_at();
    let projection = page.frame().view_projection;
    let shake = page.frame().shake;
    for place in ["battlefield.yours.lands", "hand.theirs", "library.theirs"] {
        assert!(
            page.frame()
                .resolved
                .iter()
                .any(|node| node.place.label() == place),
            "the recorded board has visible elements in {place}"
        );
    }

    let drain = page.find("card #10").rect();
    let grip = Vec2::new(drain.x + drain.width * 0.15, drain.y + drain.height / 2.0);
    let in_fan = grip + Vec2::new(30.0, 0.0);
    let top = middle(page.find("card #3").rect());
    page.hold_between(grip, in_fan);
    for point in [
        in_fan,
        in_fan.lerp(top, 0.5),
        top,
        top.lerp(in_fan, 0.5),
        in_fan,
    ] {
        page.move_to(point);
        page.step(Duration::from_millis(16));
        assert_eq!(page.view_given_at(), given);
        let frame = page.frame();
        assert_eq!(frame.view_projection, projection);
        assert_eq!(frame.shake, shake);
        assert!(
            frame.resolved.iter().any(|node| {
                node.role == Role::Card && node.id.0 == 10 && node.state.has(StateBits::HELD)
            }),
            "Mana Drain is still held at {point}"
        );
        assert_eq!(background(&mut page), board, "background at {point}");
    }
}

#[test]
fn a_card_put_back_from_the_hand_is_dropped_on_the_librarys_top_place() {
    let mut page = at_prompt("shape-split", BRAINSTORM);
    let library = page.find("region \"Top\"").rect();
    let hand = page.find("region \"Hand\"").rect();
    assert!(
        library.x + library.width <= hand.x + 1.0,
        "the library lies left of the hand: {library:?} and {hand:?}"
    );
    // As the player did: a click on the first Counterspell puts it on the library.
    page.press("card #3");
    page.step(Duration::from_millis(600));
    // The pointer, left where the Counterspell lay, would raise the Tundra now there over the
    // fan: move it off the cards first.
    page.move_to(Vec2::new(SIZE.width / 2.0, 20.0));
    page.still();
    let counterspell = page.find("card #3").rect();
    assert!(
        inside(counterspell, library),
        "the reply puts it on the library: {counterspell:?} in {library:?}"
    );
    // Then Mana Drain, held by its left edge, which the Island over it leaves uncovered, and
    // let go a name bar below the Counterspell, where the library's top card lies:
    // `Click::Drop` into pile 0 at 0, the drop the recording answers.
    let drain = page.find("card #10").rect();
    assert!(
        inside(drain, hand),
        "Mana Drain lies in the hand's fan: {drain:?}"
    );
    let grip = Vec2::new(drain.x + drain.width * 0.15, drain.y + drain.height / 2.0);
    let top_place = middle(counterspell) + Vec2::new(0.0, 0.165 * counterspell.height);
    let board = background(&mut page);
    page.drag_between(grip, top_place);
    for _ in 0..40 {
        assert_eq!(background(&mut page), board);
        page.step(Duration::from_millis(16));
    }
    page.move_to(Vec2::new(SIZE.width / 2.0, 20.0));
    page.still();
    let drain = page.find("card #10").rect();
    let counterspell = page.find("card #3").rect();
    assert!(
        inside(drain, library),
        "the reply puts Mana Drain on the library: {drain:?} in {library:?}"
    );
    assert!(
        drain.y > counterspell.y,
        "Mana Drain is the library's top card, lowest in its column: {drain:?} and \
         {counterspell:?}"
    );

    page.press("button \"Done\"");
    page.step(Duration::from_millis(600));
    assert!(page.try_find("region \"Top\"").is_err());
    assert!(page.position().0 > BRAINSTORM);
}
