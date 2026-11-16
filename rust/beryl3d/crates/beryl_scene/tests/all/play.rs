//! The script clock: scripts compiled whole, played in order and coming to rest where placement
//! puts every object, whatever the frames between (docs/beryl-3d.md, "Determinism" and "Verbs").

use crate::common::{
    MS, before, discard, draw, emerge, empty_frame, frame, resolve, show, three_beats, window,
};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{Side, StackItem, Why, ZoneName};
use beryl_protocol::script::{Act, Fact, Step, Subject};
use beryl_scene::frame::Wake;
use beryl_scene::play::Show;
use beryl_scene::pose::Curve;
use beryl_scene::testing::{ScriptBuilder, timing};

/// Every object of `update`'s view stands where placement puts it in the show's window.
fn at_rest(show: &Show, update: &GameUpdate, now: u64) {
    let placement = show
        .table()
        .placement(&update.view, show.table().abilities());
    for object in &update.view.objects {
        let slot = usize::from(object.slot.0);
        let rest = placement.get(object.slot).map(|rest| rest.pose);
        assert_eq!(
            show.pose(slot, now),
            rest,
            "card #{slot} is at its placement"
        );
    }
}

/// A show that has taken the table before and the three-beat script, with one frame at 0.
fn three_beat_show() -> (Show, GameUpdate, crate::common::Cards) {
    let (start, cards) = before();
    let (after, timings) = three_beats(&start, cards);
    let mut show = show();
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(after.clone()), timings);
    frame(&mut show, &mut empty_frame(), 0);
    (show, after, cards)
}

#[test]
fn a_script_of_three_beats_plays_to_its_end() {
    let (start, _) = before();
    let (mut show, after, cards) = three_beat_show();
    let land = usize::from(cards.land.0);
    let token = usize::from(cards.token.0);
    let placement_before = show.table().placement(&start.view, &[]);
    let placement_after = show.table().placement(&after.view, &[]);
    let held = placement_before.get(cards.land).expect("in the hand").pose;
    let landed = placement_after
        .get(cards.land)
        .expect("on the battlefield")
        .pose;
    let mut scratch = empty_frame();

    assert_eq!(
        show.pose(land, 0),
        Some(held),
        "the Plains starts in the hand"
    );
    frame(&mut show, &mut scratch, 210 * MS);
    let flying = show.pose(land, 210 * MS).expect("the Plains is drawn");
    assert!(
        flying.position != held.position && flying.position != landed.position,
        "halfway through its flight the Plains is between the hand and the battlefield"
    );
    // The viewer's land glides straight to its place: halfway through its time it is on the
    // straight line. The Plains flies to its place in the row as the first beat leaves it, which
    // the token's arrival later shifts by a little, so the line to its final place is a close
    // stand-in.
    let along = Curve::EASE_OUT.ease(0.5);
    let straight = held.position.lerp(landed.position, along);
    assert!(
        (flying.position.z - straight.z).abs() < 1.0,
        "the land glides on the straight line: {} against {}",
        flying.position.z,
        straight.z
    );
    assert_eq!(show.wake(210 * MS), Wake::Now, "a segment is under way");

    frame(&mut show, &mut scratch, 570 * MS);
    let fading = show.pose(token, 570 * MS).expect("the token is drawn");
    assert!(
        fading.opacity > 0.0 && fading.opacity < 1.0,
        "the token fades in during the second beat, at {}",
        fading.opacity
    );
    assert_eq!(show.shown(), Some(&start), "the view waits for its script");

    frame(&mut show, &mut scratch, 870 * MS);
    assert_eq!(
        show.shown(),
        Some(&after),
        "the script is over after 870 ms"
    );
    at_rest(&show, &after, 870 * MS);
    assert_eq!(show.wake(870 * MS), Wake::Never);
    assert_eq!(
        show.next_unbuilt(),
        None,
        "every verb of the script is built"
    );
}

#[test]
fn a_moment_drawn_directly_equals_the_same_moment_after_every_tick() {
    let (mut direct, ..) = three_beat_show();
    let (mut stepped, ..) = three_beat_show();
    let mut scratch = empty_frame();
    let moments = [333 * MS, 777 * MS];
    let mut ticks = Vec::new();
    for tick in 1..=800 {
        let now = tick * MS;
        let drawn = frame(&mut stepped, &mut scratch, now);
        if moments.contains(&now) {
            ticks.push(drawn);
        }
    }
    for (moment, ticked) in moments.iter().zip(&ticks) {
        let mut drawn = frame(&mut direct, &mut scratch, *moment);
        // The picture is the same; the sounds a frame lists are those that began since the frame
        // before it, so a frame drawn in one jump lists all of them.
        drawn.sounds.clear();
        let mut ticked = ticked.clone();
        ticked.sounds.clear();
        assert_eq!(drawn, ticked, "the frame at {} ms", moment / MS);
    }
}

