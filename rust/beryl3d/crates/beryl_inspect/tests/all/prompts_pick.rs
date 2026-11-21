//! The prompt shape `Pick` on the shape recordings (docs/beryl-3d.md, "The flat layer"): a
//! target clicked where it lies, the arrow that follows the pointer to it, and the cards a
//! search holds up in its tray. Each test plays the recording to a prompt, acts as the player
//! did, and lets the driver answer each click with the session's recorded reply.

use std::str::FromStr;

use app_client::driver::{Duration, Moment, Page};
use beryl_inspect::pages::page;
use beryl_protocol::protocol::{Side, ZoneName};
use game_play::light::arrows::AIM_ID;
use glam::Vec2;
use kit_frame::frame::{Role, StateBits};
use kit_probe::agent::Layout;
use kit_ui::input::Size;

const SIZE: Size = Size {
    width: 1600.0,
    height: 900.0,
};

/// `name` played from the viewer's seat to the moment `at`, when the table has come to rest
/// under that prompt, the `view`th, and on for as long as its own marks take to come up. The
/// recorded answer to that prompt is held back, so that the test answers it.
fn at_prompt(name: &str, at: &str, view: usize) -> Page {
    let mut page =
        page(name, Side::You, Layout::Desktop, SIZE).unwrap_or_else(|why| panic!("{why}"));
    page.set_reply_delay(view, Duration::from_millis(60_000));
    let moment = Moment::from_str(at).unwrap_or_else(|why| panic!("{at}: {why:?}"));
    page.seek(&moment).unwrap_or_else(|why| panic!("{why}"));
    page.step(Duration::from_millis(300));
    page
}

/// The end of the arrow to the pointer in the present frame, if one is drawn.
fn aim_end(page: &mut Page) -> Option<Vec2> {
    let frame = page.frame();
    (frame.resolved.iter())
        .find(|element| element.role == Role::Arrow && element.id.0 == AIM_ID)
        .map(|element| element.corners[3])
}

/// `shape-pick`'s sixth prompt, at its tenth view, asks for the target of a spell: one of the viewer's own
/// creatures, #12, which the prompt asks about before it takes it, or the opponent's, #260.
const TARGET: &str = "prompt:6";

#[test]
fn a_target_clicked_where_it_lies_wears_the_mark_the_reply_sends() {
    let mut page = at_prompt("shape-pick", TARGET, 10);
    let mine = page.find("card #12");
    assert!(
        mine.states().has(StateBits::ELIGIBLE),
        "the prompt's own mark is up: {:?}",
        mine.states()
    );
    // The driver answers the click with the reply recorded for `Click::On` the card's subject,
    // and fails the test on any other click.
    page.press("card #12");
    page.step(Duration::from_millis(300));
    let picked = page.find("card #12");
    assert!(
        picked.states().has(StateBits::TARGET),
        "the reply marks it as the target: {:?}",
        picked.states()
    );
}

#[test]
fn an_eligible_outline_follows_the_hand_card_when_it_rises() {
    let mut page = at_prompt("shape-pick", "prompt:1", 1);
    let card = "card #7";
    let mark = "card #7 > mark \"eligible\"";
    let resting = page.find(card).rect();
    page.hover(card);
    page.step(Duration::from_millis(300));
    let raised = page.find(card).rect();
    assert!(raised.y < resting.y - 10.0, "the card rises");
    let outline = page.find(mark).rect();
    assert!(
        (outline.y - raised.y).abs() < 2.0,
        "the outline follows: {outline:?}"
    );
    assert!((outline.width - raised.width).abs() < 2.0);
    page.check();
}

#[test]
fn the_aim_arrow_follows_the_pointer_until_the_target_is_picked() {
    let mut page = at_prompt("shape-pick", TARGET, 10);
    let first = Vec2::new(400.0, 300.0);
    page.move_to(first);
    page.step(Duration::from_millis(16));
    let end = aim_end(&mut page).expect("an arrow follows the pointer");
    assert!(
        end.distance(first) < 1.0,
        "it ends at the pointer, not {end}"
    );
    let second = Vec2::new(1000.0, 250.0);
    page.move_to(second);
    page.step(Duration::from_millis(16));
    let end = aim_end(&mut page).expect("it still follows the pointer");
    assert!(
        end.distance(second) < 1.0,
        "it follows to {second}, not {end}"
    );
    page.press("card #12");
    page.step(Duration::from_millis(16));
    assert_eq!(
        aim_end(&mut page),
        None,
        "the one target the spell takes is picked"
    );
}

#[test]
fn a_search_holds_its_cards_up_and_lets_them_go_after_the_answer() {
    // `shape-zone`'s second prompt searches the library for an artifact: #17 or #19.
    let mut page = at_prompt("shape-zone", "prompt:2", 2);
    let library = page.find("pile \"Your library\"").rect();
    let held = [page.find("card #17").rect(), page.find("card #19").rect()];
    for card in held {
        assert!(
            card.width > 2.0 * library.width,
            "held up large in the tray: {card:?}"
        );
        assert!(
            card.y + card.height < SIZE.height * 0.7,
            "over the table: {card:?}"
        );
    }
    assert!(
        (held[0].x - held[1].x).abs() > 1.0,
        "side by side: {held:?}"
    );
    // As the player did: #19, then #17 instead, then Submit.
    page.press("card #19");
    page.press("card #17");
    page.step(Duration::from_millis(300));
    assert!(
        page.find("card #17").states().has(StateBits::SELECTED),
        "the reply marks the card picked"
    );
    // The button the reply brings fades in before it can be pressed.
    page.press("button \"Submit 1\"");
    page.step(Duration::from_millis(2000));
    // The card found goes to the hand, and the one left goes back into the library.
    assert_eq!(page.find("card #17").zone(), Some(ZoneName::Hand));
    if let Ok(left) = page.try_find("card #19") {
        assert_eq!(left.zone(), Some(ZoneName::Library));
        assert!(
            left.rect().width < held[1].width * 0.8,
            "it has left the tray: {:?}",
            left.rect()
        );
    }
}

#[test]
fn a_player_is_picked_by_a_press_on_their_life_total() {
    // `shape-pick`'s third prompt asks which player Jace's +1 targets.
    let mut page = at_prompt("shape-pick", "prompt:3", 3);
    let opponent = page.find("player \"Opponent 20\"");
    assert!(opponent.interactive, "the prompt marks the opponent");
    // The driver answers only the click the player made: on the opponent.
    page.press("player \"Opponent 20\"");
    page.step(Duration::from_millis(16));
    assert!(
        !page.find("player \"You 20\"").interactive,
        "the answer closed the prompt"
    );
}
