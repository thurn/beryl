//! `fly`'s cases (`data/ui-conventions.json`, `fly`): each plays to its end with every card at its
//! placement and no jump, and each case's path is the one the web interface gives it: a spell
//! cast bows toward its caster's edge, a permanent resolving is thrown up off the stack, the
//! viewer's land glides straight, drawn in front of the hand it leaves, and the opponent's swings
//! out into the table first.

use crate::common::{MS, empty_frame, frame, show, window};
use app_client::driver::{Duration, Mode, Moment, Page, Recording};
use beryl_card_records::{TokenMakers, fixture_message};
use beryl_protocol::GameUpdate;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{CardType, ObjectId, Side, StackItem, Why, ZoneName};
use beryl_protocol::script::{Act, Curve, Room, StepTiming, Verb};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, timing};
use game_play::play::Show;
use game_play::play::record::ScriptRecord;
use game_probe::agent::steps::{no_jumps, rest_equals_placement, step_times};
use game_table::table::Window;
use glam::Vec3;
use kit_base::pose::{Pose, Segment};
use kit_probe::agent::Rule;
use kit_space::motion::pose_at;
use kit_ui::input::Size;
use std::sync::Arc;

#[path = "data_reload.rs"]
mod data_reload;

/// The cards of each side that the flights move.
#[derive(Clone, Copy)]
struct Hands {
    land: ObjectId,
    spell: ObjectId,
    other: ObjectId,
    /// A creature spell on the stack.
    resolving: ObjectId,
}

/// A table with a library, a hand of a land and two spells, two lands, and a creature spell on
/// the stack for each side, with a reset script.
fn table() -> (GameUpdate, [Hands; 2]) {
    let mut builder = ViewBuilder::new();
    let mut hands = Vec::new();
    for side in [Side::You, Side::Opponent] {
        for _ in 0..5 {
            builder.card(ZoneName::Library, side, "Island");
        }
        let land = builder.card(ZoneName::Hand, side, "Plains");
        land.types = vec![CardType::Land];
        let land = land.slot;
        let spell = builder.card(ZoneName::Hand, side, "Counterspell").slot;
        let other = builder.card(ZoneName::Hand, side, "Memory Lapse").slot;
        for name in ["Island", "Plains"] {
            builder.card(ZoneName::Battlefield, side, name).types = vec![CardType::Land];
        }
        let resolving = builder.card(ZoneName::Stack, side, "Serra Angel");
        resolving.types = vec![CardType::Creature];
        hands.push(Hands {
            land,
            spell,
            other,
            resolving: resolving.slot,
        });
    }
    builder.script(ScriptBuilder::reset().build());
    (builder.update(), [hands[0], hands[1]])
}

/// The timing the session sends for `case`, with `fly`'s values for it in
/// `data/ui-conventions.json`; a step that takes a card into or out of a hand makes room for
/// 170 ms, as `fly`'s `regroup` says.
fn timed(case: &str, duration: f64, delay: f64, curve: Curve, room: bool) -> StepTiming {
    StepTiming {
        case: Some(case.to_owned()),
        delay,
        curve,
        room: if room {
            Room::For { ms: 170.0 }
        } else {
            Room::Step
        },
        ..timing(duration)
    }
}

/// The view after the card in `slot` flies from `from` into `zone`, with that one step.
fn flown(
    before: &GameUpdate,
    slot: ObjectId,
    from: ZoneName,
    zone: ZoneName,
    why: Why,
) -> GameUpdate {
    let mut view = before.view.clone();
    let object = view
        .objects
        .iter_mut()
        .find(|o| o.slot == slot)
        .expect("the card is on the table");
    object.zone = zone;
    if object.name.is_none() {
        object.name = Some("Plains".to_owned());
    }
    let object = object.clone();
    let side = object.controller;
    let player = match side {
        Side::You => &mut view.players.you,
        Side::Opponent => &mut view.players.opponent,
    };
    view.stack
        .retain(|item| !matches!(item, StackItem::Spell { slot: s, .. } if *s == slot));
    let item = match zone {
        ZoneName::Battlefield => {
            view.battlefield.push(slot);
            None
        }
        ZoneName::Graveyard => {
            player.graveyard.push(slot);
            None
        }
        ZoneName::Stack => {
            let item = StackItem::Spell {
                slot,
                targets: Vec::new(),
                x: 0,
                paid: Vec::new(),
            };
            view.stack.push(item.clone());
            Some(item)
        }
        _ => None,
    };
    view.script = Some(
        ScriptBuilder::default()
            .beat([Act::Fly {
                from,
                why,
                object,
                at: None,
                item,
                origin: None,
            }])
            .build(),
    );
    GameUpdate {
        session: before.session,
        view,
    }
}