#[test]
fn an_update_that_arrives_during_a_script_plays_after_it() {
    let (mut show, after, cards) = three_beat_show();
    let (next, timings) = discard(&after, cards);
    let card = usize::from(cards.discard.0);
    let mut scratch = empty_frame();
    frame(&mut show, &mut scratch, 100 * MS);
    let in_hand = show
        .table()
        .placement(&after.view, &[])
        .get(cards.discard)
        .map(|r| r.pose);
    show.push(Box::new(next.clone()), timings);
    assert_eq!(show.waiting(), 1);
    assert_eq!(show.latest(), Some(&next));

    frame(&mut show, &mut scratch, 500 * MS);
    assert_eq!(
        show.pose(card, 500 * MS),
        in_hand,
        "the Counterspell stays in the hand, closed up round the Plains, while the first script \
         plays"
    );
    assert_eq!(show.waiting(), 1);

    // The second script starts as the first ends, at 870 ms, whenever the frames fall.
    frame(&mut show, &mut scratch, 1070 * MS);
    assert_eq!(show.shown(), Some(&after));
    assert_eq!(show.waiting(), 0);
    let flying = show.pose(card, 1070 * MS);
    assert!(
        flying != in_hand
            && flying
                != show
                    .table()
                    .placement(&next.view, &[])
                    .get(cards.discard)
                    .map(|r| r.pose),
        "halfway through the second script the Counterspell is on its way"
    );
    frame(&mut show, &mut scratch, 1270 * MS);
    assert_eq!(show.shown(), Some(&next));
    at_rest(&show, &next, 1270 * MS);
}

#[test]
fn a_resize_in_mid_flight_keeps_the_pose_and_arrives_at_the_new_place() {
    let (start, cards) = before();
    let (drawn, timings) = draw(&start, cards);
    let mut show = show();
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(drawn.clone()), timings);
    let mut scratch = empty_frame();
    frame(&mut show, &mut scratch, 0);
    let card = usize::from(cards.top.0);
    let old_place = show
        .table()
        .placement(&drawn.view, &[])
        .get(cards.top)
        .map(|r| r.pose);

    frame(&mut show, &mut scratch, 150 * MS);
    let was = show.pose(card, 150 * MS).expect("the card is drawn");
    show.resize(window(2560.0, 1080.0));
    frame(&mut show, &mut scratch, 150 * MS);
    let now = show.pose(card, 150 * MS).expect("the card is drawn");
    assert!(
        was.position.distance(now.position) < 1e-3
            && was.rotation.dot(now.rotation).abs() > 1.0 - 1e-6
            && (was.scale - now.scale).abs() < 1e-5
            && (was.turn - now.turn).abs() < 1e-5,
        "the card does not jump: {was:?} became {now:?}"
    );
    let new_place = show
        .table()
        .placement(&drawn.view, &[])
        .get(cards.top)
        .map(|r| r.pose);
    assert_ne!(old_place, new_place, "the hand moved with the window");
    let later = show.pose(card, 300 * MS);
    assert_ne!(later, Some(was), "the card goes on moving");

    frame(&mut show, &mut scratch, 400 * MS);
    assert_eq!(show.shown(), Some(&drawn));
    at_rest(&show, &drawn, 400 * MS);
}

#[test]
fn a_long_queue_plays_faster() {
    let (start, cards) = before();
    let mut show = show();
    show.push(Box::new(start.clone()), Vec::new());
    let mut scratch = empty_frame();
    frame(&mut show, &mut scratch, 0);
    // Twelve updates, each drawing nothing and holding 400 ms with a discard script whose
    // card is already in the graveyard after the first.
    let (first, timings) = discard(&start, cards);
    for _ in 0..12 {
        show.push(Box::new(first.clone()), timings.clone());
    }
    // More than ten waiting: the first plays at twice the speed and is over at 200 ms.
    frame(&mut show, &mut scratch, 1);
    assert_eq!(show.waiting(), 11);
    frame(&mut show, &mut scratch, 201 * MS);
    assert_eq!(
        show.waiting(),
        10,
        "the first script ended at twice the speed"
    );
}

