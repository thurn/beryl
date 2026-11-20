//! The game over screen: the web interface's `GameOver` (`ui/src/screens/GameOver.tsx`) drawn
//! flat over a finished game.

use beryl_card_records::{TokenMakers, fixture_message};
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{Culprit, LossReason, OutcomeView, Side, Stall, Status, ZoneName};
use beryl_scene::agent::Layout;
use beryl_scene::driver::{Page, Recording};
use beryl_scene::input::Size;
use beryl_scene::testing::{ScriptBuilder, ViewBuilder};

const WINDOW: Size = Size {
    width: 1600.0,
    height: 900.0,
};

/// A page showing a game that ended with `outcome`, whose table holds one card, "Jace".
fn over(outcome: OutcomeView) -> Page {
    let mut builder = ViewBuilder::new();
    builder.card(ZoneName::Battlefield, Side::You, "Jace, the Mind Sculptor");
    builder.script(ScriptBuilder::reset().build());
    let mut update = builder.update();
    update.view.status = Status::Over { outcome };
    let mut messages = vec![FixtureMessage::game_update(&update)];
    messages.insert(0, fixture_message(&messages, TokenMakers::NONE));
    let mut page = Page::new(
        Recording(messages).into(),
        Side::You,
        Layout::Desktop,
        WINDOW,
    )
    .expect("the view can be read");
    page.still();
    page
}

fn win(winner: Side, reason: LossReason, by: Option<Culprit>) -> OutcomeView {
    OutcomeView::Win { winner, reason, by }
}

fn texts(page: &mut Page) -> String {
    page.tree()
}

#[test]
fn the_title_is_the_webs_for_each_outcome() {
    for (outcome, title, why) in [
        (win(Side::You, LossReason::Life, None), "Victory", None),
        (win(Side::Opponent, LossReason::Life, None), "Defeat", None),
        (
            win(Side::Opponent, LossReason::Library, None),
            "Defeat",
            Some("Library"),
        ),
        (
            win(Side::You, LossReason::Concede, None),
            "Victory",
            Some("Conceded"),
        ),
        (OutcomeView::Draw, "Draw", None),
        (
            OutcomeView::Stalled {
                reason: Stall::Turns,
            },
            "Stalled",
            Some("too many turns"),
        ),
    ] {
        let mut page = over(outcome);
        let tree = texts(&mut page);
        assert!(tree.contains(&format!("\"{title}\"")), "{title}:\n{tree}");
        if let Some(why) = why {
            assert!(tree.contains(&format!("\"{why}\"")), "{why}:\n{tree}");
        }
    }
}

#[test]
fn the_button_opens_the_start_screen() {
    let mut page = over(win(Side::You, LossReason::Life, None));
    assert!(page.try_find("region \"game over\"").is_ok());
    page.press("button \"New Game\"");
    assert!(page.try_find("region \"game over\"").is_err());
    assert!(page.try_find("region \"start\"").is_ok(), "{}", page.tree());
}

#[test]
fn the_card_that_ended_the_game_is_shown_as_a_face() {
    let culprit = Culprit {
        name: "Jace, the Mind Sculptor".to_owned(),
        ability: None,
        text: None,
    };
    let mut page = over(win(Side::You, LossReason::Effect, Some(culprit)));
    let tree = page.tree();
    assert!(
        tree.contains("region \"Jace, the Mind Sculptor\""),
        "{tree}"
    );
    // Damage ends a game with no reason line, so a card there is not shown.
    let mut page = over(win(Side::You, LossReason::Life, None));
    assert!(!page.tree().contains("region \"Jace"));
}

#[test]
fn view_battlefield_puts_the_result_aside_and_back_brings_it_back() {
    let mut page = over(win(Side::Opponent, LossReason::Life, None));
    page.press("button \"View Battlefield\"");
    page.still();
    assert!(
        page.try_find("region \"game over\"").is_err(),
        "{}",
        page.tree()
    );
    assert!(
        page.frame().backdrop.is_none(),
        "the table is no longer blurred"
    );
    page.press("button \"Back to Defeat\"");
    page.still();
    assert!(
        page.try_find("region \"game over\"").is_ok(),
        "{}",
        page.tree()
    );
    assert!(page.frame().backdrop.is_some());
}

#[test]
fn the_result_blurs_and_darkens_what_is_drawn_under_it() {
    let mut page = over(win(Side::Opponent, LossReason::Life, None));
    let frame = page.frame();
    let backdrop = frame.backdrop.expect("the end screen filters the table");
    // The web's `.table-dimmed`: `blur(2.5px) brightness(0.68) saturate(0.85)`, at scale 1.
    assert!((backdrop.blur - 2.5).abs() < 1e-6, "{backdrop:?}");
    assert!((backdrop.color[0][0] - 0.68 * (0.213 + 0.787 * 0.85)).abs() < 1e-4);
    // The table and the flat layer's furniture lie under the filter, and the result over it.
    assert!(
        backdrop.at > frame.layer,
        "{backdrop:?} with the layer at {}",
        frame.layer
    );
    assert!((backdrop.at as usize) < frame.draws.len());
}
