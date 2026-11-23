//! `fly`'s draws (`data/ui-conventions.json`, `fly`, the cases `draw`, `draw of several` and
//! `opponent draw`): each plays to its end with every card at its placement and no jump, a card
//! the viewer draws alone is held up beside the stack, several drawn together sweep past that
//! place without stopping, and a card turns over at the moments the web interface's `turningOf`
//! (`ui/src/stage/tracks.ts`) gives.

use crate::common::{MS, empty_frame, frame, show};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{ObjectId, Side, StackItem, Why, ZoneName};
use beryl_protocol::script::{Act, StepTiming, Table as Played, time_script};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use game_play::play::Show;
use game_play::play::record::ScriptRecord;
use game_probe::agent::steps::{no_jumps, rest_equals_placement, step_times};
use glam::Vec2;
use kit_base::pose::{Pose, Segment, Shape};
use kit_space::motion::{Track, pose_at};

/// A table with a library and a hand of two for each side, and a spell in each hand.
struct Cards {
    /// The viewer's library, top first.
    library: Vec<ObjectId>,
    /// The opponent's library, top first.
    their_library: Vec<ObjectId>,
    /// The opponent's spell in their hand.
    their_spell: ObjectId,
}

fn table() -> (GameUpdate, Cards) {
    let mut builder = ViewBuilder::new();
    let mut libraries = [Vec::new(), Vec::new()];
    let mut spells = Vec::new();
    for (i, side) in [Side::You, Side::Opponent].into_iter().enumerate() {
        for _ in 0..5 {
            libraries[i].push(builder.card(ZoneName::Library, side, "Island").slot);
        }
        spells.push(builder.card(ZoneName::Hand, side, "Counterspell").slot);
        builder.card(ZoneName::Hand, side, "Memory Lapse");
    }
    builder.script(ScriptBuilder::reset().build());
    let [library, their_library] = libraries;
    (
        builder.update(),
        Cards {
            library,
            their_library,
            their_spell: spells[1],
        },
    )
}

/// The act that moves the card in `slot` of `view` to `zone`, with the card as it then is: a
/// card the viewer draws is named; the opponent's stays unknown until it reaches the stack.
fn moved(view: &mut beryl_protocol::protocol::PlayerView, slot: ObjectId, zone: ZoneName) -> Act {
    let object = view
        .objects
        .iter_mut()
        .find(|o| o.slot == slot)
        .expect("the card is on the table");
    let from = object.zone;
    object.zone = zone;
    if object.controller == Side::You || zone == ZoneName::Stack {
        object.name.get_or_insert_with(|| "Plains".to_owned());
    }
    let object = object.clone();
    let player = match object.controller {
        Side::You => &mut view.players.you,
        Side::Opponent => &mut view.players.opponent,
    };
    player.library.retain(|&s| s != slot);
    let item = (zone == ZoneName::Stack).then(|| StackItem::Spell {
        slot,
        targets: Vec::new(),
        x: 0,
        paid: Vec::new(),
    });
    if let Some(item) = &item {
        view.stack.push(item.clone());
    }
    Act::Fly {
        from,
        why: match (from, zone) {
            (ZoneName::Library, _) => Why::Draw,
            _ => Why::Cast,
        },
        object,
        at: None,
        item,
        origin: None,
    }
}

/// The view after `beats` of moves, each a list of cards and where they go, with the timing the
/// session gives them (`time_script`).
fn after(before: &GameUpdate, beats: &[&[(ObjectId, ZoneName)]]) -> (GameUpdate, Vec<StepTiming>) {
    let mut view = before.view.clone();
    let mut script = ScriptBuilder::default();
    for beat in beats {
        let acts: Vec<Act> = beat
            .iter()
            .map(|&(slot, zone)| moved(&mut view, slot, zone))
            .collect();
        script.beat(acts);
    }
    let script = script.build();
    let timings = time_script(&script, &Played::of(&before.view));
    view.script = Some(script);
    (
        GameUpdate {
            session: before.session,
            view,
        },
        timings,
    )
}

