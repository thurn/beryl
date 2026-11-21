//! A script laid out again as it starts (docs/beryl-3d.md, "Timing a script"): a step that ends
//! a span waits until the span has stood whole for its hold, counted from when the span stood
//! whole on the table as the script starts, or from the step of the same script that began it;
//! a step that has the table to itself waits until the announcements passing over it end; and
//! every later step, segment and record moves with the steps it keeps to. A script with no such
//! step is not laid out again.
//!
//! Spans begun in a script before are found in the table's `Lit`, so the test across scripts
//! uses a mark, which its verb writes there; the web interface's `Spans::whole` answers for
//! arrows and marks alike.

use crate::common::{MS, before, empty_frame, frame, show, three_beats};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{ObjectId, Side};
use beryl_protocol::script::{Act, Arrow, Mark, Notice, Span as Edge, StepTiming, Subject};
use game_play::play::Show;
use game_play::play::record::{Cause, ScriptRecord};
use game_probe::testing::{ScriptBuilder, timing};

const fn card(slot: ObjectId) -> Subject {
    Subject::Object { slot }
}

/// `before`'s view with a script of `beats`, and the island in `tapped` tapped, if any.
fn scripted(before: &GameUpdate, beats: &[Vec<Act>], tapped: Option<ObjectId>) -> GameUpdate {
    let mut view = before.view.clone();
    if let Some(slot) = tapped
        && let Some(object) = view.objects.iter_mut().find(|object| object.slot == slot)
    {
        object.tapped = true;
    }
    let mut script = ScriptBuilder::default();
    for beat in beats {
        script.beat(beat.clone());
    }
    view.script = Some(script.build());
    GameUpdate {
        session: before.session,
        view,
    }
}

/// A show that is given each update at its time, in milliseconds, and draws a frame every
/// 5 ms until `until` milliseconds.
fn play(updates: Vec<(u64, GameUpdate, Vec<StepTiming>)>, until: u64) -> Show {
    let mut show = show();
    show.record();
    let mut scratch = empty_frame();
    let mut updates = updates.into_iter().peekable();
    let mut now = 0;
    while now <= until * MS {
        while let Some((_, update, timings)) = updates.next_if(|(at, ..)| at * MS <= now) {
            show.push(Box::new(update), timings);
        }
        frame(&mut show, &mut scratch, now);
        now += 5 * MS;
    }
    show
}

/// The record of the `number`th script `show` was given, started.
fn script(show: &Show, number: usize) -> &ScriptRecord {
    let script = &show.history()[number];
    assert!(script.start.is_some(), "script {number} has started");
    script
}

/// When each step of `script` begins on its clock, in milliseconds.
fn starts(script: &ScriptRecord) -> Vec<u64> {
    script.steps.iter().map(|step| step.from / MS).collect()
}

/// The end of a target arrow or a mark that waits until the span has stood whole for 290 ms,
/// and what follows it waits for it, as the session times one (`time_script`).
fn held(duration: f64) -> StepTiming {
    StepTiming {
        hold: Some(290.0),
        with: true,
        leads: true,
        ..timing(duration)
    }
}

/// A show given a mark on the Angel for 200 ms at 10 ms, and at `ends` milliseconds a script
/// that ends the mark, holding it 290 ms, then taps the Island for 150 ms.
fn mark_then_end(ends: u64) -> (Show, ObjectId) {
    let (start, cards) = before();
    let angel = card(cards.angel);
    let mark = |span| Act::Mark {
        span,
        mark: Mark::Selected,
        on: angel,
    };
    let begun = scripted(&start, &[vec![mark(Edge::Begin)]], None);
    let mut island = begun
        .view
        .objects
        .iter()
        .find(|object| object.slot == cards.island)
        .expect("the island")
        .clone();
    island.tapped = true;
    let ended = scripted(
        &begun,
        &[vec![mark(Edge::End)], vec![Act::Tap { object: island }]],
        Some(cards.island),
    );
    let show = play(
        vec![
            (0, start, Vec::new()),
            (10, begun, vec![timing(200.0)]),
            (ends, ended, vec![held(150.0), timing(150.0)]),
        ],
        ends + 1_000,
    );
    (show, cards.island)
}

