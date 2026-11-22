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
use beryl_protocol::prompt::{Click, ClickRequest, Marked};
use beryl_protocol::protocol::{ObjectId, Side, Status, ZoneName};
use beryl_protocol::script::{Mark, Script, Subject};
use glam::Vec2;
use kit_base::data::profiles::SizeClass;
use kit_frame::frame::{ElementId, Frame, Role, Wake};
use kit_probe::agent::fault::Rule;
use kit_ui::input::{Button, Insets, Modifiers, Pointer, PointerKind, PointerPhase, Size};

const SIZE: Size = Size {
    width: 1600.0,
    height: 900.0,
};
const REPORTED_SIZE: Size = Size {
    width: 1728.0,
    height: 1117.0,
};
const MILLISECOND: u64 = 1_000_000;

/// `name` played to the moment `at`, settled.
fn at(name: &str, at: &str) -> Page {
    at_size(name, at, SIZE)
}

fn at_size(name: &str, at: &str, size: Size) -> Page {
    let mut page =
        page(name, Side::You, SizeClass::Regular, size).unwrap_or_else(|why| panic!("{why}"));
    let moment = Moment::from_str(at).unwrap_or_else(|why| panic!("{at}: {why:?}"));
    page.seek(&moment).unwrap_or_else(|why| panic!("{why}"));
    page.still();
    page
}

/// The faults of "rest equals placement" over the whole of `name`.
fn rest_faults(name: &str) -> Vec<String> {
    let mut page =
        page(name, Side::You, SizeClass::Regular, SIZE).unwrap_or_else(|why| panic!("{why}"));
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
fn the_opening_hand_fans_out_and_keep_answers_the_mulligan() {
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
    assert_fan(&mut page, SIZE, 1.0, 0.0);
    let mut reported = at_size("opening-keep", "prompt:1", REPORTED_SIZE);
    assert_fan(&mut reported, REPORTED_SIZE, 1.2, 18.5);
    reported.check();
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

/// Checks the visible cards against the web at the same scale and stage offset.
fn assert_fan(page: &mut Page, size: Size, scale: f32, stage_top: f32) {
    let frame = page.frame();
    let corners = |name: &str| {
        let (_, slot) = name.rsplit_once(" #").expect("a slot");
        frame
            .corners(ElementId(slot.parse().expect("a card id")))
            .expect("the card's corners")
    };
    let middle = corners(HAND[3]);
    let bottom = middle[0].midpoint(middle[1]);
    let web_bottom = stage_top + (246.8 + 176.0 * 88.0 / 63.0) * scale;
    assert!(
        bottom.distance(Vec2::new(size.width / 2.0, web_bottom)) < 1.0,
        "{bottom:?}"
    );
    // These are the visible web cards' rotations and drops around their bottom centers.
    for (name, (across, drop, turn)) in HAND.into_iter().zip([
        (-552.0, 45.0, -6.0_f32),
        (-368.0, 20.0, -4.0),
        (-184.0, 5.0, -2.0),
        (0.0, 0.0, 0.0),
        (184.0, 5.0, 2.0),
        (368.0, 20.0, 4.0),
        (552.0, 45.0, 6.0),
    ]) {
        let card = corners(name);
        let pivot = card[0].midpoint(card[1]);
        let edge = card[1] - card[0];
        assert!(
            pivot.distance(bottom + Vec2::new(across, drop) * scale) < 1.0,
            "{name} at {pivot:?}, middle at {bottom:?}"
        );
        assert!(
            (edge.y.atan2(edge.x).to_degrees() - turn).abs() < 0.1,
            "{name}: {edge:?}"
        );
        assert!(
            (edge.length() - 176.0 * scale).abs() < 1.0,
            "{name}: {edge:?}"
        );
    }
}

/// Advances the client until the opening and its card moves have settled.
fn settled<'a>(client: &'a mut Client, now: &mut u64) -> &'a Frame {
    while {
        client.frame(*now);
        client.wake() != Wake::Never
    } {
        *now += 16 * MILLISECOND;
    }
    client.frame(*now)
}

#[test]
fn a_card_clicked_in_the_row_while_putting_on_the_bottom_clicks_the_prompt_on_it() {
    // The recording ends at the put-on-bottom prompt with no click recorded, so the client is
    // driven by hand and the command it sends is read rather than answered.
    let recording = recording("opening-bottom", Side::You).unwrap_or_else(|why| panic!("{why}"));
    let views = recording.views().unwrap_or_else(|why| panic!("{why}"));
    let mut last = views.last().expect("a view").clone();
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
    let frame = settled(&mut client, &mut now);
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
    // The host and the visible card agree on where the click lands.
    for (face, host) in card.corners.into_iter().zip(boxed.corners) {
        assert!(face.distance(host) < 1.0, "{card:?} {boxed:?}");
    }
    let centre = card.corners[0].midpoint(card.corners[2]);
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
    // The recorded prompt has no reply. Supply the mark the session sends for the pick.
    let Status::Deciding { decision } = &mut last.view.status else {
        unreachable!("the last view asks the viewer");
    };
    decision.prompt.marks.push(Marked {
        on,
        mark: Mark::Selected,
    });
    decision.prompt.revision += 1;
    last.view.script = Some(Script::empty());
    let hand: Vec<_> = last
        .view
        .objects
        .iter()
        .filter(|object| object.zone == ZoneName::Hand && object.owner == Side::You)
        .map(|object| object.slot)
        .filter(|slot| *slot != ObjectId(8))
        .collect();
    client.handle(Input::Update(Box::new(last)));
    let frame = settled(&mut client, &mut now);
    let edge = |slot: ObjectId| {
        let [left, right, ..] = frame
            .corners(ElementId(u32::from(slot.0)))
            .expect("a hand card");
        (
            left.midpoint(right),
            (right.y - left.y).atan2(right.x - left.x).to_degrees(),
        )
    };
    let (left, left_turn) = edge(hand[0]);
    let (right, right_turn) = edge(*hand.last().expect("cards still in the fan"));
    assert_eq!(hand.len(), 6);
    assert!(
        left.midpoint(right)
            .distance(Vec2::new(SIZE.width / 2.0, left.y))
            < 1.0
    );
    assert!((left_turn + 5.0).abs() < 0.1 && (right_turn - 5.0).abs() < 0.1);
    let (picked, picked_turn) = edge(ObjectId(8));
    assert!(
        picked.y > left.y && picked_turn.abs() < 0.1,
        "{picked:?}, {picked_turn}"
    );
    assert!(rest_faults("opening-bottom").is_empty());
}

#[test]
fn the_opening_shows_the_flip_while_the_coin_spins_and_then_the_hand() {
    // `opening-bottom`'s opponent won the flip. Its views reach the client at once, and the
    // coin spins for a second and a half from then, while the hand is dealt.
    let mut page = page("opening-bottom", Side::You, SizeClass::Regular, SIZE)
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
