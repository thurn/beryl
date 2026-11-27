//! Cards a script leaves outside a scrolled row or stack on the phone layout: from the step
//! that leaves a card outside its container's window until a step moves it into view, it is
//! not drawn and takes no press, whatever later steps do to the cards around it.

use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{CardType, ObjectId, PlayerView, Side, StackItem, Why, ZoneName};
use beryl_protocol::script::{Act, Subject, Table as Played, time_script};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, object};
use game_play::play::Show;
use game_table::table::{Place, Table};
use kit_frame::frame::{Frame, Resolved, Role, StateBits};
use kit_probe::agent::checks::reachable;

use crate::common::{MS, empty_frame, frame};
use crate::table_compact::{bounds, card_faults, compact_window};

/// Every frame, one every 10 ms, that a show in a `width` by `height` window draws while
/// `after`'s script plays over `before`, in which no element moves: the frames between steps.
fn still_frames(width: f32, height: f32, before: &GameUpdate, after: &GameUpdate) -> Vec<Frame> {
    let window = compact_window(width, height);
    let mut show = Show::new(Table::new(crate::test_data::data(), window));
    let mut scratch = empty_frame();
    show.push(Box::new(before.clone()), Vec::new());
    frame(&mut show, &mut scratch, 0);
    let script = after.view.script.as_ref().expect("a script");
    let timings = time_script(script, &Played::of(&before.view));
    show.push(Box::new(after.clone()), timings);
    let mut frames = Vec::new();
    let mut now = 10 * MS;
    while now < 10_000 * MS {
        let drawn = frame(&mut show, &mut scratch, now);
        let still = !(drawn.resolved.iter()).any(|element| element.state.has(StateBits::MOVING));
        if still && !show.settled() {
            frames.push(drawn);
        }
        now += 10 * MS;
    }
    assert!(!frames.is_empty(), "a frame between steps was drawn");
    frames
}

/// The drawn cards of `frame` at `place`.
fn drawn_at(frame: &Frame, place: Place) -> Vec<&Resolved> {
    (frame.resolved.iter())
        .filter(|element| element.role == Role::Card && element.place == place.into())
        .filter(|element| element.opacity > 0.0)
        .collect()
}

/// `before`'s view with the script `acts`, one beat each, after `change`.
fn scripted(
    before: &GameUpdate,
    acts: Vec<Act>,
    change: impl FnOnce(&mut PlayerView),
) -> GameUpdate {
    let mut script = ScriptBuilder::default();
    for act in acts {
        script.beat([act]);
    }
    let mut view = before.view.clone();
    change(&mut view);
    view.script = Some(script.build());
    GameUpdate {
        session: before.session,
        view,
    }
}

/// A triggered ability of the viewer's with no source on the table.
fn trigger() -> StackItem {
    StackItem::Ability {
        source: None,
        ability: 0,
        granted: false,
        name: "Storm".to_owned(),
        triggered: true,
        taps_to_pay: None,
        nth: 0,
        text: None,
        controller: Side::You,
        manual: None,
        targets: Vec::new(),
        x: 0,
    }
}

fn stack_case() -> (GameUpdate, GameUpdate) {
    let mut view = ViewBuilder::new();
    // Seven spells, of which the phone's stack shows five and scrolls the rest.
    for index in 0..7 {
        view.card(ZoneName::Stack, Side::You, &format!("Spell {index}"));
    }
    view.script(ScriptBuilder::reset().build());
    let before = view.update();
    let first = u16::try_from(before.view.objects.len()).expect("few cards");
    let copies: Vec<ObjectId> = (first..first + 3).map(ObjectId).collect();
    // As Storm puts each copy on the stack, its trigger goes on the stack above the spells,
    // the copy appears, and the trigger leaves. Each step pushes the item at the window's
    // nearer end out of view where it stands, or brings one back, and moves every item.
    let mut acts = Vec::new();
    for (index, &slot) in copies.iter().enumerate() {
        let id = 100 + u32::try_from(index).expect("few copies");
        acts.push(Act::Emerge {
            id,
            item: trigger(),
            at: None,
        });
        acts.push(Act::Appear {
            from: ZoneName::Nowhere,
            why: Why::Created,
            object: object(slot, ZoneName::Stack, Side::You, Some("Copy")),
            at: None,
            item: Some(StackItem::spell(slot)),
        });
        acts.push(Act::Vanish {
            subject: Subject::Ability { id },
            from: ZoneName::Stack,
            why: Why::Resolve,
            object: None,
            at: None,
        });
    }
    let after = scripted(&before, acts, |view| {
        for &slot in &copies {
            view.objects
                .push(object(slot, ZoneName::Stack, Side::You, Some("Copy")));
            view.stack.push(StackItem::spell(slot));
        }
    });
    (before, after)
}

#[test]
fn a_stack_item_pushed_out_of_view_stays_undrawn_as_more_copies_arrive() {
    let (before, after) = stack_case();
    let stack = Place::Card {
        zone: ZoneName::Stack,
        side: Side::You,
        row: None,
    };
    let window = compact_window(393.0, 759.0);
    for frame in still_frames(393.0, 759.0, &before, &after) {
        let boxes: Vec<[f32; 4]> = drawn_at(&frame, stack).into_iter().map(bounds).collect();
        for (index, a) in boxes.iter().enumerate() {
            assert!(
                !boxes[index + 1..].contains(a),
                "two stack cards share the box {a:?}"
            );
        }
        assert_eq!(card_faults(&frame, window, reachable), Vec::<String>::new());
    }
}

fn lands_case() -> (GameUpdate, GameUpdate) {
    let mut view = ViewBuilder::new();
    let lands: Vec<ObjectId> = (0..8)
        .map(|_| {
            let land = view.card(ZoneName::Battlefield, Side::You, "Mountain");
            land.types = vec![CardType::Land];
            land.tapped = true;
            land.slot
        })
        .collect();
    view.card(ZoneName::Battlefield, Side::You, "Life and Limb")
        .types = vec![CardType::Enchantment];
    view.script(ScriptBuilder::reset().build());
    let before = view.update();
    let exiled = |slot: ObjectId| {
        let mut record = (before.view.objects.iter())
            .find(|object| object.slot == slot)
            .expect("the land is in the view")
            .clone();
        record.zone = ZoneName::Exile;
        record.tapped = false;
        record
    };
    let acts = (lands.iter())
        .map(|&slot| Act::Vanish {
            subject: Subject::Object { slot },
            from: ZoneName::Battlefield,
            why: Why::Exile,
            object: Some(exiled(slot)),
            at: None,
        })
        .collect();
    let after = scripted(&before, acts, |view| {
        view.battlefield.retain(|slot| !lands.contains(slot));
        for object in &mut view.objects {
            if lands.contains(&object.slot) {
                *object = exiled(object.slot);
            }
        }
    });
    (before, after)
}

#[test]
fn lands_beyond_a_scrolled_row_stay_undrawn_while_the_lands_before_them_leave() {
    let (before, after) = lands_case();
    let window = compact_window(375.0, 647.0);
    for frame in still_frames(375.0, 647.0, &before, &after) {
        assert_eq!(card_faults(&frame, window, reachable), Vec::<String>::new());
    }
}
