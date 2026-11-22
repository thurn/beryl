//! The verbs that show a card where it lies or hold it up: `flip` turns a card over in place,
//! `showcase` adds its face to the Revealed Cards pile, and `lift` holds up the cards the
//! opponent is handling. Each is played on views and scripts built by hand, with the durations,
//! delays and staggers of `data/ui-conventions.json`, and checked by the segment checks.

use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{CardHandling, Facing, ObjectId, ObjectView, Side, ZoneName};
use beryl_protocol::script::{Act, Arrow, Curve, Fact, Span, StepTiming, Subject, Verb};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, step, timing};
use game_play::play::Show;
use game_play::verbs;
use game_probe::agent::steps::{no_jumps, rest_equals_placement, step_times};
use game_table::table::Place;
use glam::Vec2;
use kit_frame::frame::{Frame, REVEALED_FACE, Role, StateBits};
use kit_probe::agent::checks::duplicates;
use kit_probe::agent::tree;
use kit_probe::agent::{Context, Layout};
use kit_ui::input::{Insets, Size};

use crate::common::{MS, empty_frame, frame, show};

/// The timing of `verb`'s step, or of its case `case`, as the data file gives it.
fn data_timing(verb: &str, case: Option<&str>) -> StepTiming {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../../data/ui-conventions.json"))
            .expect("the data file parses");
    let entry = data["verbs"]
        .as_array()
        .expect("a list of verbs")
        .iter()
        .find(|entry| entry["name"] == verb)
        .expect("the verb is in the data file");
    let values = match case {
        Some(case) => &entry["cases"][case],
        None => &entry["values"],
    };
    let number = |key: &str| values[key].as_f64().unwrap_or(0.0);
    StepTiming {
        case: case.map(str::to_owned),
        delay: number("delay"),
        stagger: number("stagger"),
        curve: match values["curve"].as_str() {
            Some("ease-in") => Curve::EaseIn,
            Some("ease-out") => Curve::EaseOut,
            Some("ease-in-out") => Curve::EaseInOut,
            _ => Curve::Linear,
        },
        ..timing(number("duration"))
    }
}

/// Plays `start` and then `after`, whose script's steps take `timings`, to the end, and returns
/// the show, which has kept a record of `after`'s script.
fn played(start: &GameUpdate, after: &GameUpdate, timings: Vec<StepTiming>) -> Show {
    let mut show = show();
    let mut buffer = empty_frame();
    show.push(Box::new(start.clone()), Vec::new());
    frame(&mut show, &mut buffer, 0);
    show.record();
    show.push(Box::new(after.clone()), timings);
    frame(&mut show, &mut buffer, 10_000 * MS);
    show
}

/// The segment checks find nothing in any script `show` recorded.
fn checks_pass(show: &Show) {
    let history = show.history();
    assert!(!history.is_empty(), "a script was recorded");
    for script in history {
        let found: Vec<String> = [
            rest_equals_placement(script),
            no_jumps(script),
            step_times(script),
        ]
        .into_iter()
        .flatten()
        .map(|found| found.message)
        .collect();
        assert!(found.is_empty(), "{found:#?}");
    }
}

/// The view after `update`, with `acts` as its script, one beat each.
fn scripted(
    update: &GameUpdate,
    view: &beryl_protocol::protocol::PlayerView,
    acts: Vec<Act>,
) -> GameUpdate {
    let mut script = ScriptBuilder::default();
    for act in acts {
        script.beat([act]);
    }
    let mut view = view.clone();
    view.script = Some(script.build());
    GameUpdate {
        session: update.session,
        view,
    }
}

/// The record of the card in `slot` in `update`'s view.
fn record(update: &GameUpdate, slot: ObjectId) -> ObjectView {
    update
        .view
        .objects
        .iter()
        .find(|object| object.slot == slot)
        .expect("the card is in the view")
        .clone()
}

fn replaced(update: &GameUpdate, object: &ObjectView) -> beryl_protocol::protocol::PlayerView {
    let mut view = update.view.clone();
    for shown in &mut view.objects {
        if shown.slot == object.slot {
            *shown = object.clone();
        }
    }
    view
}

/// The card element of `id` in `frame`.
fn card(frame: &Frame, id: u32) -> Option<&kit_frame::frame::Resolved> {
    frame
        .resolved
        .iter()
        .find(|element| element.role == Role::Card && element.id.0 == id)
}

fn context() -> Context {
    Context {
        recording: "verbs-shown".to_owned(),
        seat: Side::You,
        layout: Layout::Desktop,
        audit: crate::test_data::data().audit,
        checks: Layout::Desktop.checks(
            &crate::test_data::data().profiles,
            kit_base::geometry::Size::default(),
        ),
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        safe_area: Insets::default(),
        at: 0,
    }
}

