//! The verbs `vanish` and `emerge` (docs/beryl-3d.md, "Verbs"): for every case of each in
//! `data/ui-conventions.json`, a script timed as the session times it plays to its end with no
//! jump, every object at its placement and every step played for its time; a vanishing card
//! fades where it stands, and an emerging ability travels from its source to the stack.

use crate::common::{MS, empty_frame, frame, show};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{ObjectId, PlayerView, Side, StackItem, Why, ZoneName};

use beryl_protocol::script::{Act, Curve as Named, Subject, Table as Played, Verb, time_script};
use beryl_scene::agent::hit::bounds;
use beryl_scene::agent::steps::{Sample, no_jumps, rest_equals_placement, step_shown, step_times};
use beryl_scene::data::audit;
use beryl_scene::frame::{Frame, Role};
use beryl_scene::motion::pose_at;
use beryl_scene::play::Show;
use beryl_scene::play::record::ScriptRecord;
use beryl_scene::pose::{Pose, Segment};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder};

/// The number the scripts here give the ability they put on the stack.
const ABILITY: u32 = 5;

/// The sample table, with a reset script.
fn table() -> GameUpdate {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    builder.update()
}

/// The slot of the card named `name` that `side` has in `zone`.
fn slot(view: &PlayerView, zone: ZoneName, side: Side, name: &str) -> ObjectId {
    view.objects
        .iter()
        .find(|o| o.zone == zone && o.controller == side && o.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("{side:?} has a {name} in {zone:?}"))
        .slot
}

/// An ability of the viewer's, from the card in `source` if it names one.
fn ability(source: Option<ObjectId>, triggered: bool) -> StackItem {
    StackItem::Ability {
        source,
        ability: 0,
        granted: false,
        name: "Serra Angel".to_owned(),
        triggered,
        taps_to_pay: None,
        nth: 0,
        text: None,
        controller: Side::You,
        manual: None,
        targets: Vec::new(),
        x: 0,
    }
}

/// `before` with `act` as its one step, and `change` made to its view: the view the act leads
/// to.
fn after(before: &GameUpdate, act: Act, change: impl FnOnce(&mut PlayerView)) -> GameUpdate {
    let mut view = before.view.clone();
    change(&mut view);
    view.script = Some(ScriptBuilder::default().beat([act]).build());
    GameUpdate {
        session: before.session,
        view,
    }
}

/// The view after the card in `slot` leaves `from` for `to` (its graveyard, exile or hand) as
/// `why` says.
fn leaves(
    before: &GameUpdate,
    slot: ObjectId,
    from: ZoneName,
    to: ZoneName,
    why: Why,
) -> GameUpdate {
    let mut record = before
        .view
        .objects
        .iter()
        .find(|o| o.slot == slot)
        .expect("the card is in the view")
        .clone();
    record.zone = to;
    let act = Act::Vanish {
        subject: Subject::Object { slot },
        from,
        why,
        object: Some(record.clone()),
        at: None,
    };
    after(before, act, |view| {
        view.battlefield.retain(|&s| s != slot);
        view.stack
            .retain(|item| !matches!(item, StackItem::Spell { slot: s, .. } if *s == slot));
        if to == ZoneName::Graveyard {
            view.players.you.graveyard.push(slot);
        }
        if let Some(object) = view.objects.iter_mut().find(|o| o.slot == slot) {
            *object = record;
        }
    })
}

/// The view after an ability from the card in `source` goes on the stack.
fn emerges(before: &GameUpdate, source: Option<ObjectId>, triggered: bool) -> GameUpdate {
    let item = ability(source, triggered);
    let act = Act::Emerge {
        id: ABILITY,
        item: item.clone(),
        at: None,
    };
    after(before, act, |view| view.stack.push(item))
}

/// The view after the ability on top of the stack finishes.
fn finishes(before: &GameUpdate) -> GameUpdate {
    let act = Act::Vanish {
        subject: Subject::Ability { id: ABILITY },
        from: ZoneName::Stack,
        why: Why::Resolve,
        object: None,
        at: None,
    };
    after(before, act, |view| {
        view.stack.pop();
    })
}