#[test]
fn the_hosts_flight_arc_moves_the_card_in_the_frame() {
    let (before, [mine, _]) = table();
    let mut after = flown(
        &before,
        mine.spell,
        ZoneName::Hand,
        ZoneName::Graveyard,
        Why::Discard,
    );
    after.view.script.as_mut().unwrap().timing =
        vec![timed("discard", 165.0, 0.0, Curve::EaseOut, true)];
    let recording = Arc::new(Recording(vec![
        FixtureMessage::game_update(&before),
        FixtureMessage::game_update(&after),
    ]));
    let at_middle = |data| {
        let mut page = Page::new(
            data,
            Arc::clone(&recording),
            Side::You,
            window(1440.0, 900.0).size,
        )
        .unwrap();
        let flight = page
            .steps()
            .into_iter()
            .find(|step| step.verb == Verb::Fly)
            .unwrap();
        let locator = format!("card #{}", mine.spell.0);
        let band_at = |page: &mut Page, at| {
            page.seek(&Moment::At(Duration(at))).unwrap();
            let frame = page.frame();
            let card = frame
                .resolved
                .iter()
                .find(|node| node.id.0 == u32::from(mine.spell.0) && node.pose.is_some())
                .expect("the flight's card is in the scene");
            kit_frame::frame::PaintBand::of(card)
        };
        assert_eq!(
            band_at(&mut page, flight.middle),
            kit_frame::frame::PaintBand::Moving
        );
        let bounds = page.find(&locator).rect;
        assert_eq!(
            band_at(&mut page, flight.to),
            kit_frame::frame::PaintBand::Table
        );
        bounds
    };
    let mut data = (*crate::test_data::data()).clone();
    data.verbs.fly.arc = 0.0;
    let flat = at_middle(Arc::new(data.clone()));
    data.verbs.fly.arc = 55.0;
    assert_ne!(flat, at_middle(Arc::new(data)));
}

/// A recording show that has taken `before` and then `after`, played frame by frame at 60 Hz
/// until both have ended.
fn played(before: &GameUpdate, after: &GameUpdate, timings: Vec<StepTiming>) -> Show {
    played_in(before, after, timings, window(1440.0, 900.0))
}

/// `played`, in `window`.
fn played_in(
    before: &GameUpdate,
    after: &GameUpdate,
    timings: Vec<StepTiming>,
    window: Window,
) -> Show {
    let mut show = show();
    show.resize(window);
    show.record();
    show.push(Box::new(before.clone()), Vec::new());
    show.push(Box::new(after.clone()), timings);
    let mut scratch = empty_frame();
    let mut now = 0;
    frame(&mut show, &mut scratch, now);
    while !show.settled() && now < 5_000 * MS {
        now += 16 * MS;
        frame(&mut show, &mut scratch, now);
    }
    assert!(show.settled(), "the scripts end");
    show
}

/// The flight's script record, the last the show was given.
fn flight(show: &Show) -> &ScriptRecord {
    show.history().last().expect("the flight was recorded")
}

/// When the flight's one step begins and ends, on its script's clock.
fn span(show: &Show) -> (u64, u64) {
    let step = &flight(show).steps[0];
    (step.from, step.to)
}

/// Where the card in `slot` is at `t` on the flight's script's clock, as its segments have it.
fn pose(show: &Show, slot: usize, t: u64) -> Pose {
    let segments: Vec<Segment> = flight(show)
        .track(slot)
        .iter()
        .map(|record| record.segment)
        .collect();
    assert!(
        !segments.is_empty(),
        "card #{slot} moves in the flight's script"
    );
    pose_at(&segments, t)
}

/// No segment or step check finds a fault in any script the show played.
fn clean(show: &Show, case: &str) {
    for script in show.history() {
        let found: Vec<_> = no_jumps(script)
            .into_iter()
            .chain(rest_equals_placement(script))
            .chain(step_times(script))
            .collect();
        assert!(found.is_empty(), "{case}: {found:?}");
    }
}

fn off_line(pose: Pose, from: Pose, to: Pose) -> Vec3 {
    let way = to.position - from.position;
    let at = pose.position - from.position;
    at - way * (at.dot(way) / way.length_squared())
}