#[test]
fn the_three_verbs_are_built() {
    for verb in [Verb::Flip, Verb::Showcase, Verb::Lift] {
        assert!(verbs::built(verb), "{verb:?}");
    }
}

/// A view with an opponent's face-down permanent, and the update in which it turns face up.
fn face_down() -> (GameUpdate, GameUpdate, ObjectId) {
    let mut builder = ViewBuilder::new();
    let hidden = builder.card(ZoneName::Battlefield, Side::Opponent, "Serra Angel");
    hidden.facing = Facing::Down;
    hidden.name = None;
    let slot = hidden.slot;
    builder.script(ScriptBuilder::reset().build());
    let start = builder.update();
    let mut turned = record(&start, slot);
    turned.facing = Facing::Up;
    turned.name = Some("Serra Angel".to_owned());
    let view = replaced(&start, &turned);
    let after = scripted(&start, &view, vec![Act::Flip { object: turned }]);
    (start, after, slot)
}

#[test]
fn a_flipped_card_turns_over_at_the_middle_of_its_step_and_ends_at_its_place() {
    let (start, after, slot) = face_down();
    let flip = data_timing("flip", None);
    let duration = (flip.duration * 1e6) as u64;
    let checked = played(&start, &after, vec![flip.clone()]);
    checks_pass(&checked);

    let mut show = show();
    let mut buffer = empty_frame();
    show.push(Box::new(start), Vec::new());
    frame(&mut show, &mut buffer, 0);
    show.push(Box::new(after), vec![flip]);
    let state = |show: &mut Show, buffer: &mut Frame, at: u64| {
        let frame = frame(show, buffer, at);
        card(&frame, u32::from(slot.0))
            .expect("the card is drawn")
            .state
    };
    let start_at = 1;
    frame(&mut show, &mut buffer, start_at);
    let middle = start_at + duration / 2;
    assert!(
        state(&mut show, &mut buffer, middle - MS).has(StateBits::FACE_DOWN),
        "still face down just before the middle"
    );
    assert!(
        !state(&mut show, &mut buffer, middle + MS).has(StateBits::FACE_DOWN),
        "face up just after the middle"
    );
    let up = show
        .pose(usize::from(slot.0), middle)
        .expect("the card has a pose");
    let rest = show.table().rest(usize::from(slot.0));
    assert!(
        rest.is_none_or(|rest| up.position.z > rest.position.z),
        "it is lifted off the table while it turns"
    );
}

/// A view with three cards in the viewer's hand, and the update in which each is held up to be
/// read, one beat each.
fn revealed() -> (GameUpdate, GameUpdate, Vec<ObjectId>) {
    let mut builder = ViewBuilder::new();
    let slots: Vec<ObjectId> = ["Counterspell", "Plains", "Memory Lapse"]
        .into_iter()
        .map(|name| builder.card(ZoneName::Hand, Side::You, name).slot)
        .collect();
    builder.script(ScriptBuilder::reset().build());
    let start = builder.update();
    let acts = slots
        .iter()
        .map(|&slot| Act::Showcase {
            object: record(&start, slot),
        })
        .collect();
    let after = scripted(&start, &start.view, acts);
    (start, after, slots)
}

#[test]
fn a_showcased_card_lies_where_it_was_and_its_face_joins_the_pile() {
    let (start, after, slots) = revealed();
    let showcase = data_timing("showcase", None);
    let timings = vec![showcase; slots.len()];
    let show = played(&start, &after, timings.clone());
    checks_pass(&show);

    let mut show = show;
    let mut buffer = empty_frame();
    let frame = frame(&mut show, &mut buffer, 20_000 * MS);
    let first = u32::from(slots[0].0);
    assert!(card(&frame, first).is_some(), "the card itself is drawn");
    assert!(
        card(&frame, REVEALED_FACE + first).is_some(),
        "and its face in the pile"
    );
    assert!(duplicates(&frame, &context()).is_empty());
    // The faces lie in the order they were revealed, each to the right of the one before.
    let lefts: Vec<f32> = slots
        .iter()
        .map(|slot| {
            let face = card(&frame, REVEALED_FACE + u32::from(slot.0)).expect("a face");
            face.corners
                .iter()
                .map(|corner: &Vec2| corner.x)
                .fold(f32::MAX, f32::min)
        })
        .collect();
    assert!(lefts.windows(2).all(|pair| pair[0] < pair[1]), "{lefts:?}");
    // Each nearer the eye than the one before, so that the last revealed lies on top.
    let depths: Vec<f32> = slots
        .iter()
        .map(|slot| {
            card(&frame, REVEALED_FACE + u32::from(slot.0))
                .expect("a face")
                .depth
        })
        .collect();
    assert!(
        depths.windows(2).all(|pair| pair[0] > pair[1]),
        "{depths:?}"
    );
}