/// One frame drawn while the last script played: when, what it drew, and where the ability
/// numbered `ABILITY` and the card in `Moment::slot` were then.
struct Moment {
    t: u64,
    frame: Frame,
    ability: Option<Pose>,
    card: Option<Pose>,
}

/// A show that has played each of `updates` in turn, timed as the session times them, with a
/// frame every 10 ms until all have ended, and each frame drawn while the last one played,
/// with the pose then of the card in `slot`.
fn play(updates: &[GameUpdate], slot: Option<ObjectId>) -> (Show, Vec<Moment>) {
    let mut show = show();
    show.record();
    let mut scratch = empty_frame();
    let mut now = 0;
    let mut moments = Vec::new();
    let mut last = &updates[0];
    for (index, update) in updates.iter().enumerate() {
        let timings = match &update.view.script {
            Some(script) if index > 0 => time_script(script, &Played::of(&last.view)),
            _ => Vec::new(),
        };
        show.push(Box::new(update.clone()), timings);
        let until = now + 3_000 * MS;
        while now < until {
            let frame = frame(&mut show, &mut scratch, now);
            if index == updates.len() - 1 {
                moments.push(Moment {
                    t: now,
                    frame,
                    ability: show.ability_pose(ABILITY, now),
                    card: slot.and_then(|slot| show.pose(usize::from(slot.0), now)),
                });
            }
            now += 10 * MS;
        }
        last = update;
    }
    (show, moments)
}

/// The moment of `moments` nearest after `t`.
fn at(moments: &[Moment], t: u64) -> &Moment {
    moments
        .iter()
        .find(|moment| moment.t >= t)
        .expect("a frame was drawn then")
}

/// The record of the last script `show` played, after checking that it has started.
fn last(show: &Show) -> &ScriptRecord {
    let script = show.history().last().expect("a script was played");
    assert!(script.start.is_some(), "the script has started");
    script
}

/// The last script `show` played has the case `case`, and no segment check finds a fault in
/// it.
fn plays_cleanly(show: &Show, verb: Verb, case: &str) {
    let script = last(show);
    let step = script.steps.last().expect("a step");
    assert_eq!(step.verb, verb);
    for (check, found) in [
        ("rest equals placement", rest_equals_placement(script)),
        ("no jumps", no_jumps(script)),
        ("step times", step_times(script)),
    ] {
        assert!(found.is_empty(), "{case}: {check} found {found:?}");
    }
}

/// The case `time_script` gives the last step of `update`'s script.
fn case_of(before: &GameUpdate, update: &GameUpdate) -> Option<String> {
    let script = update.view.script.as_ref().expect("a script");
    time_script(script, &Played::of(&before.view))
        .last()
        .and_then(|timing| timing.case.clone())
}

#[test]
fn every_case_of_vanish_plays_to_its_place() {
    use ZoneName::{Battlefield, Exile, Graveyard, Hand, Stack};
    let start = table();
    let view = &start.view;
    let angel = slot(view, ZoneName::Battlefield, Side::You, "Serra Angel");
    let pacifism = slot(view, ZoneName::Battlefield, Side::Opponent, "Pacifism");
    let bolt = slot(view, ZoneName::Stack, Side::You, "Lightning Bolt");
    let cases = [
        (
            "destroyed",
            leaves(&start, angel, Battlefield, Graveyard, Why::Destroy),
        ),
        (
            "countered",
            leaves(&start, bolt, Stack, Graveyard, Why::Counter),
        ),
        (
            "resolved instant or sorcery",
            leaves(&start, bolt, Stack, Graveyard, Why::Resolve),
        ),
        (
            "exiled",
            leaves(&start, pacifism, Battlefield, Exile, Why::Exile),
        ),
        (
            "bounced",
            leaves(&start, angel, Battlefield, Hand, Why::Return),
        ),
        (
            "sacrificed",
            leaves(&start, angel, Battlefield, Graveyard, Why::Sacrifice),
        ),
    ];
    for (case, update) in cases {
        assert_eq!(case_of(&start, &update).as_deref(), Some(case));
        let (show, _) = play(&[start.clone(), update], None);
        plays_cleanly(&show, Verb::Vanish, case);
    }
    let emerged = emerges(&start, Some(angel), true);
    let finished = finishes(&emerged);
    assert_eq!(
        case_of(&emerged, &finished).as_deref(),
        Some("ability finished")
    );
    let (show, _) = play(&[start, emerged, finished], None);
    plays_cleanly(&show, Verb::Vanish, "ability finished");
    assert!(show.table().abilities().is_empty(), "the ability is gone");
}

