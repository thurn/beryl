//! The pointer on the table (docs/beryl-3d.md, "Motion the pointer drives"): the card under the
//! mouse is hovered, and a hand card rises out of the fan under it and falls back when it leaves.

use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{CardType, Side, Why, ZoneName};
use beryl_protocol::script::Act;
use beryl_scene::agent::Layout;
use beryl_scene::agent::checks::reachable;
use beryl_scene::agent::fault::Context;
use beryl_scene::driver::{Duration, Page, Recording};
use beryl_scene::frame::{Frame, Role, StateBits};
use beryl_scene::input::{Button, Input, Insets, Modifiers, Pointer, PointerKind, PointerPhase};
use beryl_scene::input::{Rect, Size};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder, timing};
use beryl_scene::{Client, Config};
use glam::Vec2;

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
        Recording(messages).into(),
        Side::You,
        Layout::Desktop,
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
fn a_rise_is_where_its_time_says_however_many_frames_were_drawn() {
    let card = "card \"Memory Lapse\"";
    let mut every = page(&[sample()]);
    every.hover(card);
    every.step(ms(50));
    let mut none = page(&[sample()]);
    none.set_tick(ms(50));
    none.hover(card);
    none.step(ms(50));
    let (a, b) = (every.find(card).rect(), none.find(card).rect());
    assert!(same(a, b), "{a:?} drawn every tick, {b:?} drawn once");
    assert!(
        a.y < every.find("card \"Counterspell\"").rect().y,
        "it is part way up"
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
    let mut client = Client::new(Config {
        elements: 2048,
        draws: 2048,
        name_bytes: 65_536,
    });
    client.handle(Input::Surface {
        size: WINDOW,
        scale: 2.0,
        safe_area: Insets::default(),
    });
    client.handle(Input::Update(Box::new(sample().update())));
    let card = client
        .frame(0)
        .resolved
        .iter()
        .find(|element| element.role == Role::Card && element.interactive)
        .map(|element| (element.corners.iter().copied().sum::<Vec2>()) / 4.0)
        .expect("an interactive card");
    for phase in [PointerPhase::Down, PointerPhase::Move] {
        client.handle(Input::Pointer(Pointer {
            id: 1,
            kind: PointerKind::Touch,
            phase,
            position: card,
            button: Button::Primary,
            wheel: Vec2::ZERO,
            modifiers: Modifiers::default(),
        }));
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
        recording: "sample".to_owned(),
        seat: Side::You,
        layout: Layout::Desktop,
        size: WINDOW,
        safe_area: Insets::default(),
        at: 0,
    };
    let faults = reachable(&frame, &context);
    assert!(faults.len() < cards.len(), "{faults:?}");
}