/// A recording show that has taken `before` and then `after`, played frame by frame at 60 Hz
/// until both have ended.
fn played(before: &GameUpdate, (after, timings): (GameUpdate, Vec<StepTiming>)) -> Show {
    let mut show = show();
    show.record();
    show.push(Box::new(before.clone()), Vec::new());
    show.push(Box::new(after), timings);
    let mut scratch = empty_frame();
    let mut now = 0;
    frame(&mut show, &mut scratch, now);
    while !show.settled() && now < 10_000 * MS {
        now += 16 * MS;
        frame(&mut show, &mut scratch, now);
    }
    assert!(show.settled(), "the scripts end");
    show
}

fn flight(show: &Show) -> &ScriptRecord {
    show.history().last().expect("the flight was recorded")
}

/// The segments of the card in `slot` in the flight's script.
fn segments(show: &Show, slot: ObjectId) -> Vec<Segment> {
    flight(show)
        .track(usize::from(slot.0))
        .iter()
        .map(|record| record.segment)
        .collect()
}

/// When the step that moves `slot` begins and ends.
fn span(show: &Show, slot: ObjectId) -> (u64, u64) {
    let step = flight(show)
        .steps
        .iter()
        .find(|step| step.subject == Some(usize::from(slot.0)))
        .expect("a step moves the card");
    (step.from, step.to)
}

/// The time `share` of the way from `from` to `to`.
fn at((from, to): (u64, u64), share: f32) -> u64 {
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a step lasts seconds"
    )]
    let t = from + ((to - from) as f64 * f64::from(share)).round() as u64;
    t
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

