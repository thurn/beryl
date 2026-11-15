//! The frames `rank` picks for the instruction-count gate, and a playback's shape.

use beryl_inspect::gate::{Phase, REST_FRAMES, play, rank};
use beryl_inspect::stress::NAMES;
use beryl_scene::frame::{Frame, Role};

#[test]
fn rank_picks_the_frames_with_the_most_work() {
    // Every frame of a flight writes each card and evaluates each card's segment, and the rest
    // frames before it draw only the cards the rows show, so the first frames of the flight
    // rank highest, the earliest of equals first.
    let first: [usize; 8] = std::array::from_fn(|i| REST_FRAMES + i);
    assert_eq!(rank("stress-255"), Some(first));
    assert_eq!(rank("stress-1024"), Some(first));
    assert_eq!(rank("no-such-board"), None);
}

#[test]
fn rank_sees_the_frame_that_places_the_most_objects() {
    // A view whose script moves cards between zones is compiled step by step, and each step
    // places the rows it changed again. Only `Work::placed` counts that work, and it costs more
    // instructions than any other frame of `verbs-zones`, so the frame doing the most of it
    // must rank. A frame at rest places nothing.
    let mut most = (0, 0);
    let mut at_rest = 0;
    assert!(play("verbs-zones", |moment, frame| {
        if frame.work.placed > most.0 {
            most = (frame.work.placed, moment.frame);
        }
        if moment.phase == Phase::Rest {
            at_rest += frame.work.placed;
        }
    }));
    assert!(most.0 > 0, "no frame of verbs-zones placed an object");
    let ranked = rank("verbs-zones").expect("verbs-zones is a gated board");
    assert!(
        ranked.contains(&most.1),
        "frame {} places {} objects, but rank picks {ranked:?}",
        most.1,
        most.0
    );
    assert_eq!(at_rest, 0, "frames at rest placed objects");
}

#[test]
fn a_stress_board_rests_then_flies_until_its_script_ends() {
    for name in NAMES {
        let mut phases = Vec::new();
        assert!(play(name, |moment, frame| {
            phases.push(moment.phase);
            if moment.phase == Phase::Rest {
                assert_eq!(frame.work.segments, 0, "{name} frame {}", moment.frame);
            }
        }));
        // The first view is a reset, which the first frame takes and shows at once.
        assert_eq!(phases[0], Phase::Flight, "{name}");
        assert_eq!(
            phases[1..REST_FRAMES],
            [Phase::Rest; REST_FRAMES - 1],
            "{name}"
        );
        // The 40 flights take 39 staggers of 30 ms and one flight of 420 ms: 1,590 ms, or 96
        // frames at 60 a second, and the frame that shows the board at rest after them.
        assert_eq!(phases.len() - REST_FRAMES, 97, "{name}");
    }
}

#[test]
fn a_recorded_board_plays_every_view() {
    // `pay-default` opens prompts, so some of its frames lay out text.
    let mut texts = 0;
    let mut frames = 0;
    assert!(play("pay-default", |_, frame| {
        frames += 1;
        texts += frame.work.text_runs;
    }));
    // Its views each wait 30 frames for the one before, and there are more than two.
    assert!(frames > 2 * REST_FRAMES, "{frames} frames");
    assert!(texts > 0, "no frame of pay-default laid out text");
}

#[test]
fn a_flight_draws_only_the_cards_the_rows_show() {
    // A battlefield row too long for the window shows a window of its cards, and the rest rest
    // undrawn at its end, above the last card shown. While the 40 permanents fly, the cards
    // that stay slide along their rows; one out of view before and after the flight must stay
    // undrawn, or it covers the row's last card and the frame draws the whole battlefield.
    for name in NAMES {
        let mut at_rest = 0;
        let mut most_in_flight = 0;
        // The cards drawn, not the records: each permanent also writes the records of its
        // badges.
        let cards = |frame: &Frame| {
            (frame.resolved.iter())
                .filter(|element| element.role == Role::Card)
                .count()
        };
        assert!(play(name, |moment, frame| match moment.phase {
            Phase::Rest => at_rest = at_rest.max(cards(frame)),
            Phase::Flight if moment.frame >= REST_FRAMES => {
                most_in_flight = most_in_flight.max(cards(frame));
            }
            Phase::Flight => {}
        }));
        // The flying cards, which were all shown before they took off, are the only ones a
        // flight may add.
        assert!(
            most_in_flight <= at_rest + 40,
            "{name}: {most_in_flight} cards drawn in flight, {at_rest} at rest"
        );
    }
}