/// The camera's right, up and backward directions.
fn axes(show: &Show) -> [Vec3; 3] {
    show.table().camera().axes()
}

/// Each case's script, with the case's duration and delay.
fn cases() -> Vec<(&'static str, GameUpdate, GameUpdate, Vec<StepTiming>)> {
    let (before, [mine, theirs]) = table();
    let land = flown(
        &before,
        mine.land,
        ZoneName::Hand,
        ZoneName::Battlefield,
        Why::Play,
    );
    let their_land = flown(
        &before,
        theirs.land,
        ZoneName::Hand,
        ZoneName::Battlefield,
        Why::Play,
    );
    // The permanent spell in `slot` resolving, which the session times alike for either side.
    let resolves = |case, slot| {
        let after = flown(
            &before,
            slot,
            ZoneName::Stack,
            ZoneName::Battlefield,
            Why::Resolve,
        );
        let timing = timed("permanent resolves", 520.0, 330.0, Curve::EaseOut, false);
        (case, before.clone(), after, vec![timing])
    };
    let resolving = [
        resolves("permanent resolves", mine.resolving),
        resolves("opponent permanent resolves", theirs.resolving),
    ];
    let discard = flown(
        &before,
        mine.spell,
        ZoneName::Hand,
        ZoneName::Graveyard,
        Why::Discard,
    );
    let cast = flown(
        &before,
        mine.spell,
        ZoneName::Hand,
        ZoneName::Stack,
        Why::Cast,
    );
    let their_cast = flown(
        &before,
        theirs.spell,
        ZoneName::Hand,
        ZoneName::Stack,
        Why::Cast,
    );
    vec![
        (
            "land played",
            before.clone(),
            land,
            vec![timed("land played", 315.0, 0.0, Curve::EaseIn, true)],
        ),
        (
            "opponent land played",
            before.clone(),
            their_land,
            vec![timed(
                "opponent land played",
                615.0,
                0.0,
                Curve::EaseOut,
                true,
            )],
        ),
        (
            "discard",
            before.clone(),
            discard,
            vec![timed("discard", 165.0, 0.0, Curve::EaseOut, true)],
        ),
        (
            "own spell cast",
            before.clone(),
            cast,
            vec![timed("own spell cast", 370.0, 0.0, Curve::EaseOut, true)],
        ),
        (
            "opponent spell cast",
            before,
            their_cast,
            vec![timed(
                "opponent spell cast",
                400.0,
                0.0,
                Curve::EaseOut,
                true,
            )],
        ),
    ]
    .into_iter()
    .chain(resolving)
    .collect()
}

#[test]
fn every_case_plays_to_its_placement_with_no_jump_for_its_time() {
    for (case, before, after, timings) in cases() {
        let show = played(&before, &after, timings);
        clean(&show, case);
    }
}

/// A spell cast, by either player, is off the straight way at the middle of its time, toward
/// its caster's edge of the table: the viewer's down the screen, the opponent's up it, by a
/// share of the way.
#[test]
fn a_cast_bows_toward_its_casters_edge() {
    for (case, edge) in [("own spell cast", -1.0), ("opponent spell cast", 1.0)] {
        let (_, before, after, timings) = cases()
            .into_iter()
            .find(|(name, ..)| *name == case)
            .expect("the case");
        let show = played(&before, &after, timings);
        let script = flight(&show);
        let slot = script.steps[0].subject.expect("a card flies");
        let (start, end) = span(&show);
        let from = pose(&show, slot, start);
        let to = pose(&show, slot, end);
        let middle = pose(&show, slot, u64::midpoint(start, end));
        // A straight way in the table's space is a straight line on the window, so the bow is
        // measured there, as the web interface draws it.
        let camera = show.table().camera();
        let [a, b, m] = [from, to, middle].map(|pose| camera.to_screen(pose.position));
        let line = b - a;
        let off = (m - a) - line * ((m - a).dot(line) / line.length_squared());
        // The window's y runs down it, and the viewer's edge is its bottom.
        assert!(
            off.y * -edge > 0.0,
            "{case}: bows toward its caster's edge: {off:?}"
        );
        assert!(
            off.length() > 0.03 * line.length(),
            "{case}: bows by a share of its way on the screen: {off:?} of {line:?}"
        );
    }
}