/// A case's name, the view before it, and the view after it with the session's timing.
type Case = (&'static str, GameUpdate, (GameUpdate, Vec<StepTiming>));

/// The cases of a draw, each with the session's timing.
fn cases() -> Vec<Case> {
    let (before, cards) = table();
    let alone = after(&before, &[&[(cards.library[0], ZoneName::Hand)]]);
    let several = after(
        &before,
        &[&[
            (cards.library[0], ZoneName::Hand),
            (cards.library[1], ZoneName::Hand),
            (cards.library[2], ZoneName::Hand),
        ]],
    );
    let theirs = after(&before, &[&[(cards.their_library[0], ZoneName::Hand)]]);
    vec![
        ("draw", before.clone(), alone),
        ("draw of several", before.clone(), several),
        ("opponent draw", before, theirs),
    ]
}

#[test]
fn every_draw_plays_to_its_placement_with_no_jump_for_its_time() {
    for (case, before, (after, timings)) in cases() {
        assert!(
            timings.iter().all(|t| t.case.as_deref() == Some(case)),
            "{case}: the session times it as {:?}",
            timings.iter().map(|t| t.case.clone()).collect::<Vec<_>>()
        );
        let show = played(&before, (after, timings));
        clean(&show, case);
    }
}

/// A card drawn into the viewer's hand flies along the six-node draw spline: face down as it
/// lifts off the library, flipping across edge-on near the middle of the table, leaning through
/// its 3D flourish, and staying face up through the hold and drop into the hand.
#[test]
fn a_card_drawn_alone_flips_mid_flight_and_grows_to_hold_size() {
    let (before, cards) = table();
    let slot = cards.library[0];
    let show = played(&before, after(&before, &[&[(slot, ZoneName::Hand)]]));
    let segments = segments(&show, slot);
    let step = span(&show, slot);
    assert!(
        segments
            .iter()
            .any(|segment| matches!(segment.shape, Shape::Spline(_))),
        "the draw uses a spline segment"
    );
    // Face down during lift-off, face up after crossing edge-on around t = 0.26, and face up at
    // arrival in the hand.
    assert!(
        pose_at(&segments, at(step, 0.12)).turn > 0.5,
        "face down during lift-off"
    );
    assert!(
        pose_at(&segments, at(step, 0.36)).turn < 0.5,
        "face up after crossing edge-on"
    );
    assert!(pose_at(&segments, step.1).turn < 1e-4, "face up at rest");

    // On screen, the card stays near library width while lifting off and grows to ~1.3x hand
    // height at the hold plateau.
    let camera = show.table().camera();
    let card_height = |share: f32| {
        let corners = camera.card_box(&pose_at(&segments, at(step, share))).0;
        corners[0].distance(corners[3])
    };
    let (library_h, hold_h, hand_h) = (card_height(0.0), card_height(0.64), card_height(1.0));
    assert!(
        hold_h > hand_h * 1.2 && hold_h < hand_h * 1.4,
        "hold height {hold_h} should be ~1.3x hand height {hand_h}"
    );
    assert!(
        hold_h > library_h * 1.5,
        "card grows substantially from library ({library_h}) to hold ({hold_h})"
    );
}

/// Where a pose's centre is on the window, in logical points.
fn on_screen(show: &Show, pose: Pose) -> Vec2 {
    show.table().camera().to_screen(pose.position)
}

/// A card drawn into the viewer's hand holds stationary at node 4 during the Hermite curve's
/// flat hold plateau (`ease[1].time..=ease[2].time`), positioned at the authored hold spot beside
/// the stack.
#[test]
fn a_card_drawn_alone_is_held_up_beside_the_stack() {
    let local = &crate::test_data::data().verbs.fly.draw.local;
    let (before, cards) = table();
    let slot = cards.library[0];
    let show = played(&before, after(&before, &[&[(slot, ZoneName::Hand)]]));
    let segments = segments(&show, slot);
    let step = span(&show, slot);
    let camera = show.table().camera();
    let size = camera.window.size;
    let hold_ndc = local.nodes[3].ndc;
    let expected = Vec2::new(
        f32::midpoint(hold_ndc.x, 1.0) * size.width,
        f32::midpoint(1.0, -hold_ndc.y) * size.height,
    );
    let (hold_start, hold_end) = (local.ease[1].time, local.ease[2].time);
    let anchor = on_screen(&show, pose_at(&segments, at(step, hold_start)));
    assert!(
        anchor.distance(expected) < 2.0,
        "hold spot {anchor:?} should match authored {expected:?}"
    );
    for i in 0..=10 {
        #[expect(clippy::cast_precision_loss, reason = "a small count")]
        let share = hold_start + (hold_end - hold_start) * i as f32 / 10.0;
        let seen = on_screen(&show, pose_at(&segments, at(step, share)));
        assert!(
            seen.distance(anchor) < 0.5,
            "card moved during flat hold plateau at {share}: {seen:?} vs {anchor:?}"
        );
    }
}

/// An opponent's drawn card stays face down, pauses briefly on their library, drops down the
/// right flank, sweeps across upper centre at peak apparent scale, and lands in their hand.
#[test]
fn an_opponents_draw_follows_the_four_node_spline_face_down() {
    let (before, cards) = table();
    let slot = cards.their_library[0];
    let show = played(&before, after(&before, &[&[(slot, ZoneName::Hand)]]));
    let segments = segments(&show, slot);
    let step = span(&show, slot);
    let spline = segments
        .iter()
        .find(|segment| matches!(segment.shape, Shape::Spline(_)))
        .expect("opponent draw uses a spline");
    let start_pos = on_screen(&show, spline.pose_at(step.0));
    let paused_pos = on_screen(&show, spline.pose_at(at(step, 0.04)));
    assert!(
        start_pos.distance(paused_pos) < 0.5,
        "pauses before launch: {start_pos:?} vs {paused_pos:?}"
    );
    let drop_pos = on_screen(&show, spline.pose_at(at(step, 0.30)));
    let apex_pos = on_screen(&show, spline.pose_at(at(step, 0.60)));
    let end_pos = on_screen(&show, spline.pose_at(step.1));
    assert!(
        drop_pos.y > start_pos.y + 40.0,
        "drops down the board from library: {drop_pos:?} vs {start_pos:?}"
    );
    assert!(
        apex_pos.x < drop_pos.x - 40.0 && apex_pos.y > end_pos.y + 40.0,
        "sweeps left across upper centre before entering hand: {apex_pos:?}"
    );
    let camera = show.table().camera();
    for share in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let pose = spline.pose_at(at(step, share));
        assert!(pose.turn > 0.5, "stays face down at {share}");
        // Verify the card back never spins sideways in-plane while travelling into an inverted
        // opponent hand fan.
        let corners = camera.card_box(&pose).0;
        let vertical = (corners[3] - corners[0]).normalize_or_zero();
        assert!(
            vertical.y.abs() > 0.7,
            "card spun sideways at {share}: vertical axis {vertical:?}"
        );
    }
}