/// After a card is revealed and the table comes to rest, its face in the Revealed Cards pile is
/// in the place `revealed`, and no card element is in the zone `nowhere`, where a card goes
/// that has left every zone. `just b3d stand-in` once saw the opponent's searched-for card's
/// face printed as `nowhere.yours`, which read as an object the client had failed to drop.
#[test]
fn a_revealed_face_rests_in_the_revealed_pile_and_no_card_in_nowhere() {
    let (start, after, slots) = revealed();
    let showcase = data_timing("showcase", None);
    let mut show = played(&start, &after, vec![showcase; slots.len()]);
    let mut buffer = empty_frame();
    let frame = frame(&mut show, &mut buffer, 20_000 * MS);
    let face = REVEALED_FACE + u32::from(slots[0].0);
    let index = (frame.resolved.iter())
        .position(|element| element.role == Role::Card && element.id.0 == face)
        .expect("the face is drawn");
    let line = tree::line(&frame, u32::try_from(index).expect("few elements"));
    assert!(line.contains(" revealed "), "{line}");
    let printed = tree::tree(
        &frame,
        &tree::TreeOptions {
            within: Some("revealed".to_owned()),
            ..tree::TreeOptions::default()
        },
    )
    .expect("`revealed` is a place");
    assert_eq!(printed.matches("card ").count(), slots.len(), "{printed}");
    let nowhere: Vec<String> = (0..frame.resolved.len())
        .filter(|&index| {
            let element = &frame.resolved[index];
            element.role == Role::Card
                && matches!(
                    Place::in_frame(element.place),
                    Some(Place::Card {
                        zone: ZoneName::Nowhere,
                        ..
                    })
                )
        })
        .map(|index| tree::line(&frame, u32::try_from(index).expect("few elements")))
        .collect();
    assert!(nowhere.is_empty(), "{nowhere:#?}");
}

#[test]
fn a_reset_empties_the_revealed_pile() {
    let (start, after, slots) = revealed();
    let showcase = data_timing("showcase", None);
    let mut show = played(&start, &after, vec![showcase; slots.len()]);
    let mut buffer = empty_frame();
    let mut again = start.clone();
    again.view.script = Some(ScriptBuilder::reset().build());
    show.push(Box::new(again), Vec::new());
    let frame = frame(&mut show, &mut buffer, 20_000 * MS);
    assert!(card(&frame, REVEALED_FACE + u32::from(slots[0].0)).is_none());
}

/// A view with five cards in the opponent's library, and the updates in which they look at the
/// top three and stop.
fn looked_at() -> (GameUpdate, GameUpdate, GameUpdate, Vec<ObjectId>) {
    let mut builder = ViewBuilder::new();
    for _ in 0..5 {
        builder.card(ZoneName::Library, Side::Opponent, "Island");
    }
    builder.card(ZoneName::Library, Side::You, "Island");
    builder.script(ScriptBuilder::reset().build());
    let start = builder.update();
    let library = start.view.players.opponent.library.clone();
    let top: Vec<ObjectId> = library.iter().rev().take(3).copied().collect();
    let handling = CardHandling::Library {
        side: Side::Opponent,
        count: Some(3),
        scry: Some(true),
    };
    let mut handled = start.view.clone();
    handled.handling = Some(handling);
    let begin = scripted(
        &start,
        &handled,
        vec![Act::Lift {
            span: Span::Begin,
            handling,
        }],
    );
    let mut done = start.view.clone();
    done.handling = None;
    let end = scripted(
        &begin,
        &done,
        vec![Act::Lift {
            span: Span::End,
            handling,
        }],
    );
    (start, begin, end, top)
}

/// The plain values of `lift` for a look at `count` cards of a library: each card as long as one
/// alone, one after another, as the session times it.
fn library_look(count: f64) -> StepTiming {
    let own = data_timing("lift", None);
    StepTiming {
        duration: own.duration + (count - 1.0) * own.stagger,
        each: Some(own.duration),
        ..own
    }
}

