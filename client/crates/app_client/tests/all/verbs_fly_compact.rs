//! Compact-window flight visibility: staggered flights into a narrow or piled row separate in
//! mid-air so each flying card remains visible for `step-shown`.

use app_client::driver::{Mode, Page, Recording};
use beryl_card_records::{TokenMakers, fixture_message};
use beryl_protocol::GameUpdate;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{CardType, ObjectId, Side, Why, ZoneName};
use beryl_protocol::script::{Act, Table, time_script};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use game_table::table::Window;
use kit_probe::agent::Rule;
use kit_ui::input::Size;

fn compact_window(width: f32, height: f32) -> Window {
    let size = Size { width, height };
    Window {
        size,
        scale: 3.0,
        safe_area: crate::test_data::data()
            .profiles
            .for_size(size)
            .test_safe_area(),
        ..Window::default()
    }
}

fn unseen_steps(
    before: &GameUpdate,
    after: &GameUpdate,
    seat: Side,
    window: Window,
) -> Vec<String> {
    let mut messages = vec![
        FixtureMessage::game_update(before),
        FixtureMessage::game_update(after),
    ];
    messages.insert(0, fixture_message(&messages, TokenMakers::NONE));
    let mut page = Page::in_window(
        crate::test_data::data(),
        Recording(messages).into(),
        seat,
        window,
    )
    .expect("the views can be read");
    page.check_all(Mode::EveryTick)
        .faults
        .into_iter()
        .filter(|fault| fault.rule == Rule::StepShown)
        .map(|fault| fault.message)
        .collect()
}

/// Six identical lands returning from opponent exile into a piled battlefield lands row in one
/// staggered beat on a 393x759 compact window are each seen as they fly rather than covering each
/// other along the short arc.
#[test]
fn staggered_exile_returns_into_piled_compact_row_are_seen_in_flight() {
    let mut builder = ViewBuilder::new();
    for side in [Side::You, Side::Opponent] {
        for _ in 0..5 {
            builder.card(ZoneName::Library, side, "Plains");
        }
    }
    let mut exiled = Vec::new();
    for _ in 0..6 {
        let card = builder.card(ZoneName::Exile, Side::Opponent, "Mountain");
        card.types = vec![CardType::Land];
        exiled.push(card.slot);
    }
    builder.script(ScriptBuilder::reset().build());
    let before = builder.update();

    let mut view = before.view.clone();
    let mut flights = Vec::new();
    for slot in exiled {
        let object = view
            .objects
            .iter_mut()
            .find(|o| o.slot == slot)
            .expect("exiled mountain exists");
        object.zone = ZoneName::Battlefield;
        let object = object.clone();
        view.battlefield.push(slot);
        flights.push(Act::Fly {
            from: ZoneName::Exile,
            why: Why::Effect,
            object,
            at: None,
            item: None,
            origin: None,
        });
    }
    let mut script = ScriptBuilder::default();
    script.beat(flights);
    let mut script = script.build();
    script.timing = time_script(&script, &Table::of(&before.view));
    view.script = Some(script);
    let after = GameUpdate {
        session: before.session,
        view,
    };

    let unseen = unseen_steps(&before, &after, Side::You, compact_window(393.0, 759.0));
    assert!(unseen.is_empty(), "{unseen:?}");
}

/// Several lands flown in quick succession to the end of a narrow compact lands row stay visible
/// in mid-air rather than stacking behind the earlier flights from the same beat.
#[test]
fn staggered_flights_into_narrow_compact_lands_row_are_seen_in_flight() {
    let mut builder = ViewBuilder::new();
    let mut lands: Vec<ObjectId> = Vec::new();
    for index in 0..12 {
        let name = format!("Land {index:02}");
        let card = builder.card(ZoneName::Battlefield, Side::You, &name);
        card.types = vec![CardType::Land];
        lands.push(card.slot);
    }
    builder.script(ScriptBuilder::reset().build());
    let before = builder.update();

    let mut view = before.view.clone();
    let mut flights = Vec::new();
    let end = u16::try_from(view.battlefield.len().saturating_sub(1)).expect("fits u16");
    for &slot in &lands[..5] {
        view.battlefield.retain(|&s| s != slot);
        view.battlefield.push(slot);
        let object = view
            .objects
            .iter_mut()
            .find(|o| o.slot == slot)
            .expect("land exists")
            .clone();
        flights.push(Act::Fly {
            from: ZoneName::Battlefield,
            why: Why::Effect,
            object,
            at: Some(end),
            item: None,
            origin: None,
        });
    }
    let mut script = ScriptBuilder::default();
    script.beat(flights);
    let mut script = script.build();
    script.timing = time_script(&script, &Table::of(&before.view));
    view.script = Some(script);
    let after = GameUpdate {
        session: before.session,
        view,
    };

    let unseen = unseen_steps(&before, &after, Side::You, compact_window(375.0, 647.0));
    assert!(unseen.is_empty(), "{unseen:?}");
}