/// Three cards drawn together play continuously across their steps with no stalled tracks.
#[test]
fn several_drawn_together_never_rest_on_the_way() {
    let (before, cards) = table();
    let drawn = [cards.library[0], cards.library[1], cards.library[2]];
    let beat: Vec<_> = drawn.iter().map(|&slot| (slot, ZoneName::Hand)).collect();
    let show = played(&before, after(&before, &[&beat]));
    for slot in drawn {
        let mut track = Track::default();
        for segment in segments(&show, slot) {
            track.push(segment, segment.start).expect("room");
        }
        let (from, to) = span(&show, slot);
        let mut t = from;
        while t < to {
            assert!(track.moving(t), "card #{} rests at {t}", slot.0);
            t += 4 * MS;
        }
    }
}

/// The opponent's spell turns over on its way to the stack and is edge-on `turn.cast.at` of the
/// way.
#[test]
fn an_opponents_spell_is_edge_on_at_the_cast_turn() {
    let (before, cards) = table();
    let slot = cards.their_spell;
    let show = played(&before, after(&before, &[&[(slot, ZoneName::Stack)]]));
    let segments = segments(&show, slot);
    let step = span(&show, slot);
    let turn = show.table().data().verbs.fly.turn.cast;
    let turned = pose_at(&segments, at(step, turn.at)).turn;
    assert!((turned - 0.5).abs() < 0.02, "turned {turned}");
    let before_turn = turn.at - turn.each - 0.01;
    assert!((pose_at(&segments, at(step, before_turn)).turn - 1.0).abs() < 1e-4);
}

/// A card drawn and then cast in one script keeps every segment of both flights on its track.
#[test]
fn a_draw_then_a_cast_fits_on_one_track() {
    let (before, cards) = table();
    let slot = cards.library[0];
    let show = played(
        &before,
        after(
            &before,
            &[&[(slot, ZoneName::Hand)], &[(slot, ZoneName::Stack)]],
        ),
    );
    clean(&show, "draw then cast");
    let segments = segments(&show, slot);
    assert!(segments.len() >= 2, "{segments:?}");
    let mut track = Track::default();
    for segment in segments {
        assert!(
            track.push(segment, segment.start).is_ok(),
            "the track holds every segment"
        );
    }
}

/// When the window changes shape during a draw spline, the segment under way takes its new end
/// pose and keeps the card at the exact same position, scale, and 3D transform at that instant.
#[test]
fn a_new_end_mid_draw_spline_keeps_the_pose_at_that_instant() {
    use glam::Vec3;
    let (before, cards) = table();
    for slot in [cards.library[0], cards.their_library[0]] {
        let show = played(&before, after(&before, &[&[(slot, ZoneName::Hand)]]));
        let segments = segments(&show, slot);
        let flight = *segments
            .iter()
            .find(|segment| matches!(segment.shape, Shape::Spline(_)))
            .expect("draw uses a spline");
        let step = (flight.start, flight.end());
        for share in [0.10, 0.25, 0.42, 0.64, 0.82] {
            let t = at(step, share);
            let now = flight.pose_at(t);
            let mut moved = Segment {
                to: Pose {
                    position: flight.to.position + Vec3::new(25.0, -15.0, 10.0),
                    scale: flight.to.scale * 1.15,
                    ..flight.to
                },
                ..flight
            };
            moved.through(t, now);
            let kept = moved.pose_at(t);
            assert!(
                kept.position.distance(now.position) < 1e-2,
                "slot #{} at {share}: {kept:?} vs {now:?}",
                slot.0
            );
            assert!((kept.scale - now.scale).abs() < 1e-4, "at {share}");
            assert!((kept.turn - now.turn).abs() < 1e-4, "at {share}");
            assert!(
                kept.rotation.dot(now.rotation).abs() > 1.0 - 1e-5,
                "at {share}"
            );
        }
    }
}