/// A permanent resolving is at its greatest height by a quarter of its time, and is
/// half way across after a quarter of its time.
#[test]
fn a_resolving_permanent_is_thrown_up_and_half_way_across_at_a_quarter() {
    let (_, before, after, timings) = cases()
        .into_iter()
        .find(|(name, ..)| *name == "permanent resolves")
        .expect("the case");
    let show = played(&before, &after, timings);
    let slot = flight(&show).steps[0].subject.expect("a card flies");
    let (start, end) = span(&show);
    let from = pose(&show, slot, start);
    let to = pose(&show, slot, end);
    let quarter = start + (end - start) / 4;
    let [_, up, _] = axes(&show);
    let height = |t: u64| off_line(pose(&show, slot, t), from, to).dot(up);
    let highest = (start..=end)
        .step_by(usize::try_from(MS).expect("small"))
        .map(|t| (t, height(t)))
        .fold(
            (start, f32::MIN),
            |best, next| if next.1 > best.1 { next } else { best },
        );
    assert!(
        highest.1 > 20.0,
        "it rises well above the straight way: {highest:?}"
    );
    // It stays at its greatest height from a fifth of its time to over half of it.
    let at_quarter = height(quarter);
    assert!(
        at_quarter > 0.97 * highest.1,
        "at its greatest height by a quarter of its time: {at_quarter} of {highest:?}"
    );
    let across = progress(pose(&show, slot, quarter), from, to);
    assert!(
        (across - 0.5).abs() < 0.08,
        "half way across at a quarter of its time: {across}"
    );
}

/// A permanent resolving never rises past the top of the window's safe area, as the web's
/// `lifted` caps its rise at the room over its start and its place. The desktop layout leaves
/// room for the full `thrown.height` over every row, because the opponent's hand lies above
/// them, so both rise that far in a plain window. With a safe area whose top lies near the
/// stack's, there is less room: the opponent's then rises less, and its top stays inside the
/// safe area all the way.
#[test]
fn a_thrown_permanent_never_rises_past_the_top_of_the_window() {
    // Under the top of the opponent's creatures' row, which stands where the web interface's
    // does, so that the room over the card's place is all the safe area caps.
    let inset = window_inset(220.0);
    for (case, window, capped) in [
        ("permanent resolves", window(1440.0, 900.0), false),
        ("opponent permanent resolves", window(1440.0, 900.0), false),
        ("opponent permanent resolves", inset, true),
    ] {
        let (_, before, after, timings) = cases()
            .into_iter()
            .find(|(name, ..)| *name == case)
            .expect("the case");
        let show = played_in(&before, &after, timings, window);
        let slot = flight(&show).steps[0].subject.expect("a card flies");
        let (start, end) = span(&show);
        let camera = show.table().camera();
        let (from, to) = (pose(&show, slot, start), pose(&show, slot, end));
        let [_, up, _] = axes(&show);
        let mut highest = f32::MIN;
        for t in (start..=end).step_by(usize::try_from(MS).expect("small")) {
            let now = pose(&show, slot, t);
            let (corners, _) = camera.card_box(&now);
            let top = corners.iter().map(|c| c.y).fold(f32::INFINITY, f32::min);
            let edge = window.safe_area.top;
            assert!(
                top > edge - 1.0,
                "{case}: its top is at {top} points, above the safe area's at {edge}, at {t}"
            );
            highest = highest.max(off_line(pose(&show, slot, t), from, to).dot(up));
        }
        let full = show.table().data().verbs.fly.thrown.height
            * crate::test_data::data().card.width
            * from.scale;
        if capped {
            assert!(highest < 0.8 * full, "{case}: rises {highest} of {full}");
        } else {
            assert!(highest > 0.9 * full, "{case}: rises {highest} of {full}");
        }
    }
}

/// A 1440 by 900 window whose safe area begins `top` points below its top.
fn window_inset(top: f32) -> Window {
    let mut window = window(1440.0, 900.0);
    window.safe_area.top = top;
    window
}

/// How far along the way from `from` to `to` `pose` is, from 0 to 1.
fn progress(pose: Pose, from: Pose, to: Pose) -> f32 {
    let way = to.position - from.position;
    (pose.position - from.position).dot(way) / way.length_squared()
}