/// A draw that takes 1,050 ms: the cards already in the hand go to their new places in the
/// 170 ms the step's `room` gives from when the drawn card's act is applied, and are at rest
/// then, long before the drawn card lands; they were still on their way half way through.
#[test]
fn the_hand_regroups_in_the_time_its_room_gives_whatever_the_draw_takes() {
    use beryl_protocol::script::Room;
    let (start, cards) = before();
    let (after, mut timings) = draw(&start, cards);
    timings[0].duration = 1050.0;
    timings[0].room = Room::For { ms: 170.0 };
    let mut show = show();
    show.push(Box::new(start.clone()), Vec::new());
    show.push(Box::new(after.clone()), timings);
    frame(&mut show, &mut empty_frame(), 0);
    let placement = show.table().placement(&after.view, &[]);
    let held: Vec<_> = start
        .view
        .objects
        .iter()
        .filter(|o| o.zone == beryl_protocol::protocol::ZoneName::Hand)
        .map(|o| o.slot)
        .collect();
    assert_eq!(held.len(), 3, "three cards were in the hand");
    let rests = |slot: beryl_protocol::protocol::ObjectId| placement.get(slot).map(|r| r.pose);
    let moved = held
        .iter()
        .filter(|&&slot| show.pose(usize::from(slot.0), 0) != rests(slot))
        .count();
    assert!(moved > 0, "the draw changes where the hand's cards rest");
    for &slot in &held {
        let index = usize::from(slot.0);
        assert_eq!(
            show.pose(index, 170 * MS),
            rests(slot),
            "card #{index} at rest"
        );
    }
    assert!(
        held.iter()
            .any(|&slot| show.pose(usize::from(slot.0), 85 * MS) != rests(slot)),
        "a card of the hand is still on its way half way through the regroup"
    );
    let top = usize::from(cards.top.0);
    assert_ne!(
        show.pose(top, 170 * MS),
        rests(cards.top),
        "the drawn card is still in flight"
    );
}

#[test]
fn a_step_the_table_cannot_take_leaves_the_steps_after_it_placed() {
    // The first beat places the Plains' groups. The second names an ability that is not on
    // the stack, so the table cannot take it and the third step places every group again.
    // That step used to panic, clearing the groups' record of their slots before reading it.
    let (start, cards) = before();
    let (mut after, mut timings) = crate::common::three_beats(&start, cards);
    let script = after.view.script.as_mut().expect("a script");
    script.beats.insert(
        1,
        vec![Step {
            act: Act::Vanish {
                subject: Subject::Ability { id: 999 },
                from: ZoneName::Stack,
                why: Why::Resolve,
                object: None,
                at: None,
            },
            cause: None,
        }],
    );
    timings.insert(1, timing(100.0));
    let mut show = show();
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(after.clone()), timings);
    let mut scratch = empty_frame();
    for ms in (0..=1_500).step_by(50) {
        frame(&mut show, &mut scratch, ms * MS);
    }
    at_rest(&show, &after, 1_500 * MS);
}

#[test]
fn a_step_about_an_ability_the_view_did_not_number_does_not_stop_the_show() {
    // A view's stack gives an engine ability no number: the numbers come from the scripts
    // before it. A step that changes the ability numbered 7 used to panic comparing it with
    // an ability that had none, instead of placing the table whole.
    let (mut start, cards) = before();
    let ability = StackItem::Ability {
        source: Some(cards.angel),
        ability: 0,
        granted: false,
        name: "Serra Angel".to_owned(),
        triggered: true,
        taps_to_pay: None,
        nth: 0,
        text: None,
        controller: Side::You,
        manual: None,
        targets: Vec::new(),
        x: 0,
    };
    start.view.stack.push(ability.clone());
    let mut after = start.clone();
    after.view.script = Some(
        ScriptBuilder::default()
            .beat([Act::Set {
                fact: Fact::Item {
                    subject: Subject::Ability { id: 7 },
                    item: ability,
                },
            }])
            .build(),
    );
    let mut show = show();
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(after.clone()), vec![timing(100.0)]);
    let mut scratch = empty_frame();
    for ms in (0..=500).step_by(50) {
        frame(&mut show, &mut scratch, ms * MS);
    }
    at_rest(&show, &after, 500 * MS);
}

