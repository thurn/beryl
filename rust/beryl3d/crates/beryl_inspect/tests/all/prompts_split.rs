//! The prompt shapes `Split` and the pile of identical tokens on their recordings
//! (docs/beryl-3d.md, "The flat layer"): a split's two piles held up in boxes under the session's
//! names for them, a card clicked to the other pile or carried into one, and a click on a pile's
//! top token. Each test plays the recording to a prompt, acts as the player did, and lets the
//! driver answer each click with the session's recorded reply, which fails the test for any
//! click the player did not make: a drop into another pile, or before another card, among them.

use std::str::FromStr;

use beryl_inspect::pages::page;
use beryl_protocol::protocol::Side;
use beryl_scene::agent::Layout;
use beryl_scene::driver::{Duration, Moment, Page};
use beryl_scene::input::{Rect, Size};
use glam::Vec2;

const SIZE: Size = Size {
    width: 1600.0,
    height: 900.0,
};

/// `name` played from the viewer's seat to `prompt:<view>`, when the table has come to rest
/// under it, and on for as long as its own marks and tray take to come up. The recorded answer
/// to that prompt is held back, so that the test answers it.
fn at_prompt(name: &str, view: usize) -> Page {
    let mut page =
        page(name, Side::You, Layout::Desktop, SIZE).unwrap_or_else(|why| panic!("{why}"));
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
