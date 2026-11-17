//! The opening screen (`screens::opening`) on the opening recordings: the hand stands in a row
//! of large cards over the dimmed table, Keep answers the mulligan with its button, and a card
//! clicked in the row while cards go on the bottom clicks the prompt on it.

use std::str::FromStr;

use beryl_inspect::pages::{page, recording};
use beryl_protocol::prompt::{Click, ClickRequest};
use beryl_protocol::protocol::{ObjectId, Side, Status};
use beryl_protocol::script::Subject;
use beryl_scene::agent::Layout;
use beryl_scene::agent::fault::Rule;
use beryl_scene::command::SessionCommand;
use beryl_scene::driver::{Mode, Moment, Page};
use beryl_scene::frame::{Role, Wake};
use beryl_scene::input::{
    Button, Input, Insets, Modifiers, Pointer, PointerKind, PointerPhase, Size,
};
use beryl_scene::{Client, Config};

const SIZE: Size = Size {
    width: 1600.0,
    height: 900.0,
};
const MILLISECOND: u64 = 1_000_000;

/// `name` played to the moment `at`, settled.
fn at(name: &str, at: &str) -> Page {
    let mut page =
        page(name, Side::You, Layout::Desktop, SIZE).unwrap_or_else(|why| panic!("{why}"));
    let moment = Moment::from_str(at).unwrap_or_else(|why| panic!("{at}: {why:?}"));
    page.seek(&moment).unwrap_or_else(|why| panic!("{why}"));
    page.still();
    page
}

/// The faults of "rest equals placement" over the whole of `name`.
fn rest_faults(name: &str) -> Vec<String> {
    let mut page =
        page(name, Side::You, Layout::Desktop, SIZE).unwrap_or_else(|why| panic!("{why}"));
    (page.check_all(Mode::AtCheckTimes).faults.into_iter())
        .filter(|fault| fault.rule == Rule::RestPlacement)
        .map(|fault| fault.message)
        .collect()
}

/// `opening-keep`'s hand: Plains #33 to Damn #39.
const HAND: [&str; 7] = [
    "Plains #33",
    "Jace, the Mind Sculptor #34",
    "Thirst for Discovery #35",
    "Counterspell #36",
    "Island #37",
    "Beza, the Bounding Spring #38",
    "Damn #39",
];

#[test]
fn the_opening_hand_stands_in_the_row_and_keep_answers_the_mulligan() {
    let mut page = at("opening-keep", "prompt:1");
    let tree = page.tree();
    // The opening's backdrop covers the step stops, as the web's overlay does: none takes a
    // press beside the hand.
    assert!(!tree.contains("toggle \"Your draw\""), "{tree}");
    assert!(tree.contains("text \"Opening hand\""), "{tree}");
    assert!(tree.contains("text \"On the draw\""), "{tree}");
    // The life totals are hidden, as the web's overlay covers them: the opponent's stood over
    // "On the draw".
    assert!(!tree.contains("player \"Opponent 20\""), "{tree}");
    assert!(!tree.contains("player \"You 20\""), "{tree}");
    for name in HAND {
        let (card_name, slot) = name.rsplit_once(" #").expect("a slot");
        let boxed = page.find(&format!("region \"{name}\"")).rect();
        let card = page.find(&format!("card \"{card_name}\" #{slot}")).rect();
        // The card is drawn in its box, not at the hand along the window's bottom.
        assert!(
            (card.x - boxed.x).abs() < 1.0 && (card.y - boxed.y).abs() < 1.0,
            "{name} at {card:?}, its box at {boxed:?}"
        );
        assert!(
            card.y + card.height < SIZE.height / 2.0 + 100.0,
            "{name} at {card:?}"
        );
    }
    page.check();
    let (view, _, _) = page.position();
    // The driver answers the press with the reply recorded for the mode prompt's Keep button,
    // and fails the test on any other command.
    page.press("button \"Keep 7\"");
    page.still();
    let (after, _, _) = page.position();
    assert!(
        after > view,
        "keeping plays the game on: view {view} then {after}"
    );
    assert!(rest_faults("opening-keep").is_empty());
}

#[test]
fn a_card_clicked_in_the_row_while_putting_on_the_bottom_clicks_the_prompt_on_it() {
    // The recording ends at the put-on-bottom prompt with no click recorded, so the client is
    // driven by hand and the command it sends is read rather than answered.
    let recording = recording("opening-bottom", Side::You).unwrap_or_else(|why| panic!("{why}"));
    let views = recording.views().unwrap_or_else(|why| panic!("{why}"));
    let last = views.last().expect("a view").clone();
    let Status::Deciding { decision } = &last.view.status else {
        panic!("the last view asks the viewer");
    };
    let (decision, revision) = (decision.id, decision.prompt.revision);
    let mut client = Client::new(Config::LARGEST);
    client.handle(Input::Surface {
        size: SIZE,
        scale: 1.0,
        safe_area: Insets::default(),
    });
    for view in views {
        client.handle(Input::Update(Box::new(view)));
    }
    let mut now = 0;
    while {
        client.frame(now);
        client.wake() != Wake::Never
    } {
        now += 16 * MILLISECOND;
    }
    let frame = client.frame(now);
    let named = |name: &str| {
        (frame.resolved.iter())
            .find(|element| frame.name(element.name) == name)
            .unwrap_or_else(|| panic!("no element {name}"))
    };
    let boxed = named("Stock Up #8");
    assert_eq!(boxed.role, Role::Region);
    let card = (frame.resolved.iter())
        .find(|element| element.role == Role::Card && frame.name(element.name) == "Stock Up")
        .expect("the card");
    // The card stands in its box in the row, where the click lands.
    assert!(
        card.corners[0].distance(boxed.corners[0]) < 1.0,
        "{card:?} {boxed:?}"
    );
    let centre = (boxed.corners[0] + boxed.corners[2]) / 2.0;
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        client.handle(Input::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase,
            position: centre,
            button: Button::Primary,
            wheel: glam::Vec2::ZERO,
            modifiers: Modifiers::default(),
        }));
        now += 16 * MILLISECOND;
        client.frame(now);
    }
    let commands: Vec<SessionCommand> = std::iter::from_fn(|| client.next_command()).collect();
    let on = Subject::Object { slot: ObjectId(8) };
    assert_eq!(
        commands,
        [SessionCommand::Click(ClickRequest {
            decision,
            revision,
            click: Click::On { on },
        })]
    );
    assert!(rest_faults("opening-bottom").is_empty());
}