/// When the Island's tap begins in the last script of `show`, on its clock, in milliseconds.
fn tap_start(show: &Show, island: ObjectId) -> u64 {
    let script = script(show, 2);
    let tap = script
        .track(usize::from(island.0))
        .into_iter()
        .find(|record| record.cause == Cause::Verb(1))
        .expect("the tap moves the island");
    tap.segment.start / MS
}

#[test]
fn a_span_ended_in_a_later_script_waits_until_it_has_stood_whole_for_its_hold() {
    // The mark stands whole at 210 ms. The script that ends it starts 100 ms later, so its end
    // waits 190 ms more, and the tap after it is 190 ms later than compiled.
    let (late, island) = mark_then_end(310);
    // Started two seconds after the mark stood whole, the end has waited long enough.
    let (settled, _) = mark_then_end(2_210);
    assert_eq!(script(&late, 2).start, Some(310 * MS));
    assert_eq!(starts(script(&settled, 2)), [0, 150], "nothing moves");
    assert_eq!(starts(script(&late, 2)), [190, 340]);
    assert_eq!(script(&settled, 2).end / MS, 300);
    assert_eq!(
        script(&late, 2).end / MS,
        490,
        "the script ends 190 ms later"
    );
    assert_eq!(tap_start(&settled, island), 150);
    assert_eq!(
        tap_start(&late, island),
        340,
        "the tap's segment moves with it"
    );
}

#[test]
fn a_hold_after_a_script_that_caught_up_counts_the_script_time_it_played() {
    // The mark on the Angel stands whole 200 ms into its script, which then marks the Island
    // for 400 ms more. So many updates wait behind it (more than `catch_up.double`) that it
    // plays at twice real time: the 400 ms after the mark stood whole pass in 200 real ms. The
    // web interface measures holds in script time, so when the next script ends the mark the
    // mark has stood 400 ms, past its 290 ms hold, and the end waits for nothing.
    let (start, cards) = before();
    let mark = |span, on| Act::Mark {
        span,
        mark: Mark::Selected,
        on: card(on),
    };
    let begun = scripted(
        &start,
        &[
            vec![mark(Edge::Begin, cards.angel)],
            vec![mark(Edge::Begin, cards.island)],
        ],
        None,
    );
    let ended = scripted(&begun, &[vec![mark(Edge::End, cards.angel)]], None);
    let mut updates = vec![
        (0, start, Vec::new()),
        (10, begun.clone(), vec![timing(200.0), timing(400.0)]),
    ];
    // Scripts with no steps, which take no time, so that more than `catch_up.double` updates
    // wait as the marks' script starts.
    let filler = scripted(&begun, &[], None);
    updates.extend((0..10).map(|_| (10, filler.clone(), Vec::new())));
    updates.push((10, ended, vec![held(150.0)]));
    let show = play(updates, 1_000);
    let last = show.history().len() - 1;
    assert_eq!(script(&show, 1).rate, 2, "the marks' script catches up");
    assert_eq!(script(&show, last).rate, 1);
    assert_eq!(
        script(&show, last).start,
        Some(310 * MS),
        "the marks' 600 ms of script time took 300 real ms"
    );
    assert_eq!(
        starts(script(&show, last)),
        [0],
        "the end waits for nothing"
    );
}

#[test]
fn an_arrow_begun_and_ended_in_one_script_waits_from_its_own_begin() {
    let (start, cards) = before();
    let point = |span| Act::Point {
        span,
        arrow: Arrow::FriendlyTarget,
        from: card(cards.discard),
        to: card(cards.angel),
    };
    let both = scripted(
        &start,
        &[vec![point(Edge::Begin)], vec![point(Edge::End)]],
        None,
    );
    let show = play(
        vec![
            (0, start, Vec::new()),
            (10, both, vec![timing(200.0), held(150.0)]),
        ],
        1_000,
    );
    // The arrow stands whole as its begin ends, at 200 ms, and its end waits 290 ms from then.
    assert_eq!(starts(script(&show, 1)), [0, 490]);
    assert_eq!(show.relays(), 1);
}

