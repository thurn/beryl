//! The pointer on the table (docs/beryl-3d.md, "Motion the pointer drives"): the card under the
//! mouse is hovered, and a hand card rises out of the fan under it and falls back when it leaves.

use app_client::Input;
use app_client::driver::{Duration, Page, Recording};
use app_client::{Client, Config};
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{CardType, Side, Why, ZoneName};
use beryl_protocol::script::Act;
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, timing};
use glam::Vec2;
use kit_frame::frame::{Frame, Material, Role, StateBits};
use kit_probe::agent::checks::reachable;
use kit_probe::agent::fault::Context;
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Button, Insets, Modifiers, Pointer, PointerKind, PointerPhase};
use kit_ui::input::{Rect, Size};

const WINDOW: Size = Size {
    width: 1600.0,
    height: 900.0,
};

fn ms(milliseconds: u64) -> Duration {
    Duration::from_millis(milliseconds)
}

/// A page that plays `views`, the first starting the table over, from the viewer's seat.
fn page(views: &[ViewBuilder]) -> Page {
    let messages = views
        .iter()
        .map(|view| FixtureMessage::game_update(&view.update()))
        .collect();
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        WINDOW,
    )
    .unwrap();
    page.still();
    page
}

/// The sample table, starting the table over.
fn sample() -> ViewBuilder {
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    view
}

/// The names of the cards drawn `hovered` in `frame`.
fn hovered(frame: &Frame) -> Vec<String> {
    frame
        .resolved
        .iter()
        .filter(|element| element.role == Role::Card && element.state.has(StateBits::HOVERED))
        .map(|element| frame.name(element.name).to_owned())
        .collect()
}

