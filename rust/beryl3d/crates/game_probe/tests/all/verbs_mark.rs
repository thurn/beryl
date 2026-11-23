//! The verb `mark` and the marks it draws (docs/beryl-3d.md, "Objects and placement": marks are
//! separate quads): every case of the verb in `data/ui-conventions.json` plays to its end with
//! every object at its placement; a card wears its mark's state from the mark's begin to its
//! end, across scripts; an outline encloses its card in flight; a mark under another card lies
//! in the table's band; and a card picked to pay turns and wears no outline. The
//! "relations hold" check faults an outline off its card.

use crate::common::{MS, before, discard, empty_frame, frame, show};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{ObjectId, Side};
use beryl_protocol::script::{
    Act, Mark, Span as Edge, Subject, Table as Played, Verb, time_script,
};
use beryl_protocol::testing::ScriptBuilder;
use game_play::light::marks;
use game_play::play::Show;
use game_play::play::record::ScriptRecord;
use game_probe::agent::steps::{no_jumps, rest_equals_placement, step_times};
use glam::Vec3;
use kit_base::data::light::Light;
use kit_base::data::profiles::SizeClass;
use kit_base::pose::Pose;
use kit_frame::frame::{
    Band, Draw, ElementId, Frame, Material, Resolved, Role, ShapeAnchor, StateBits, sprites,
};
use kit_probe::agent::checks::relations_hold;
use kit_probe::agent::{Context, Rule};
use kit_ui::input::Insets;

const fn card(slot: ObjectId) -> Subject {
    Subject::Object { slot }
}

/// `before` with one step that begins or ends `mark` on `on`. A mark changes nothing in the
/// view.
fn marked(before: &GameUpdate, span: Edge, mark: Mark, on: Subject) -> GameUpdate {
    let mut view = before.view.clone();
    view.script = Some(
        ScriptBuilder::default()
            .beat([Act::Mark { span, mark, on }])
            .build(),
    );
    GameUpdate {
        session: before.session,
        view,
    }
}

/// The case `time_script` gives the last step of `update`'s script, played after `before`.
fn case_of(before: &GameUpdate, update: &GameUpdate) -> Option<String> {
    let script = update.view.script.as_ref().expect("a script");
    time_script(script, &Played::of(&before.view))
        .last()
        .and_then(|timing| timing.case.clone())
}

/// A show that has played each of `updates` in turn, the first as it is and each later one
/// timed as the session times it, with a frame every 10 ms for 1.5 s after each; and every
/// frame it drew, with its time.
fn play(updates: &[GameUpdate]) -> (Show, Vec<(u64, Frame)>) {
    let mut show = show();
    show.record();
    let mut scratch = empty_frame();
    let mut now = 0;
    let mut frames = Vec::new();
    for (index, update) in updates.iter().enumerate() {
        let timings = match &update.view.script {
            Some(script) if index > 0 => time_script(script, &Played::of(&updates[index - 1].view)),
            _ => Vec::new(),
        };
        show.push(Box::new(update.clone()), timings);
        let until = now + 1_500 * MS;
        while now < until {
            frames.push((now, frame(&mut show, &mut scratch, now)));
            now += 10 * MS;
        }
    }
    (show, frames)
}

/// The record of the script `back` scripts before the last that `show` played, started.
fn played(show: &Show, back: usize) -> &ScriptRecord {
    let history = show.history();
    let script = &history[history.len() - 1 - back];
    assert!(script.start.is_some(), "the script has started");
    script
}

/// When the only step of the script `back` before the last began and ended, in real time.
fn step_span(show: &Show, back: usize) -> (u64, u64) {
    let script = played(show, back);
    script.span(&script.steps[0]).expect("the script started")
}

/// The element of the card in `slot` in `frame`.
fn element(frame: &Frame, slot: ObjectId) -> &Resolved {
    frame
        .resolved
        .iter()
        .find(|element| element.id.0 == u32::from(slot.0))
        .expect("the card is drawn")
}

/// The records of `frame` drawn with `sprite` as a glow: the marks.
fn sprites_of(frame: &Frame, sprite: [u16; 4]) -> Vec<&Draw> {
    frame
        .draws
        .iter()
        .filter(|draw| draw.material == Material::Glow as u8 && draw.uv == sprite)
        .collect()
}

/// Where the quad of `draw` takes the point `(x, y)` of its unit square, in the table's space.
fn quad_point(draw: &Draw, x: f32, y: f32) -> Vec3 {
    let m = draw.model;
    Vec3::new(
        m[0] * x + m[1] * y + m[3],
        m[4] * x + m[5] * y + m[7],
        m[8] * x + m[9] * y + m[11],
    )
}