#[test]
fn a_later_script_finds_the_ability_an_earlier_script_numbered() {
    // The first script puts an ability on the stack as number 7; the second changes it, then
    // resolves it, both by that number, and then discards a card. A view's stack numbers no
    // engine ability, so a show that built each script's table from the view before could not
    // find ability 7: those steps failed and the discard was placed whole.
    let (start, cards) = before();
    let ability = |x| StackItem::Ability {
        source: Some(cards.angel),
        ability: 0,
        granted: false,
        name: "Serra Angel".to_owned(),
        triggered: true,
        taps_to_pay: None,
        nth: 0,
        text: None,
        controller: Side::You,
        manual: None,
        targets: Vec::new(),
        x,
    };
    let mut emerged = start.clone();
    emerged.view.stack.push(ability(0));
    emerged.view.script = Some(
        ScriptBuilder::default()
            .beat([Act::Emerge {
                id: 7,
                item: ability(0),
                at: None,
            }])
            .build(),
    );
    let (mut resolved, mut timings) = discard(&emerged, cards);
    resolved.view.stack.clear();
    let script = resolved.view.script.as_mut().expect("a script");
    script.beats.insert(
        0,
        vec![Step {
            act: Act::Set {
                fact: Fact::Item {
                    subject: Subject::Ability { id: 7 },
                    item: ability(2),
                },
            },
            cause: None,
        }],
    );
    script.beats.insert(
        1,
        vec![Step {
            act: Act::Vanish {
                subject: Subject::Ability { id: 7 },
                from: ZoneName::Stack,
                why: Why::Resolve,
                object: None,
                at: None,
            },
            cause: None,
        }],
    );
    timings.splice(0..0, [timing(100.0), timing(100.0)]);
    let mut show = show();
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(emerged), vec![timing(100.0)]);
    show.push(Box::new(resolved.clone()), timings);
    assert_eq!(show.unplaced(), 0, "every step finds what it names");
    let mut scratch = empty_frame();
    // The first script ends at 100 ms and the second at 700 ms; the discard flies from 300 ms.
    frame(&mut show, &mut scratch, 0);
    frame(&mut show, &mut scratch, 100 * MS);
    let slot = usize::from(cards.discard.0);
    let rest = show
        .table()
        .placement(&resolved.view, &[])
        .get(cards.discard)
        .map(|r| r.pose);
    let flying = show.pose(slot, 500 * MS);
    assert!(
        flying.is_some() && flying != rest,
        "the discard is in flight"
    );
    for ms in (150..=1_000).step_by(50) {
        frame(&mut show, &mut scratch, ms * MS);
    }
    at_rest(&show, &resolved, 1_000 * MS);
}

#[test]
fn a_fading_card_is_drawn_with_the_blended_records() {
    // The opaque pass neither blends nor shows what lies behind, so a card drawn there while
    // it fades in would darken from black and hide the cards under it (docs/beryl-3d.md,
    // "Opaque first, nearest first").
    let (mut show, _, cards) = three_beat_show();
    let drawn = frame(&mut show, &mut empty_frame(), 570 * MS);
    let token = show.pose(usize::from(cards.token.0), 570 * MS);
    assert!(
        token.is_some_and(|pose| pose.opacity < 1.0),
        "the token fades in"
    );
    let card = beryl_scene::frame::Material::Card as u8;
    let (opaque, blended) = drawn.draws.split_at(drawn.opaque as usize);
    assert!(opaque.iter().all(|draw| draw.tint[3] == 255));
    assert_eq!(
        blended
            .iter()
            .filter(|draw| draw.material == card && draw.tint[3] < 255)
            .count(),
        1,
        "the fading token is among the blended records"
    );
}

#[test]
fn an_ability_that_emerges_ends_on_the_stack_and_one_that_vanishes_leaves_it() {
    let (start, cards) = before();
    let (emerged, emerge_timings) = emerge(&start, cards, 5);
    let (resolved, resolve_timings) = resolve(&emerged, 5);
    let mut show = show();
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(emerged.clone()), emerge_timings);
    let mut scratch = empty_frame();
    // The emerge plays from 0 to 200 ms.
    frame(&mut show, &mut scratch, 0);
    frame(&mut show, &mut scratch, 300 * MS);
    assert_eq!(show.table().abilities(), [5]);
    let rest = show
        .table()
        .placement(&emerged.view, &[5])
        .ability(5)
        .map(|rest| rest.pose);
    assert!(rest.is_some(), "the ability has a place on the stack");
    assert_eq!(show.ability_pose(5, 300 * MS), rest);
    assert_eq!(
        show.reach(Subject::Ability { id: 5 }, 300 * MS)
            .map(|reach| reach.pose),
        rest,
        "light reaches the ability where it rests"
    );
    at_rest(&show, &emerged, 300 * MS);

    show.push(Box::new(resolved.clone()), resolve_timings);
    for ms in (300..=700).step_by(50) {
        frame(&mut show, &mut scratch, ms * MS);
    }
    assert!(show.table().abilities().is_empty());
    assert_eq!(show.ability_pose(5, 700 * MS), None);
    at_rest(&show, &resolved, 700 * MS);
}