#[test]
fn a_vanishing_card_fades_where_it_stands() {
    let start = table();
    let angel = slot(&start.view, ZoneName::Battlefield, Side::You, "Serra Angel");
    let destroyed = leaves(
        &start,
        angel,
        ZoneName::Battlefield,
        ZoneName::Graveyard,
        Why::Destroy,
    );
    let script = destroyed.view.script.as_ref().expect("a script");
    let timing = &time_script(script, &Played::of(&start.view))[0];
    assert_eq!(timing.curve, Named::Linear, "vanish's curve is \"none\"");
    let (show, _) = play(&[start, destroyed], None);
    let record = last(&show);
    let step = &record.steps[0];
    let segments: Vec<Segment> = record
        .track(usize::from(angel.0))
        .iter()
        .map(|record| record.segment)
        .collect();
    let pose = |t: u64| pose_at(&segments, t);
    let before = pose(step.from);
    let half = pose(step.from + (step.to - step.from) / 2);
    assert_eq!(half.position, before.position, "it stays where it was");
    assert!(
        (half.opacity - 0.75).abs() < 1e-3,
        "at half its step it is 1 - 0.5² opaque: {}",
        half.opacity
    );
    let ended = pose(step.to);
    assert!(
        ended.position != before.position,
        "it ends in the graveyard"
    );
    assert!((ended.opacity - 1.0).abs() < 1e-6, "seen there");
}

/// The ability numbered `ABILITY` as each of `moments` draws it, as "every step shown" samples
/// a step's subject.
fn ability_samples(moments: &[Moment]) -> Vec<Sample> {
    moments
        .iter()
        .filter_map(|moment| {
            let element = moment
                .frame
                .resolved
                .iter()
                .find(|element| element.role == Role::Ability)?;
            let seen = element.opacity > audit::TRANSPARENT;
            Some(Sample {
                t: moment.t,
                rect: bounds(element),
                seen,
                hidden: (!seen).then_some("transparent"),
            })
        })
        .collect()
}

#[test]
fn every_case_of_emerge_travels_from_the_source_to_the_stack() {
    let start = table();
    let angel = slot(&start.view, ZoneName::Battlefield, Side::You, "Serra Angel");
    for (case, triggered) in [("triggered ability", true), ("activated ability", false)] {
        let emerged = emerges(&start, Some(angel), triggered);
        assert_eq!(case_of(&start, &emerged).as_deref(), Some(case));
        let (show, moments) = play(&[start.clone(), emerged], Some(angel));
        plays_cleanly(&show, Verb::Emerge, case);
        let record = last(&show);
        let step = record.steps.last().expect("a step");
        let (from, to) = record.span(step).expect("the script started");
        let first = at(&moments, from);
        let source = first.card.expect("the Angel");
        let begins = first.ability.expect("the ability");
        assert!(
            begins
                .position
                .truncate()
                .distance(source.position.truncate())
                < 5.0,
            "{case}: it starts in the middle of its source: {begins:?} {source:?}"
        );
        assert!(begins.scale < 0.7, "{case}: small at first");
        let landed = at(&moments, to);
        let rests = landed.ability.expect("on the stack");
        let table = show.table();
        let id = *table.abilities().last().expect("an ability on the stack");
        let placed = table.ability_rest(id).expect("placed on the stack");
        assert!(
            (rests.scale - placed.scale).abs() < 1e-6 && rests.scale > begins.scale,
            "{case}: it grows to its size on the stack"
        );
        assert!(
            moments
                .iter()
                .filter(|moment| moment.t >= from && moment.t <= to)
                .all(|moment| moment.card == Some(source)),
            "{case}: its source does not move"
        );
        // "Every step shown" judges the ability as it judges a flight.
        let mut judged = step.clone();
        judged.subject = Some(0);
        assert_eq!(
            step_shown(&judged, from, to, &ability_samples(&moments)),
            None,
            "{case}: the ability moves far enough for long enough to be seen"
        );
    }
}