#[test]
fn looked_at_cards_come_off_their_library_and_go_back() {
    for timing in [
        library_look(3.0),
        data_timing("lift", Some("tray")),
        data_timing("lift", Some("sort")),
    ] {
        let (start, begin, end, top) = looked_at();
        let mut show = show();
        let mut buffer = empty_frame();
        show.record();
        show.push(Box::new(start.clone()), Vec::new());
        frame(&mut show, &mut buffer, 0);
        show.push(Box::new(begin.clone()), vec![timing.clone()]);
        // A script starts at the first frame after it is given.
        frame(&mut show, &mut buffer, MS);
        // Once the look has begun, each card is where the table holds up the cards handled.
        let lifted = show.table().placement(&begin.view, &[]);
        let resting = show.table().placement(&start.view, &[]);
        let up = 5_000 * MS;
        frame(&mut show, &mut buffer, up);
        for slot in &top {
            let pose = show.pose(usize::from(slot.0), up).expect("a pose");
            let place = lifted.get(*slot).expect("a place").pose;
            let pile = resting.get(*slot).expect("a place").pose;
            assert_eq!(pose, place, "{slot:?} is held up");
            assert_ne!(place, pile, "{slot:?} is off its pile");
        }
        show.push(Box::new(end), vec![timing.clone()]);
        frame(&mut show, &mut buffer, up + MS);
        let back = 10_000 * MS;
        frame(&mut show, &mut buffer, back);
        for slot in &top {
            let pose = show.pose(usize::from(slot.0), back).expect("a pose");
            let pile = resting.get(*slot).expect("a place").pose;
            assert_eq!(pose, pile, "{slot:?} is back on its pile");
        }
        checks_pass(&show);
    }
}

#[test]
fn a_library_choice_keeps_the_cards_on_the_pile() {
    let (start, _, _, _) = looked_at();
    let mut choice = start.view.clone();
    choice.handling = Some(CardHandling::Library {
        side: Side::Opponent,
        count: Some(3),
        scry: None,
    });
    let order = choice.players.opponent.library.clone();
    let after = scripted(
        &start,
        &choice,
        vec![Act::Set {
            fact: Fact::Library {
                player: Side::Opponent,
                order: order.clone(),
            },
        }],
    );
    let show = played(&start, &after, vec![timing(0.0)]);
    let pile = show.table().placement(&start.view, &[]);
    for slot in order {
        assert_eq!(
            show.pose(usize::from(slot.0), 10_000 * MS),
            pile.get(slot).map(|rest| rest.pose),
            "{slot:?} stays on the pile"
        );
    }
    checks_pass(&show);
}

#[test]
fn a_look_at_several_cards_takes_them_one_after_another() {
    let (start, begin, _, top) = looked_at();
    let timing = library_look(3.0);
    let show = played(&start, &begin, vec![timing.clone()]);
    let script = &show.history()[0];
    let starts: Vec<u64> = top
        .iter()
        .map(|slot| {
            script
                .track(usize::from(slot.0))
                .iter()
                .map(|record| record.segment)
                .filter(|segment| segment.duration > 0)
                .map(|segment| segment.start)
                .min()
                .expect("the card moves")
        })
        .collect();
    let stagger = (timing.stagger * 1e6) as u64;
    assert_eq!(starts[1] - starts[0], stagger, "{starts:?}");
    assert_eq!(starts[2] - starts[1], stagger, "{starts:?}");
}

/// A look that ends with no `lift` step, at a script whose first step is the end of an arrow
/// that leads its beat (`StepTiming::leads`), as the session sends the end of an arrow a card's
/// text named: the looked-at cards go back to their pile over that step. A step that leads once
/// had its act counted as going ahead of it into no time, so the cards jumped back.
#[test]
fn looked_at_cards_go_back_over_a_leading_arrow_end() {
    let (start, mut begin, _, top) = looked_at();
    // A line from the opponent to the top card, begun as the look does.
    let (arrow, from, to) = (
        Arrow::Source,
        Subject::Player {
            player: Side::Opponent,
        },
        Subject::Object { slot: top[0] },
    );
    let point = |span| Act::Point {
        span,
        arrow,
        from,
        to,
    };
    begin
        .view
        .script
        .as_mut()
        .expect("a script")
        .beats
        .push(vec![step(point(Span::Begin))]);
    let mut done = start.view.clone();
    done.handling = None;
    let end = scripted(&begin, &done, vec![point(Span::End)]);
    let leads = StepTiming {
        hold: Some(290.0),
        with: true,
        leads: true,
        ..timing(33.0)
    };
    let mut show = show();
    let mut buffer = empty_frame();
    show.push(Box::new(start), Vec::new());
    frame(&mut show, &mut buffer, 0);
    show.push(Box::new(begin), vec![library_look(3.0), timing(33.0)]);
    frame(&mut show, &mut buffer, MS);
    frame(&mut show, &mut buffer, 5_000 * MS);
    show.record();
    show.push(Box::new(end), vec![leads]);
    for ms in (5_001..=6_000).step_by(16) {
        frame(&mut show, &mut buffer, ms * MS);
    }
    checks_pass(&show);
}
