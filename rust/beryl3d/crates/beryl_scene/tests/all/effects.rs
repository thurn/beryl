//! The effects a step begins, on the show's clock (docs/beryl-3d.md, "Effects"): each
//! `StepEffect` of a step's timing becomes an `EffectRun` timed as the web stage times it, and
//! the show holds it until it ends, past the end of its script.

use beryl_protocol::protocol::Side;
use beryl_protocol::script::{Curve, Edge, StepEffect, StepTiming, Subject};
use beryl_scene::effects::{Effect, EffectRun};
use beryl_scene::frame::Wake;
use beryl_scene::play::Show;

use crate::common::{MS, before, discard, empty_frame, frame, show};

/// An effect named `name` that begins at `when` of its step, with nothing else set.
fn effect(name: &str, when: Edge) -> StepEffect {
    StepEffect {
        name: name.to_owned(),
        when,
        on: None,
        into: None,
        duration: None,
        delay: 0.0,
        curve: Curve::Linear,
    }
}

/// A show whose first script shows the table, and whose second discards a card in one step
/// from 100 to 500 ms that begins `effects`. Both have started by the frame at 0, so that each
/// script time is also a real time. Returns the show and the discarded card.
fn started(effects: Vec<StepEffect>) -> (Show, Subject) {
    let (start, cards) = before();
    let (after, mut timings) = discard(&start, cards);
    timings[0] = StepTiming {
        delay: 100.0,
        effects,
        ..timings[0].clone()
    };
    let mut show = show();
    show.record();
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(after), timings);
    let mut buffer = empty_frame();
    frame(&mut show, &mut buffer, 0);
    (
        show,
        Subject::Object {
            slot: cards.discard,
        },
    )
}

/// Each run the show holds, as its effect and when it starts and ends in milliseconds.
fn runs(show: &Show) -> Vec<(Effect, f64, f64)> {
    #[expect(clippy::cast_precision_loss, reason = "a test's times are small")]
    let ms = |t: u64| t as f64 / 1e6;
    show.effects()
        .iter()
        .map(|run| (run.effect, ms(run.start), ms(run.start + run.duration)))
        .collect()
}

#[test]
fn an_effect_at_the_end_of_a_step_begins_as_the_step_ends() {
    let (show, card) = started(vec![StepEffect {
        duration: Some(330.0),
        ..effect("settle", Edge::End)
    }]);
    assert_eq!(runs(&show), vec![(Effect::Settle, 500.0, 830.0)]);
    let run = show.effects()[0];
    assert_eq!(run.on, card, "on the step's subject");
    assert_eq!(run.step, 0);
    let record: Vec<EffectRun> = show.history()[1].effects.clone();
    assert_eq!(
        record,
        show.effects(),
        "the record holds it on the script's clock"
    );
}

#[test]
fn an_effect_into_a_step_with_no_duration_lasts_as_long_as_the_step() {
    // As on the web stage, it begins 0.376 of the step's 400 ms in, and lasts 400 ms.
    let (show, _) = started(vec![StepEffect {
        into: Some(0.376),
        ..effect("dissolve", Edge::Start)
    }]);
    assert_eq!(runs(&show), vec![(Effect::Dissolve, 250.4, 650.4)]);
}

#[test]
fn an_effect_on_something_else_is_on_that_and_one_on_nothing_named_is_dropped() {
    let player = Subject::Player {
        player: Side::Opponent,
    };
    let (show, _) = started(vec![
        StepEffect {
            on: Some(player),
            ..effect("loss", Edge::Start)
        },
        StepEffect {
            duration: Some(0.0),
            ..effect("glow", Edge::Start)
        },
    ]);
    assert_eq!(runs(&show), vec![(Effect::Loss, 100.0, 500.0)]);
    assert_eq!(show.effects()[0].on, player);
}

#[test]
fn a_run_is_listed_at_every_frame_of_its_span_and_at_none_after() {
    let (mut show, _) = started(vec![StepEffect {
        duration: Some(330.0),
        ..effect("settle", Edge::End)
    }]);
    let mut buffer = empty_frame();
    for t in (500..900).step_by(10) {
        frame(&mut show, &mut buffer, t * MS);
        assert_eq!(
            show.effects().len(),
            usize::from(t < 830),
            "listed at {t} ms only while it plays"
        );
    }
}

#[test]
fn an_effect_outlives_its_script_and_never_holds_up_the_show() {
    let (mut show, _) = started(vec![StepEffect {
        duration: Some(1000.0),
        ..effect("glow", Edge::End)
    }]);
    let mut buffer = empty_frame();
    frame(&mut show, &mut buffer, 700 * MS);
    assert!(
        show.settled(),
        "the script ended at 500 ms, so a prompt may open"
    );
    assert_eq!(runs(&show), vec![(Effect::Glow, 500.0, 1500.0)]);
    assert_eq!(show.wake(700 * MS), Wake::Now, "it is under way");
    frame(&mut show, &mut buffer, 1500 * MS);
    assert!(show.effects().is_empty());
    assert_eq!(show.wake(1500 * MS), Wake::Never);
}

/// The recorded reply to a view is due a fixed time after the table rests, so the table must
/// rest at the same time however its frames fall. It used to rest when the first frame after the
/// effect found it over, so a check on every tick played the replies later than a check at the
/// check times did, and the two disagreed about what the steps after showed.
#[test]
fn the_table_rests_as_an_effect_that_outlives_its_script_ends_however_the_frames_fall() {
    let glow = || {
        started(vec![StepEffect {
            duration: Some(1000.0),
            ..effect("glow", Edge::End)
        }])
        .0
    };
    let mut buffer = empty_frame();
    for frames in [&[700, 1500][..], &[700, 1517], &[1999]] {
        let mut show = glow();
        for &t in frames {
            frame(&mut show, &mut buffer, t * MS);
        }
        assert_eq!(show.wake(frames[frames.len() - 1] * MS), Wake::Never);
        assert_eq!(show.rested(), 1500 * MS, "drawn at {frames:?} ms");
    }
}

#[test]
fn the_show_wakes_for_an_effect_that_begins_after_its_script_ends() {
    let (mut show, _) = started(vec![StepEffect {
        delay: 100.0,
        duration: Some(200.0),
        ..effect("pulse", Edge::End)
    }]);
    let mut buffer = empty_frame();
    frame(&mut show, &mut buffer, 550 * MS);
    assert!(show.settled());
    assert_eq!(show.wake(550 * MS), Wake::At(600 * MS));
    frame(&mut show, &mut buffer, 600 * MS);
    assert_eq!(show.wake(600 * MS), Wake::Now);
}

#[test]
fn a_later_run_of_one_effect_on_one_thing_cuts_short_the_one_before() {
    let (show, _) = started(vec![
        StepEffect {
            duration: Some(1000.0),
            ..effect("glow", Edge::Start)
        },
        StepEffect {
            duration: Some(200.0),
            ..effect("glow", Edge::End)
        },
    ]);
    assert_eq!(
        runs(&show),
        vec![(Effect::Glow, 100.0, 500.0), (Effect::Glow, 500.0, 700.0)]
    );
}

#[test]
fn a_reset_clears_every_effect() {
    let (mut show, _) = started(vec![StepEffect {
        duration: Some(5000.0),
        ..effect("glow", Edge::End)
    }]);
    let mut buffer = empty_frame();
    frame(&mut show, &mut buffer, 600 * MS);
    assert_eq!(show.effects().len(), 1);
    let (reset, _) = before();
    show.push(Box::new(reset), Vec::new());
    frame(&mut show, &mut buffer, 700 * MS);
    assert!(show.effects().is_empty());
}