/// The opponent's land first moves the other way across the screen from its place, down into
/// the open table, and ends on its place.
#[test]
fn the_opponents_land_swings_out_before_it_lands() {
    let (_, before, after, timings) = cases()
        .into_iter()
        .find(|(name, ..)| *name == "opponent land played")
        .expect("the case");
    let show = played(&before, &after, timings);
    let slot = flight(&show).steps[0].subject.expect("a card flies");
    let (start, end) = span(&show);
    let from = pose(&show, slot, start);
    let to = pose(&show, slot, end);
    let [right, up, _] = axes(&show);
    let across = (to.position - from.position).dot(right);
    let early = pose(&show, slot, start + (end - start) / 5);
    let moved = early.position - from.position;
    assert!(
        moved.dot(right) * across < 0.0,
        "it first goes the other way across the screen: from {:?} toward {:?}, at {:?}",
        from.position,
        to.position,
        early.position
    );
    assert!(
        off_line(early, from, to).dot(up) < 0.0,
        "it drops into the open table, down the screen from its straight way: {:?}",
        early.position
    );
    let placement = show.table().placement(&after.view, &[]);
    let rest = placement
        .get(flight_slot(&after))
        .map(|rest| rest.pose)
        .expect("a place");
    assert_eq!(to, rest, "it ends at its placement");
}

fn flight_slot(after: &GameUpdate) -> ObjectId {
    let script = after.view.script.as_ref().expect("a script");
    match &script.beats[0][0].act {
        Act::Fly { object, .. } => object.slot,
        _ => unreachable!("a flight"),
    }
}

/// When a card leaves the hand, the cards that stay are at their new places 170 ms after the
/// step begins, the room the session gives it, though the step lasts longer.
#[test]
fn the_hand_closes_up_in_170_ms_after_a_card_leaves_it() {
    let (_, before, after, timings) = cases()
        .into_iter()
        .find(|(name, ..)| *name == "own spell cast")
        .expect("the case");
    let (_, [mine, _]) = table();
    let show = played(&before, &after, timings);
    let (start, end) = span(&show);
    assert!(end - start > 170 * MS, "the cast outlasts the room");
    let placement = show.table().placement(&after.view, &[]);
    let script = flight(&show);
    let moved: Vec<usize> = [mine.land, mine.other]
        .into_iter()
        .map(|slot| usize::from(slot.0))
        .filter(|&slot| !script.track(slot).is_empty())
        .collect();
    assert!(!moved.is_empty(), "a card of the hand moves to close up");
    for slot in moved {
        let id = ObjectId(u16::try_from(slot).expect("a slot"));
        let rest = placement
            .get(id)
            .map(|rest| rest.pose)
            .expect("in the hand");
        assert_ne!(
            pose(&show, slot, start + 85 * MS),
            rest,
            "card #{slot} on its way"
        );
        assert_eq!(
            pose(&show, slot, start + 170 * MS),
            rest,
            "card #{slot} at rest"
        );
    }
}

/// A resize in the middle of a thrown flight leaves the card where it was at that instant: as it
/// shrinks off the stack, aloft, and on its way down.
#[test]
fn a_resize_in_mid_throw_keeps_the_pose() {
    // The step begins at 330 ms and lasts 520.
    for at in [350, 450, 700] {
        let (_, before, after, timings) = cases()
            .into_iter()
            .find(|(name, ..)| *name == "permanent resolves")
            .expect("the case");
        let mut show = show();
        show.push(Box::new(before), Vec::new());
        show.push(Box::new(after.clone()), timings);
        let mut scratch = empty_frame();
        frame(&mut show, &mut scratch, 0);
        let slot = usize::from(flight_slot(&after).0);
        let at = at * MS;
        frame(&mut show, &mut scratch, at);
        let was = show.pose(slot, at).expect("in flight");
        show.resize(window(2560.0, 1080.0));
        frame(&mut show, &mut scratch, at);
        let now = show.pose(slot, at).expect("in flight");
        assert!(
            was.position.distance(now.position) < 1e-2
                && was.rotation.dot(now.rotation).abs() > 1.0 - 1e-5
                && (was.scale - now.scale).abs() < 1e-4,
            "the card does not jump at {} ms: {was:?} became {now:?}",
            at / MS
        );
    }
}

