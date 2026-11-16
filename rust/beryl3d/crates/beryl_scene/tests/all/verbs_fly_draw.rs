//! `fly`'s draws (`data/ui-conventions.json`, `fly`, the cases `draw`, `draw of several` and
//! `opponent draw`): each plays to its end with every card at its placement and no jump, a card
//! the viewer draws alone is held up beside the stack, several drawn together sweep past that
//! place without stopping, and a card turns over at the moments the web interface's `turningOf`
//! (`ui/src/stage/tracks.ts`) gives.

use crate::common::{MS, empty_frame, frame, show};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{ObjectId, Side, StackItem, Why, ZoneName};
use beryl_protocol::script::{Act, StepTiming, Table as Played, time_script};
use beryl_scene::agent::steps::{no_jumps, rest_equals_placement, step_times};
use beryl_scene::data::space::verbs::fly::{held, turn};
use beryl_scene::motion::{Track, pose_at};
use beryl_scene::play::Show;
use beryl_scene::play::record::ScriptRecord;
use beryl_scene::pose::{Pose, Segment};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder};
use glam::Vec2;

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

/// A card drawn turns face up as it leaves the library: it shows its back until `turn.draw.at`
/// of its flight and its face from then on.
#[test]
fn a_drawn_card_is_face_up_from_the_draw_turn() {
    let (before, cards) = table();
    let slot = cards.library[0];
    let show = played(&before, after(&before, &[&[(slot, ZoneName::Hand)]]));
    let segments = segments(&show, slot);
    let step = span(&show, slot);
    let face_down = |t: u64| pose_at(&segments, t).turn > 0.5;
    assert!(face_down(at(step, turn::draw::AT - 0.01)));
    assert!(!face_down(at(step, turn::draw::AT + 0.005)));
    assert!(!face_down(step.1));
}

/// Where a pose's centre is on the window, in logical points.
fn on_screen(show: &Show, pose: Pose) -> Vec2 {
    show.table().camera().to_screen(pose.position)
}

/// A card drawn alone is held up at the held place for the whole of the hold: it is within a
/// logical point of the straight way from there toward its place in the hand, and no farther
/// along it than it sinks while held. It sinks along a straight way in the table's space, and
/// the hand is a little farther from the eye, so on the window it sinks a little more: a
/// hundredth of the way is allowed for that.
#[test]
fn a_card_drawn_alone_is_held_up_beside_the_stack() {
    let (before, cards) = table();
    let slot = cards.library[0];
    let show = played(&before, after(&before, &[&[(slot, ZoneName::Hand)]]));
    let segments = segments(&show, slot);
    let step = span(&show, slot);
    let camera = show.table().camera();
    let size = camera.window.size;
    let place = Vec2::new(
        f32::midpoint(held::NDC.x, 1.0) * size.width,
        f32::midpoint(1.0, -held::NDC.y) * size.height,
    );
    let rest = on_screen(&show, pose_at(&segments, step.1));
    let way = rest - place;
    let sink = beryl_scene::data::space::verbs::fly::HELD_SINK;
    for i in 0..=10 {
        #[expect(clippy::cast_precision_loss, reason = "a small count")]
        let share = held::IN + held::HOLD * i as f32 / 10.0;
        let seen = on_screen(&show, pose_at(&segments, at(step, share)));
        let off = seen - place;
        let along = off.dot(way) / way.length_squared();
        let aside = (off - way * along).length();
        assert!(aside < 1.0, "{share}: {aside} points off the way down");
        assert!(
            (-0.001..=sink + 0.01).contains(&along),
            "{share}: {along} of the way down"
        );
    }
}

/// Three cards drawn together never stop between the library and the hand: each moves from
/// when its step begins until it ends.
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
    let turned = pose_at(&segments, at(step, turn::cast::AT)).turn;
    assert!((turned - 0.5).abs() < 0.02, "turned {turned}");
    let before_turn = turn::cast::AT - turn::cast::EACH - 0.01;
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
    assert!(segments.len() >= 4, "{segments:?}");
    let mut track = Track::default();
    for segment in segments {
        assert!(
            track.push(segment, segment.start).is_ok(),
            "the track holds every segment"
        );
    }
}

/// When the window changes shape during a flight that turns over or sweeps past a place, the
/// segment under way takes its new end pose and keeps the card where it was at that instant,
/// face and size included.
#[test]
fn a_new_end_mid_turn_or_sweep_keeps_the_pose_at_that_instant() {
    use beryl_scene::pose::{Curve, Shape, Turning};
    use beryl_scene::verbs::Interval;
    use glam::Vec3;
    let from = Pose {
        turn: 1.0,
        ..Pose::default()
    };
    let to = Pose {
        position: Vec3::new(300.0, -100.0, 40.0),
        scale: 1.5,
        ..Pose::default()
    };
    let flight = Segment {
        shape: Shape::Swept {
            bulge: Vec3::new(40.0, 120.0, 60.0),
            grow: 0.4,
        },
        turning: Some(Turning {
            at: turn::draw::AT,
            each: turn::draw::EACH,
        }),
        ..Segment::between(
            from,
            to,
            Interval {
                from: 0,
                to: 700 * MS,
            },
            Curve::EASE_OUT,
        )
    };
    for t in [60 * MS, 112 * MS, 150 * MS, 400 * MS] {
        let now = flight.pose_at(t);
        let mut moved = Segment {
            to: Pose {
                position: Vec3::new(260.0, -130.0, 30.0),
                scale: 1.3,
                ..to
            },
            ..flight
        };
        moved.through(t, now);
        let kept = moved.pose_at(t);
        assert!(
            kept.position.distance(now.position) < 1e-2,
            "{t}: {kept:?} {now:?}"
        );
        assert!((kept.scale - now.scale).abs() < 1e-4, "{t}");
        assert!((kept.turn - now.turn).abs() < 1e-4, "{t}");
    }
}