/// A turn banner for 1,000 ms that passes over the table.
fn banner() -> (Act, StepTiming) {
    let act = Act::Announce {
        notice: Notice::Turn { active: Side::You },
    };
    let timing = StepTiming {
        passes: true,
        ..timing(1_000.0)
    };
    (act, timing)
}

/// A coin flipped for the Angel, shown for 500 ms with the table to itself.
fn coin(source: ObjectId) -> (Act, StepTiming) {
    let act = Act::Announce {
        notice: Notice::Coin {
            player: Side::You,
            won: true,
            source,
        },
    };
    let timing = StepTiming {
        alone: true,
        ..timing(500.0)
    };
    (act, timing)
}

#[test]
fn a_coin_after_a_turn_banner_starts_when_the_banner_ends() {
    let (start, cards) = before();
    let (banner, banner_timing) = banner();
    let (coin, coin_timing) = coin(cards.angel);
    // In one script: the banner's beat holds only until the banner begins, and the coin waits
    // until it ends.
    let one = scripted(&start, &[vec![banner.clone()], vec![coin.clone()]], None);
    let show = play(
        vec![
            (0, start.clone(), Vec::new()),
            (10, one, vec![banner_timing.clone(), coin_timing.clone()]),
        ],
        2_000,
    );
    assert_eq!(starts(script(&show, 1)), [0, 1_000]);
    // In two: the banner's script is over as the banner begins, and the next script's coin
    // waits until the banner ends.
    let first = scripted(&start, &[vec![banner]], None);
    let second = scripted(&first, &[vec![coin]], None);
    let show = play(
        vec![
            (0, start, Vec::new()),
            (10, first, vec![banner_timing]),
            (10, second, vec![coin_timing]),
        ],
        2_000,
    );
    assert_eq!(script(&show, 2).start, Some(10 * MS));
    assert_eq!(starts(script(&show, 2)), [1_000]);
}

#[test]
fn a_script_with_no_step_that_waits_is_not_laid_out_again() {
    let (start, cards) = before();
    let (after, timings) = three_beats(&start, cards);
    let show = play(vec![(0, start, Vec::new()), (10, after, timings)], 1_000);
    assert_eq!(show.relays(), 0);
}

#[test]
fn a_reset_plays_only_its_announcements() {
    let (start, cards) = before();
    let (banner, banner_timing) = banner();
    let mut island = start
        .view
        .objects
        .iter()
        .find(|object| object.slot == cards.island)
        .expect("the island")
        .clone();
    island.tapped = true;
    // The web stage applies a reset's other steps at once and plays its announcements as a
    // script of their own (`resetSteps` in ui/src/stage/table.ts).
    let mut reset = scripted(&start, &[], Some(cards.island));
    let mut builder = ScriptBuilder::reset();
    builder.beat([Act::Tap { object: island }]).beat([banner]);
    reset.view.script = Some(builder.build());
    let show = play(
        vec![
            (0, start, Vec::new()),
            (10, reset, vec![timing(150.0), banner_timing]),
        ],
        2_000,
    );
    let played = script(&show, 1);
    let verbs: Vec<_> = played.steps.iter().map(|step| step.verb).collect();
    assert_eq!(verbs, [beryl_protocol::script::Verb::Announce]);
    assert_eq!(starts(played), [0], "the banner does not wait for the tap");
}

#[test]
fn the_first_views_script_plays_nothing() {
    let (start, cards) = before();
    let (after, timings) = three_beats(&start, cards);
    let show = play(vec![(0, after, timings)], 1_000);
    assert!(script(&show, 0).steps.is_empty());
    assert_eq!(script(&show, 0).end, 0);
}