#[test]
fn every_case_of_mark_plays_to_its_place() {
    let (start, cards) = before();
    let angel = card(cards.angel);
    let island = card(cards.island);
    let begun = marked(&start, Edge::Begin, Mark::Selected, angel);
    let cases = [
        (
            "eligible",
            vec![
                start.clone(),
                marked(&start, Edge::Begin, Mark::Eligible, angel),
            ],
        ),
        ("selected", vec![start.clone(), begun.clone()]),
        (
            "target",
            vec![
                start.clone(),
                marked(&start, Edge::Begin, Mark::FriendlyTarget, angel),
            ],
        ),
        (
            "paying",
            vec![
                start.clone(),
                marked(&start, Edge::Begin, Mark::Paying, island),
            ],
        ),
        (
            "end",
            vec![
                start.clone(),
                begun.clone(),
                marked(&begun, Edge::End, Mark::Selected, angel),
            ],
        ),
    ];
    for (case, updates) in cases {
        let last = &updates[updates.len() - 1];
        assert_eq!(
            case_of(&updates[updates.len() - 2], last).as_deref(),
            Some(case)
        );
        let (show, _) = play(&updates);
        let script = played(&show, 0);
        assert_eq!(script.steps[0].verb, Verb::Mark);
        for (check, found) in [
            ("rest equals placement", rest_equals_placement(script)),
            ("no jumps", no_jumps(script)),
            ("step times", step_times(script)),
        ] {
            assert!(found.is_empty(), "{case}: {check} found {found:?}");
        }
    }
}

#[test]
fn a_card_wears_its_mark_from_the_marks_begin_to_its_end_across_scripts() {
    let (start, cards) = before();
    let angel = card(cards.angel);
    let begun = marked(&start, Edge::Begin, Mark::Eligible, angel);
    let ended = marked(&begun, Edge::End, Mark::Eligible, angel);
    let (show, frames) = play(&[start, begun, ended]);
    let (begins, _) = step_span(&show, 1);
    let (ends, _) = step_span(&show, 0);
    assert!(begins < ends, "the mark is begun before it is ended");
    for (t, frame) in &frames {
        let eligible = element(frame, cards.angel).state.has(StateBits::ELIGIBLE);
        assert_eq!(
            eligible,
            *t >= begins && *t < ends,
            "eligible at {} ms (marked from {} to {} ms)",
            t / MS,
            begins / MS,
            ends / MS
        );
        assert!(
            !element(frame, cards.island).state.has(StateBits::ELIGIBLE),
            "no other card is marked"
        );
    }
}

/// A selected card's outline is the stylesheet's four box shadows round the card's own box,
/// as the web interface's `.stage-mark` element is the card's size, through a flight that turns
/// and scales the card.
#[test]
fn an_outline_rings_its_card_through_a_flight() {
    let (start, cards) = before();
    let begun = marked(&start, Edge::Begin, Mark::Selected, card(cards.discard));
    let (flown, _) = discard(&begun, cards);
    let (show, frames) = play(&[start, begun, flown]);
    let (from, to) = step_span(&show, 0);
    let camera = show.table().camera();
    let mut checked = 0;
    for (t, frame) in frames
        .iter()
        .filter(|(t, _)| *t >= from && *t <= to + 20 * MS)
    {
        let shadows = (frame.draws.iter())
            .filter(|draw| draw.material == Material::BoxShadow as u8)
            .count();
        assert_eq!(shadows, 4, "four box shadows at {} ms", t / MS);
        assert_eq!(frame.rings.len(), 1, "one outline at {} ms", t / MS);
        let ring = &frame.rings[0];
        let mark = &frame.resolved[ring.element as usize];
        assert_eq!(mark.role, Role::Mark);
        assert_eq!(
            frame.resolved[mark.parent as usize].id.0,
            u32::from(cards.discard.0)
        );
        assert_eq!(
            frame.decoration_of(ring.element),
            Some(ShapeAnchor::Outline(ElementId(u32::from(cards.discard.0)))),
        );
        let m = ring.model;
        let [x, y] = ring.line;
        let outline = [(-x, -y), (x, -y), (x, y), (-x, y)].map(|(x, y)| {
            camera.to_screen(Vec3::new(
                m[0] * x + m[1] * y + m[3],
                m[4] * x + m[5] * y + m[7],
                m[8] * x + m[9] * y + m[11],
            ))
        });
        let corners = element(frame, cards.discard).corners;
        // The line runs along the card's edge: each of its corners within a point of one of
        // the card's.
        for corner in outline {
            let nearest = (corners.iter())
                .map(|card| card.distance(corner))
                .fold(f32::MAX, f32::min);
            assert!(
                nearest < 1.0,
                "at {} ms the outline's corner {corner} is {nearest} points from the card's \
                 {corners:?}",
                t / MS
            );
        }
        checked += 1;
    }
    assert!(checked > 10, "the flight was drawn: {checked} frames");
}