#[test]
fn an_ability_with_no_source_on_the_table_fades_in_at_its_place() {
    let start = table();
    let emerged = emerges(&start, None, true);
    let (show, moments) = play(&[start, emerged], None);
    plays_cleanly(&show, Verb::Emerge, "no source");
    let record = last(&show);
    let (from, to) = record.span(&record.steps[0]).expect("started");
    let begins = at(&moments, from).ability.expect("the ability");
    let rests = at(&moments, to).ability.expect("on the stack");
    assert_eq!(begins.position, rests.position, "it does not travel");
    assert!(begins.opacity < 0.1, "it fades in: {}", begins.opacity);
}

#[test]
fn what_a_vanishing_card_leaves_room_for_closes_up_after_it() {
    use beryl_protocol::protocol::CardType;
    use beryl_scene::play::record::Cause;
    // The sample table with a second creature in the Angel's row, which closes up after it.
    let mut builder = ViewBuilder::sample();
    builder
        .card(ZoneName::Battlefield, Side::You, "Savannah Lions")
        .types = vec![CardType::Creature];
    builder.script(ScriptBuilder::reset().build());
    let start = builder.update();
    let angel = slot(&start.view, ZoneName::Battlefield, Side::You, "Serra Angel");
    let bounced = leaves(
        &start,
        angel,
        ZoneName::Battlefield,
        ZoneName::Hand,
        Why::Return,
    );
    let (show, _) = play(&[start, bounced], None);
    let record = last(&show);
    let step = &record.steps[0];
    // The web stage's `roomIn`: 40% of the step, in whole milliseconds, once the card is gone.
    let closes = ((step.to - step.from) as f64 * 0.4 / MS as f64).round() as u64 * MS;
    let slides: Vec<Segment> = record
        .segments
        .iter()
        .filter(|record| record.slot != usize::from(angel.0) && record.cause == Cause::Slide(0))
        .map(|record| record.segment)
        .collect();
    assert!(!slides.is_empty(), "what the Angel left room for closes up");
    for slide in &slides {
        assert_eq!(slide.start, step.to, "it closes up once the Angel is gone");
        assert_eq!(slide.end(), step.to + closes, "for 40% of the step");
    }
    assert_eq!(record.end, step.to + closes, "the beat lasts until it has");
}

#[test]
fn a_hand_a_card_vanishes_from_closes_up_over_the_step() {
    use beryl_scene::play::record::Cause;
    let start = table();
    let counterspell = slot(&start.view, ZoneName::Hand, Side::You, "Counterspell");
    let discarded = leaves(
        &start,
        counterspell,
        ZoneName::Hand,
        ZoneName::Graveyard,
        Why::Discard,
    );
    let (show, _) = play(&[start, discarded], None);
    let record = last(&show);
    let step = &record.steps[0];
    let slides: Vec<Segment> = record
        .segments
        .iter()
        .filter(|record| record.slot != usize::from(counterspell.0))
        .filter(|record| record.cause == Cause::Slide(0))
        .map(|record| record.segment)
        .collect();
    assert!(!slides.is_empty(), "the rest of the hand closes up");
    for slide in &slides {
        assert!(
            slide.end() <= step.to,
            "within the step, as on the web stage"
        );
    }
    assert_eq!(record.end, step.to, "nothing waits after the step");
}
