//! The opening screen (`screens::opening`) on the opening recordings: the hand stands in a row
//! of large cards over the dimmed table, Keep answers the mulligan with its button, and a card
//! clicked in the row while cards go on the bottom clicks the prompt on it.

use kit_ui::input::Input as DeviceInput;
use std::str::FromStr;

use app_client::Input;
use app_client::command::SessionCommand;
use app_client::driver::{Mode, Moment, Page};
use app_client::{Client, Config};
use beryl_inspect::pages::{page, recording};
use beryl_protocol::prompt::{Click, ClickRequest};
use beryl_protocol::protocol::{ObjectId, Side, Status};
use beryl_protocol::script::Subject;
use kit_frame::frame::{Role, Wake};
use kit_probe::agent::Layout;
use kit_probe::agent::fault::Rule;
use kit_ui::input::{Button, Insets, Modifiers, Pointer, PointerKind, PointerPhase, Size};

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
    assert!(tree.contains("text \"Opening hand\""), "{tree}");
    assert!(tree.contains("text \"On the draw\""), "{tree}");
    // The life orbs stand under the opening's backdrop, as the web's overlay is drawn over
    // its orbs, with the viewer's total. The opponent's is hidden under "On the draw", as the
    // web's coin covers it.
    assert!(tree.contains("region \"opponent's orb\""), "{tree}");
    assert!(!tree.contains("player \"Opponent 20\""), "{tree}");
    assert!(tree.contains("player \"You 20\""), "{tree}");
    // The stops under the backdrop take no press, as the web's overlay takes it.
    let draw = (tree.lines())
        .find(|line| line.contains("toggle \"Your draw\""))
        .expect("the viewer's draw stop");
    assert!(!draw.contains("interactive"), "{draw}");
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
    let mut client = Client::new(crate::test_data::data(), Config::LARGEST);
    client.handle(Input::Device(DeviceInput::Surface {
        size: SIZE,
        scale: 1.0,
        safe_area: Insets::default(),
    }));
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
        client.handle(Input::Device(DeviceInput::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase,
            position: centre,
            button: Button::Primary,
            wheel: glam::Vec2::ZERO,
            modifiers: Modifiers::default(),
        })));
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

#[test]
fn the_opening_shows_the_flip_while_the_coin_spins_and_then_the_hand() {
    // `opening-bottom`'s opponent won the flip. Its views reach the client at once, and the
    // coin spins for a second and a half from then, while the hand is dealt.
    let mut page = page("opening-bottom", Side::You, Layout::Desktop, SIZE)
        .unwrap_or_else(|why| panic!("{why}"));
    let moment = Moment::from_str("899ms").unwrap_or_else(|why| panic!("{why:?}"));
    page.seek(&moment).unwrap_or_else(|why| panic!("{why}"));
    let tree = page.tree();
    // The opening is drawn while the deal plays, with the flip's title and the dots, no word
    // of who starts and no button yet. The hand is dealt below the window, as the web's
    // desktop board holds it away under the opening, and the opening's row shows none of it.
    assert!(tree.contains("text \"Opponent won the flip\""), "{tree}");
    assert!(tree.contains("region \"waiting\""), "{tree}");
    assert!(!tree.contains("On the draw"), "{tree}");
    assert!(!tree.contains("button \"Keep 7\""), "{tree}");
    let plains = (tree.lines())
        .find(|line| line.contains("card \"Plains\" #33"))
        .unwrap_or_else(|| panic!("{tree}"));
    let top: f32 = (plains.split_whitespace())
        .find_map(|word| word.split_once(',').and_then(|(_, y)| y.parse().ok()))
        .unwrap_or_else(|| panic!("{plains}"));
    assert!(top >= SIZE.height, "{plains}");
    assert!(tree.contains("player \"You 20\""), "{tree}");
    // Once it has stopped, the hand stands in the row and the decision is offered.
    let mut page = at("opening-bottom", "prompt:1");
    let tree = page.tree();
    assert!(tree.contains("text \"Opening hand\""), "{tree}");
    assert!(tree.contains("button \"Keep 7\""), "{tree}");
    assert!(tree.contains("card \"Plains\" #33"), "{tree}");
    assert!(!tree.contains("region \"waiting\""), "{tree}");
}
