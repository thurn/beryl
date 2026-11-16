//! A script compiled a few steps at a time over several frames poses every object as the same
//! script compiled whole does, at every frame (docs/beryl-3d.md, "Threads" and "Determinism").

use beryl_protocol::protocol::PlayerView;
use beryl_protocol::protocol::{Side, Why, ZoneName};
use beryl_protocol::script::{Act, Step, StepTiming};
use beryl_scene::play::parts::{STEPS_PER_ADVANCE, compile_in_parts};
use beryl_scene::table::{DESKTOP, Table};
use beryl_scene::testing::{ViewBuilder, object, script, timing};

use crate::common::{MS, window};

/// How many permanents the battlefield holds, how many beats each script has, and how many
/// permanents each beat of flights moves.
const PERMANENTS: usize = 48;
const BEATS: usize = 4;
const PER_BEAT: usize = 8;
/// How many cards each beat of draws draws: each draw moves every card of the library a little,
/// and a track holds at most `SEGMENTS` segments, so a script with many more draws drops the
/// earliest segments of the library's cards however it is compiled.
const DRAWS: usize = 3;

/// A flight that waits 120 ms after the step before it in its beat, so that its steps begin one
/// after another and each lands well after the next has begun.
fn staggered() -> StepTiming {
    StepTiming {
        stagger: 120.0,
        ..timing(420.0)
    }
}

/// Compiles `after`'s script from `before` whole and a few steps at a time, at a frame every
/// 60th of a second from the one that receives the script until well after it ends, both only
/// as each frame needs and with `STEPS_PER_ADVANCE` steps more, and checks that every pose at
/// every frame is the same and that the steps are compiled over at least `frames` frames.
fn compiles_in_parts(
    (before, after): (&PlayerView, &PlayerView),
    (beats, per_beat): (usize, usize),
    frames: usize,
) {
    let table = Table::new(DESKTOP, window(1440.0, 900.0));
    let times: Vec<u64> = (0..420).map(|frame| frame * 50 * MS / 3).collect();
    let in_time = compile_in_parts(&table, (before, after), (&times, 0));
    let parts = compile_in_parts(&table, (before, after), (&times, STEPS_PER_ADVANCE));
    let moved = beats * per_beat;
    for (in_parts, least) in [(&in_time, 0), (&parts, STEPS_PER_ADVANCE)] {
        for (frame, (part, whole)) in in_parts.parts.iter().zip(&in_parts.whole).enumerate() {
            assert_eq!(
                part, whole,
                "at least {least} steps a frame: every pose at frame {frame} is as compiled whole"
            );
        }
        assert_eq!(in_parts.steps.iter().sum::<usize>(), moved);
    }
    // Compiled only as each frame needs, each beat is compiled as it comes near, and a long
    // beat a few steps at a time.
    assert!(
        in_time.steps.iter().filter(|&&steps| steps > 0).count() >= frames,
        "steps compiled only as needed are compiled over {frames} frames or more: {:?}",
        in_time.steps
    );
    assert!(
        parts.steps[0] <= per_beat + STEPS_PER_ADVANCE,
        "the frame that receives the script compiles its first beat and a few steps more, not \
         all {moved}: {:?}",
        parts.steps
    );
}

#[test]
fn flights_along_a_row_compiled_in_parts_pose_every_card_as_compiled_whole() {
    let mut builder = ViewBuilder::new();
    for _ in 0..PERMANENTS {
        builder.card(ZoneName::Battlefield, Side::You, "Serra Angel");
    }
    let before = builder.build();
    // Each beat flies the first permanents of the row to its end, one after another, so that
    // each flight moves every card after it.
    let moved = BEATS * PER_BEAT;
    let end = u16::try_from(PERMANENTS - 1).expect("a slot fits");
    let beats: Vec<Vec<Step>> = before.objects[..moved]
        .chunks(PER_BEAT)
        .map(|chunk| {
            chunk
                .iter()
                .map(|object| Step {
                    act: Act::Fly {
                        from: ZoneName::Battlefield,
                        why: Why::Effect,
                        object: object.clone(),
                        at: Some(end),
                        item: None,
                        origin: None,
                    },
                    cause: None,
                })
                .collect()
        })
        .collect();
    let mut after = before.clone();
    after.battlefield.rotate_left(moved);
    after.objects.rotate_left(moved);
    after.script = Some(script(None, beats, vec![staggered(); moved]));
    // Each flight lands after the next two have begun, which cannot change where it lands, so a
    // beat is compiled a few steps at a time.
    compiles_in_parts((&before, &after), (BEATS, PER_BEAT), 2 * BEATS);
}

#[test]
fn draws_compiled_in_parts_pose_every_card_as_compiled_whole() {
    let mut builder = ViewBuilder::new();
    for _ in 0..BEATS * DRAWS + 4 {
        builder.card(ZoneName::Library, Side::You, "Island");
    }
    builder.card(ZoneName::Hand, Side::You, "Plains");
    let mut before = builder.build();
    // Draws in the game's first turn are an opening hand, held up together.
    before.turn.number = 3;
    // Each beat draws cards one after another, so that each draw makes room in the hand while
    // the cards drawn before it are still on their way, and changes where they land.
    let mut after = before.clone();
    let mut beats = Vec::new();
    for chunk in before.players.you.library[..BEATS * DRAWS].chunks(DRAWS) {
        let mut beat = Vec::new();
        for &slot in chunk {
            let card = object(slot, ZoneName::Hand, Side::You, Some("Island"));
            after.objects[usize::from(slot.0)] = card.clone();
            after.players.you.library.retain(|&other| other != slot);
            beat.push(Step {
                act: Act::Fly {
                    from: ZoneName::Library,
                    why: Why::Draw,
                    object: card,
                    at: None,
                    item: None,
                    origin: None,
                },
                cause: None,
            });
        }
        beats.push(beat);
    }
    after.script = Some(script(None, beats, vec![staggered(); BEATS * DRAWS]));
    // Each draw changes where the draws before it in its beat land, so a beat is compiled whole
    // before it begins.
    compiles_in_parts((&before, &after), (BEATS, DRAWS), BEATS);
}
