//! A script compiled a few steps at a time over several frames poses every object as the same
//! script compiled whole does, at every frame (docs/beryl-3d.md, "Threads" and "Determinism").

use app_client::Input;
use app_client::{Client, Config};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{PlayerView, SessionId};
use beryl_protocol::protocol::{Side, Why, ZoneName};
use beryl_protocol::script::{Act, Step, StepTiming};
use beryl_protocol::testing::{ViewBuilder, object, script, step, timing};
use game_play::play::parts::{STEPS_PER_ADVANCE, compile_in_parts};
use game_table::table::{DESKTOP, Table};
use kit_frame::frame::Wake;
use kit_ui::input::{Insets, Size};

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
    let table = Table::new(crate::test_data::data(), DESKTOP, window(1440.0, 900.0));
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

/// The battlefield of `PERMANENTS` permanents, and the view in which a script of `BEATS` beats
/// of `PER_BEAT` staggered flights each has flown the first of them to the end of the row, one
/// after another, so that each flight moves every card after it.
fn row_flights() -> (PlayerView, PlayerView) {
    let mut builder = ViewBuilder::new();
    for _ in 0..PERMANENTS {
        builder.card(ZoneName::Battlefield, Side::You, "Serra Angel");
    }
    let before = builder.build();
    let after = flights_after(&before);
    (before, after)
}

/// The view in which a script of `BEATS` beats of `PER_BEAT` staggered flights each has flown
/// the first permanents of `before`'s row to its end, one after another.
fn flights_after(before: &PlayerView) -> PlayerView {
    let moved = BEATS * PER_BEAT;
    let end = u16::try_from(PERMANENTS - 1).expect("a slot fits");
    let beats: Vec<Vec<Step>> = before.battlefield[..moved]
        .chunks(PER_BEAT)
        .map(|chunk| {
            chunk
                .iter()
                .map(|&slot| {
                    step(Act::Fly {
                        from: ZoneName::Battlefield,
                        why: Why::Effect,
                        object: before.objects[usize::from(slot.0)].clone(),
                        at: Some(end),
                        item: None,
                        origin: None,
                    })
                })
                .collect()
        })
        .collect();
    let mut after = before.clone();
    after.script = None;
    after.battlefield.rotate_left(moved);
    after.script = Some(script(None, beats, vec![staggered(); moved]));
    after
}

#[test]
fn flights_along_a_row_compiled_in_parts_pose_every_card_as_compiled_whole() {
    let (before, after) = row_flights();
    // Each flight lands after the next two have begun, which cannot change where it lands, so a
    // beat is compiled a few steps at a time.
    compiles_in_parts((&before, &after), (BEATS, PER_BEAT), 2 * BEATS);
}

/// Each frame's draw records with the steps the frame compiled.
type Frames = Vec<(Vec<kit_frame::frame::Draw>, u32)>;

/// Plays `ticks` through a client as a host does, a frame every 60th of a second with `prepare`
/// before each, handing over each tick's updates together once the tick before has played out,
/// and returns the frames from when the last tick's updates arrive.
fn played(ticks: &[&[GameUpdate]], whole: bool) -> Frames {
    let mut client = Client::new(crate::test_data::data(), Config::LARGEST);
    if whole {
        client.compile_whole();
    }
    client.handle(Input::Surface {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale: 1.0,
        safe_area: Insets::default(),
    });
    let mut frames = Vec::new();
    let mut now = 0;
    for (index, updates) in ticks.iter().enumerate() {
        for update in *updates {
            client.handle(Input::Update(Box::new(update.clone())));
        }
        loop {
            client.prepare(now);
            let frame = client.frame(now);
            if index + 1 == ticks.len() {
                frames.push((frame.draws.clone(), frame.work.steps));
            }
            now += 50 * MS / 3;
            if client.wake() == Wake::Never {
                break;
            }
        }
    }
    frames
}

#[test]
fn a_client_compiles_a_long_script_over_frames_and_draws_each_as_compiled_whole() {
    let (before, after) = row_flights();
    let update = |view: PlayerView| GameUpdate {
        session: SessionId(1),
        view,
    };
    let ticks: [&[GameUpdate]; 2] = [&[update(before)], &[update(after)]];
    let parts = played(&ticks, false);
    let whole = played(&ticks, true);
    assert_eq!(
        parts.len(),
        whole.len(),
        "the script plays as long either way"
    );
    for (frame, ((part, _), (whole, _))) in parts.iter().zip(&whole).enumerate() {
        assert!(
            part == whole,
            "frame {frame} after the script arrives draws as the script compiled whole does"
        );
    }
    let moved = BEATS * PER_BEAT;
    assert!(moved >= 30, "the script is long: {moved} steps");
    assert_eq!(whole[0].1, u32::try_from(moved).expect("fits"));
    assert!(
        parts[0].1 <= u32::try_from(PER_BEAT + STEPS_PER_ADVANCE).expect("fits"),
        "the frame that receives the script compiles its first beat and a few steps more, not \
         all {moved}: {:?}",
        parts.iter().map(|(_, steps)| steps).collect::<Vec<_>>()
    );
    assert_eq!(
        parts
            .iter()
            .map(|(_, steps)| *steps as usize)
            .sum::<usize>(),
        moved,
        "every step is compiled once"
    );
    assert!(
        parts.iter().filter(|(_, steps)| *steps > 0).count() >= BEATS,
        "the steps are compiled over {BEATS} frames or more"
    );
}

#[test]
fn a_script_handed_over_behind_another_compiles_as_it_starts_and_draws_as_compiled_whole() {
    let (before, first) = row_flights();
    let second = flights_after(&first);
    let update = |view: PlayerView| GameUpdate {
        session: SessionId(1),
        view,
    };
    let both = [update(first), update(second)];
    let ticks: [&[GameUpdate]; 2] = [&[update(before)], &both];
    let parts = played(&ticks, false);
    let whole = played(&ticks, true);
    assert_eq!(
        parts.len(),
        whole.len(),
        "the scripts play as long either way"
    );
    for (frame, ((part, _), (whole, _))) in parts.iter().zip(&whole).enumerate() {
        assert!(
            part == whole,
            "frame {frame} after both scripts arrive draws as the scripts compiled whole do"
        );
    }
    let moved = 2 * BEATS * PER_BEAT;
    assert_eq!(whole[0].1, u32::try_from(moved).expect("fits"));
    let steps: Vec<u32> = parts.iter().map(|(_, steps)| *steps).collect();
    assert!(
        steps
            .iter()
            .all(|&steps| steps as usize <= PER_BEAT + STEPS_PER_ADVANCE),
        "no frame compiles more than a beat and a few steps, not the second script whole as it \
         arrives: {steps:?}"
    );
    assert_eq!(
        steps.iter().map(|&steps| steps as usize).sum::<usize>(),
        moved,
        "every step is compiled once"
    );
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
            beat.push(step(Act::Fly {
                from: ZoneName::Library,
                why: Why::Draw,
                object: card,
                at: None,
                item: None,
                origin: None,
            }));
        }
        beats.push(beat);
    }
    after.script = Some(script(None, beats, vec![staggered(); BEATS * DRAWS]));
    // Each draw changes where the draws before it in its beat land, so a beat is compiled whole
    // before it begins.
    compiles_in_parts((&before, &after), (BEATS, DRAWS), BEATS);
}