fn centre(rect: Rect) -> Vec2 {
    Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

fn same(a: Rect, b: Rect) -> bool {
    [
        (a.x, b.x),
        (a.y, b.y),
        (a.width, b.width),
        (a.height, b.height),
    ]
    .iter()
    .all(|(a, b)| (a - b).abs() < 0.01)
}

#[test]
fn a_hovered_hand_card_rises_and_falls_back() {
    let card = "card \"Counterspell\"";
    let mut page = page(&[sample()]);
    let rest = page.find(card).rect();
    page.hover(card);
    assert!(page.find(card).states().has(StateBits::HOVERED));
    page.step(ms(100));
    let up = page.find(card).rect();
    assert!(
        up.y + up.height < rest.y + rest.height - 10.0 && up.height > rest.height * 1.2,
        "raised to {up:?} from {rest:?}"
    );
    // Off the table's cards, above the battlefield.
    page.move_to(Vec2::new(WINDOW.width / 2.0, 4.0));
    assert!(!page.find(card).states().has(StateBits::HOVERED));
    page.step(ms(160));
    assert!(same(page.find(card).rect(), rest), "back in the fan");
}

#[test]
fn a_hovered_hand_card_paints_over_the_life_hud_until_it_lands() {
    let mut page = page(&[sample()]).with_scale(2.0);
    page.set_looking(true);
    let locator = "card \"Plains\" #6";
    let check = |frame: &Frame, lifted: bool| {
        use super::flat_layer::find;

        let card = frame
            .resolved
            .iter()
            .position(|node| node.role == Role::Card && node.id.0 == 6)
            .expect("Plains in hand");
        let orb = find(frame, Role::Region, "your orb");
        let total = find(frame, Role::Player, "You 20");
        let step = find(frame, Role::Icon, "Your draw");
        let card_draw = frame
            .owners
            .iter()
            .zip(&frame.draws)
            .position(|(&owner, draw)| {
                owner as usize == card && draw.material == Material::Card as u8
            })
            .expect("the hand card draws its face");

        for hud in [orb, total, step] {
            let node = &frame.resolved[hud];
            let name = frame.name(node.name);
            if lifted {
                let bounds =
                    kit_frame::frame::picking::Area::Quad(frame.resolved[card].corners, None);
                assert!(
                    bounds.contains(node.corners.iter().copied().sum::<Vec2>() * 0.25),
                    "the card overlaps {name}"
                );
            }
            let mut draws = frame
                .owners
                .iter()
                .enumerate()
                .filter(|&(_, &owner)| owner as usize == hud)
                .map(|(index, _)| index)
                .peekable();
            assert!(draws.peek().is_some(), "{name} draws records");
            assert!(
                draws.all(|draw| (card_draw > draw) == lifted),
                "wrong paint order for {name} and the hand card"
            );
        }
    };

    check(page.frame(), false);
    page.hover(locator);
    page.step(ms(60));
    check(page.frame(), true);
    page.step(ms(100));
    check(page.frame(), true);
    page.move_to(Vec2::new(WINDOW.width / 2.0, 4.0));
    page.step(ms(15));
    check(page.frame(), true);
    page.step(ms(160));
    check(page.frame(), false);
}

#[test]
fn a_rise_is_where_its_time_says_however_many_frames_were_drawn() {
    let card = "card \"Memory Lapse\"";
    let mut every = page(&[sample()]);
    let rest = every.find(card).rect();
    every.hover(card);
    every.step(ms(33));
    let mut none = page(&[sample()]);
    none.set_tick(ms(33));
    none.hover(card);
    none.step(ms(33));
    let (a, b) = (every.find(card).rect(), none.find(card).rect());
    assert!(same(a, b), "{a:?} drawn every tick, {b:?} drawn once");

    every.step(ms(67));
    let raised = every.find(card).rect();
    let share = (rest.y - a.y) / (rest.y - raised.y);
    // The web's cubic ease-out has covered 70% at 33 ms. The card's changing tilt
    // also moves its upper edge, so allow for the projected box's shape.
    assert!(
        (0.60..0.80).contains(&share),
        "lifted {share:.2} of the way"
    );
}

#[test]
fn sweeping_a_hand_hovers_each_card_once_in_order() {
    let names = [
        "Opt",
        "Ponder",
        "Preordain",
        "Brainstorm",
        "Counterspell",
        "Memory Lapse",
        "Mana Leak",
    ];
    let mut view = ViewBuilder::new();
    for name in names {
        view.card(ZoneName::Hand, Side::You, name);
    }
    view.script(ScriptBuilder::reset().build());
    let mut page = page(&[view]);
    let first = page.find("card \"Opt\"").rect();
    let last = page.find("card \"Mana Leak\"").rect();
    let middle = page.find("card \"Brainstorm\"").rect();
    let y = middle.y + middle.height / 2.0;
    let mut seen: Vec<String> = Vec::new();
    let steps = 120;
    for step in 0..=steps {
        let share = step as f32 / steps as f32;
        let x = (first.x - 10.0) + (last.x + last.width + 10.0 - first.x + 10.0) * share;
        page.move_to(Vec2::new(x, y));
        page.step(ms(16));
        for name in hovered(page.frame()) {
            if seen.last() != Some(&name) {
                seen.push(name);
            }
        }
    }
    assert_eq!(seen, names);
}

#[test]
fn a_card_that_flies_under_a_still_pointer_is_hovered_when_it_lands() {
    let mut before = ViewBuilder::new();
    for name in ["Counterspell", "Memory Lapse"] {
        before.card(ZoneName::Hand, Side::You, name);
    }
    let land = before.card(ZoneName::Hand, Side::You, "Island");
    land.types = vec![CardType::Land];
    let slot = land.slot;
    before.script(ScriptBuilder::reset().build());

    let mut view = before.build();
    let object = view.objects.iter_mut().find(|o| o.slot == slot).unwrap();
    object.zone = ZoneName::Battlefield;
    let object = object.clone();
    let mut after = ViewBuilder::new();
    for object in &view.objects {
        let added = after.card(object.zone, Side::You, object.name.as_deref().unwrap());
        added.types.clone_from(&object.types);
    }
    let mut landed = after.clone();
    landed.script(ScriptBuilder::reset().build());
    let mut flight = ScriptBuilder::default()
        .beat([Act::Fly {
            from: ZoneName::Hand,
            why: Why::Play,
            object,
            at: None,
            item: None,
            origin: None,
        }])
        .build();
    flight.timing = vec![timing(300.0)];
    after.script(flight);

    // Where the land rests once it has landed.
    let target = centre(page(&[landed]).find("card \"Island\"").rect());
    let mut page = page(&[before, after]);
    page.move_to(target);
    assert!(!hovered(page.frame()).contains(&"Island".to_owned()));
    page.step(ms(410));
    assert!(
        page.find("card \"Island\"").states().has(StateBits::MOVING),
        "the land is in flight"
    );
    assert!(!hovered(page.frame()).contains(&"Island".to_owned()));
    page.still();
    assert_eq!(hovered(page.frame()), ["Island"]);
}

#[test]
fn a_touch_hovers_nothing() {
    let mut client = Client::new(
        crate::test_data::data(),
        Config {
            elements: 2048,
            draws: 2048,
            name_bytes: 65_536,
        },
    );
    client.handle(Input::Device(DeviceInput::Surface {
        size: WINDOW,
        scale: 2.0,
        safe_area: Insets::default(),
    }));
    client.handle(Input::Update(Box::new(sample().update())));
    let card = client
        .frame(0)
        .resolved
        .iter()
        .find(|element| element.role == Role::Card && element.interactive)
        .map(|element| (element.corners.iter().copied().sum::<Vec2>()) / 4.0)
        .expect("an interactive card");
    for phase in [PointerPhase::Down, PointerPhase::Move] {
        client.handle(Input::Device(DeviceInput::Pointer(Pointer {
            id: 1,
            kind: PointerKind::Touch,
            phase,
            position: card,
            button: Button::Primary,
            wheel: Vec2::ZERO,
            modifiers: Modifiers::default(),
        })));
        let frame = client.frame(1_000_000);
        assert!(
            frame
                .resolved
                .iter()
                .all(|element| !element.state.has(StateBits::HOVERED)),
            "a touch {phase:?} hovers nothing"
        );
    }
}

#[test]
fn the_viewers_cards_at_rest_are_interactive_and_judged_reachable() {
    let mut page = page(&[sample()]);
    let frame = page.frame().clone();
    let cards: Vec<&str> = frame
        .resolved
        .iter()
        .filter(|element| element.role == Role::Card && element.interactive)
        .map(|element| frame.name(element.name))
        .collect();
    // Two hands of three, the viewer's face up; five permanents a side; a spell on the stack.
    assert_eq!(cards.len(), 3 + 5 * 2 + 1, "{cards:?}");
    assert!(!cards.contains(&"Brainstorm"), "a graveyard is a pile");
    let context = Context {
        size_class: crate::REGULAR,
        audit: crate::test_data::data().audit,
        checks: crate::test_data::data()
            .profiles
            .for_size_class(crate::REGULAR)
            .checks,
        size: WINDOW,
        safe_area: Insets::default(),
        at: 0,
    };
    let faults = reachable(&frame, &context);
    assert!(faults.len() < cards.len(), "{faults:?}");
}

#[test]
fn a_raised_hand_card_leaves_its_neighbours_reachable() {
    let card = "card \"Memory Lapse\"";
    let mut page = page(&[sample()]);
    let context = Context {
        size_class: crate::REGULAR,
        audit: crate::test_data::data().audit,
        checks: crate::test_data::data()
            .profiles
            .for_size_class(crate::REGULAR)
            .checks,
        size: WINDOW,
        safe_area: Insets::default(),
        at: 0,
    };
    let at_rest = reachable(page.frame(), &context);
    page.hover(card);
    page.step(ms(100));
    let raised = reachable(page.frame(), &context);
    assert_eq!(raised.len(), at_rest.len(), "{raised:#?}");
}