/// A card played from the middle of a ten-card hand onto the table is seen for its whole
/// flight, as "every step shown" asks. Each card of a hand faces the camera a little nearer the
/// eye than the one before it, so a card that flew along the straight line from its place would
/// start behind the cards to its right and stay mostly covered by them for half its flight.
#[test]
fn a_card_played_from_the_middle_of_a_big_hand_is_seen_as_it_flies() {
    let mut builder = ViewBuilder::new();
    for _ in 0..5 {
        builder.card(ZoneName::Library, Side::You, "Island");
    }
    let mut played = None;
    for index in 0..10 {
        let card = builder.card(ZoneName::Hand, Side::You, "Plains");
        card.types = vec![CardType::Land];
        if index == 4 {
            played = Some(card.slot);
        }
    }
    builder
        .card(ZoneName::Battlefield, Side::You, "Island")
        .types = vec![CardType::Land];
    builder.script(ScriptBuilder::reset().build());
    let before = builder.update();
    let played = played.expect("the hand has a fifth card");
    let mut after = flown(
        &before,
        played,
        ZoneName::Hand,
        ZoneName::Battlefield,
        Why::Play,
    );
    after
        .view
        .script
        .as_mut()
        .expect("the flight has a script")
        .timing = vec![timed("land played", 315.0, 0.0, Curve::EaseIn, true)];
    let size = Size {
        width: 2560.0,
        height: 1080.0,
    };
    let unseen = unseen_steps(&before, &after, Side::You, size);
    assert!(unseen.is_empty(), "{unseen:?}");
}

/// The "every step shown" faults of a recording of `before` and then `after`, played from
/// `seat` in a window of `size` and checked at every tick. A flight's fault says its card was
/// seen in fewer than `audit.seen` of the frames it flies in.
fn unseen_steps(before: &GameUpdate, after: &GameUpdate, seat: Side, size: Size) -> Vec<String> {
    let mut messages = vec![
        FixtureMessage::game_update(before),
        FixtureMessage::game_update(after),
    ];
    messages.insert(0, fixture_message(&messages, TokenMakers::NONE));
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        seat,
        size,
    )
    .expect("the views can be read");
    page.check_all(Mode::EveryTick)
        .faults
        .into_iter()
        .filter(|fault| fault.rule == Rule::StepShown)
        .map(|fault| fault.message)
        .collect()
}

/// The opponent's discard is seen for at least `audit.seen` of its flight in every desktop
/// window. Their hand is held mostly above the window and their graveyard lies at its top right,
/// so a flight along its own line in the table's space, between ends at very different depths,
/// curves above the top edge on the window, where the card turns edge-on as it turns face up.
#[test]
fn the_opponents_discard_is_seen_as_it_flies_to_their_graveyard() {
    let mut builder = ViewBuilder::new();
    for side in [Side::You, Side::Opponent] {
        for _ in 0..5 {
            builder.card(ZoneName::Library, side, "Island");
        }
    }
    // A full hand, face down to the viewer, so that the second card from the left flies far
    // across the top of the window and turns over on its way.
    let mut discarded = None;
    for index in 0..7 {
        let card = builder.card(ZoneName::Hand, Side::Opponent, "Island");
        card.name = None;
        if index == 1 {
            discarded = Some(card.slot);
        }
    }
    builder.script(ScriptBuilder::reset().build());
    let before = builder.update();
    let discarded = discarded.expect("the hand has a second card");
    let mut after = flown(
        &before,
        discarded,
        ZoneName::Hand,
        ZoneName::Graveyard,
        Why::Discard,
    );
    after
        .view
        .script
        .as_mut()
        .expect("the flight has a script")
        .timing = vec![timed("discard", 165.0, 0.0, Curve::EaseOut, true)];
    // The card comes back from the graveyard into the hand, as dredge brings one back.
    let mut returned = flown(
        &after,
        discarded,
        ZoneName::Graveyard,
        ZoneName::Hand,
        Why::Return,
    );
    returned
        .view
        .players
        .opponent
        .graveyard
        .retain(|slot| *slot != discarded);
    let script = returned
        .view
        .script
        .as_mut()
        .expect("the return has a script");
    script.timing = vec![timed("return", 400.0, 0.0, Curve::EaseOut, true)];
    let mut discarded_before = after.clone();
    discarded_before.view.script = Some(ScriptBuilder::reset().build());
    for (width, height) in [
        (1440.0, 900.0),
        (1600.0, 900.0),
        (1920.0, 1080.0),
        (2560.0, 1080.0),
    ] {
        let size = Size { width, height };
        let unseen = unseen_steps(&before, &after, Side::You, size);
        assert!(unseen.is_empty(), "discard at {width}x{height}: {unseen:?}");
        let unseen = unseen_steps(&discarded_before, &returned, Side::You, size);
        assert!(unseen.is_empty(), "return at {width}x{height}: {unseen:?}");
    }
}