/// `marks::band` with the shipped tuning.
fn band(on: Subject, pose: &Pose) -> Band {
    let light = Light::parse(include_str!("../../../../../../data/beryl3d/light.json")).unwrap();
    marks::band(&light, on, pose)
}

#[test]
fn a_mark_under_another_card_lies_in_the_tables_band() {
    let (start, cards) = before();
    let mut show = show();
    let mut scratch = empty_frame();
    show.push(Box::new(start.clone()), Vec::new());
    frame(&mut show, &mut scratch, 0);
    let library = &start.view.players.you.library;
    let (bottom, top) = (library[0], library[library.len() - 1]);
    let pose = |slot: ObjectId| show.pose(usize::from(slot.0), 0).expect("in the pile");
    assert!(
        pose(top).position.z > pose(bottom).position.z,
        "the top card lies over the bottom one"
    );
    assert_eq!(band(card(bottom), &pose(bottom)), Band::TableMarks);
    let hand = cards.discard;
    assert_eq!(
        band(card(hand), &pose(hand)),
        Band::RaisedMarks,
        "a card in the hand is raised"
    );
    assert_eq!(
        band(Subject::Player { player: Side::You }, &pose(hand)),
        Band::RaisedMarks
    );
}

#[test]
fn a_permanent_picked_to_pay_turns_and_wears_a_tap_symbol() {
    let (start, cards) = before();
    // The Angel is untapped; the Island taps in the first script, which this one is not.
    let angel = card(cards.angel);
    let paying = marked(&start, Edge::Begin, Mark::Paying, angel);
    let (show, frames) = play(&[start, paying]);
    let (_, whole) = step_span(&show, 0);
    let extent = |frame: &Frame| {
        let corners = element(frame, cards.angel).corners;
        let (low, high) = corners
            .iter()
            .fold((corners[0], corners[0]), |(low, high), c| {
                (low.min(*c), high.max(*c))
            });
        high - low
    };
    let (_, first) = &frames[0];
    let upright = extent(first);
    assert!(upright.y > upright.x, "untapped, it is taller than wide");
    let (_, after) = frames
        .iter()
        .find(|(t, _)| *t > whole)
        .expect("a frame after the mark came up");
    let turned = extent(after);
    assert!(turned.x > turned.y, "paying, it lies turned: {turned}");
    assert!(after.rings.is_empty(), "no outline");
    let taps = sprites_of(after, sprites::TAP.rect);
    assert_eq!(taps.len(), 1, "a tap symbol");
    // The symbol lies over the card's face, which the mark lifts off the table, so that the
    // card does not hide it from the depth test.
    let tap = quad_point(taps[0], 0.0, 0.0);
    let face = after
        .draws
        .iter()
        .filter(|draw| draw.material == Material::Card as u8)
        .map(|draw| quad_point(draw, 0.0, 0.0))
        .min_by(|a, b| {
            a.truncate()
                .distance(tap.truncate())
                .total_cmp(&b.truncate().distance(tap.truncate()))
        })
        .expect("the card is drawn");
    assert!(
        tap.z > face.z,
        "the tap symbol at height {} lies over the paying card's face at {}",
        tap.z,
        face.z
    );
}

/// The "relations hold" check passes an outline the table drew round its card through a
/// flight, and faults the same outline moved 10 millimetres to the right or drawn at seven
/// tenths of its width, as the desktop app's outlines once were.
#[test]
fn the_marks_check_faults_an_outline_moved_off_its_card_or_narrower_than_it() {
    let (start, cards) = before();
    let begun = marked(&start, Edge::Begin, Mark::Selected, card(cards.discard));
    let (flown, _) = discard(&begun, cards);
    let (show, frames) = play(&[start, begun, flown]);
    let context = Context {
        size_class: SizeClass::Regular,
        audit: crate::test_data::data().audit,
        checks: crate::test_data::data()
            .profiles
            .for_size_class(SizeClass::Regular)
            .checks,
        size: show.table().camera().window.size,
        safe_area: Insets::default(),
        at: 0,
    };
    let mut checked = 0;
    for (t, frame) in frames.iter().filter(|(_, frame)| !frame.rings.is_empty()) {
        assert_eq!(
            relations_hold(frame, &context),
            Vec::new(),
            "the drawn outline is on its card at {} ms",
            t / MS
        );
        let mut moved = frame.clone();
        moved.rings[0].model[3] += 10.0;
        let mut narrowed = frame.clone();
        for column in [0, 4, 8] {
            narrowed.rings[0].model[column] *= 0.7;
        }
        for (how, broken) in [("moved", moved), ("narrowed", narrowed)] {
            let faults = relations_hold(&broken, &context);
            assert_eq!(faults.len(), 1, "the {how} outline at {} ms", t / MS);
            assert_eq!(faults[0].rule, Rule::RelationsHold);
        }
        checked += 1;
    }
    assert!(checked > 10, "the outline was drawn: {checked} frames");
}
