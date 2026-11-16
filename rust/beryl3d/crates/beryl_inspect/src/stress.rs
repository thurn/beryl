//! Stress boards: positions at the engine's capacity, built by hand with no engine, that the
//! frame-time gates draw. The engine knows only a few dozen cards, so a board made by playing
//! it would repeat a handful of names, stack into piles and cost less to draw than a real one.
//! Here every permanent has a name of its own. Nothing reads a clock or a random number, so
//! two calls return equal values.

use beryl_protocol::PlayerView;
use beryl_protocol::card_records::{CardEntry, CardFace, CardImages, CardRecords};
use beryl_protocol::protocol::{
    CardHandling, CardType, Facing, ObjectId, ObjectView, PlayerState, PowerToughness, Side,
    StackItem, TargetRef, Why, ZoneName,
};
use beryl_protocol::script::{
    Act, Arrow, Curve, Fact, Mark, Notice, Reset, Room, Span, Step, StepTiming, Stream, Subject,
    Table, Tally, replay, time_script,
};
use beryl_scene::testing;

/// How many permanents move in a stress board's second view.
const FLIGHTS: usize = 40;
/// How long each flight takes, in milliseconds.
const FLIGHT_DURATION: f64 = 420.0;
/// How long after one flight the next one starts, in milliseconds.
const FLIGHT_STAGGER: f64 = 30.0;

/// A synthetic board and any scripted changes to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Stress {
    /// The first view is a reset with every object at rest. Each later view carries a script.
    pub views: Vec<PlayerView>,
    /// For each view after the first, one timing per step of its script, in script order:
    /// `timings[i]` belongs to `views[i + 1]`, whose script carries the same timings as a
    /// session's does.
    pub timings: Vec<Vec<StepTiming>>,
}

/// The name of every stress board.
pub const NAMES: [&str; 5] = [
    "stress-255",
    "stress-1024",
    "stress-verbs",
    "stress-hand",
    "stress-stack",
];

/// The synthetic recording called `name`, if there is one.
#[must_use]
pub fn stress(name: &str) -> Option<Stress> {
    Some(match name {
        "stress-255" => board(255),
        "stress-1024" => board(1024),
        "stress-verbs" => verbs(),
        "stress-hand" => hand(),
        "stress-stack" => stack(),
        _ => return None,
    })
}

/// A battlefield of `count` permanents, then the same one after `FLIGHTS` of them have moved
/// to its end.
fn board(count: u16) -> Stress {
    let mut first = PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20));
    first.objects = (0..count).map(permanent).collect();
    first.battlefield = first.objects.iter().map(|object| object.slot).collect();
    first.script = Some(testing::script(
        Some(Reset::default()),
        Vec::new(),
        Vec::new(),
    ));

    // Each moved permanent joins the end of the battlefield, after the ones that stay.
    let mut second = first.clone();
    let moved: Vec<ObjectView> = second.objects[..FLIGHTS].to_vec();
    second.battlefield.rotate_left(FLIGHTS);
    second.objects.rotate_left(FLIGHTS);
    let end = count - 1;
    let beat: Vec<Step> = moved
        .into_iter()
        .map(|object| Step {
            act: Act::Fly {
                from: ZoneName::Battlefield,
                why: Why::Effect,
                object,
                at: Some(end),
                item: None,
                origin: None,
            },
            cause: None,
        })
        .collect();
    let timings: Vec<StepTiming> = beat.iter().map(|_| flight_timing()).collect();
    second.script = Some(testing::script(None, vec![beat], timings.clone()));

    Stress {
        views: vec![first, second],
        timings: vec![timings],
    }
}

/// The permanent in slot `index + 1`: its own name; a creature or a land by turns, a third of
/// them tapped, and the controllers alternating every two so that each kind is split evenly.
fn permanent(index: u16) -> ObjectView {
    let side = if (index / 2).is_multiple_of(2) {
        Side::You
    } else {
        Side::Opponent
    };
    let mut object = card(index, ZoneName::Battlefield, side);
    object.tapped = index.is_multiple_of(3);
    if index.is_multiple_of(2) {
        object.types = vec![CardType::Creature];
        object.pt = Some(PowerToughness {
            power: 2,
            toughness: 2,
        });
    } else {
        object.types = vec![CardType::Land];
    }
    object
}

/// The hand-built timing of one flight: every step the same, and the next begins after
/// `FLIGHT_STAGGER`.
fn flight_timing() -> StepTiming {
    StepTiming {
        case: None,
        duration: FLIGHT_DURATION,
        delay: 0.0,
        stagger: FLIGHT_STAGGER,
        curve: Curve::EaseInOut,
        each: None,
        with: false,
        after: Vec::new(),
        hold: None,
        leads: false,
        passes: false,
        alone: false,
        opens: false,
        ahead: false,
        closes: None,
        room: Room::Step,
        phases: Vec::new(),
        effects: Vec::new(),
    }
}

fn start(objects: Vec<ObjectView>, stack: Vec<StackItem>, reset: Reset) -> PlayerView {
    let mut view = PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20));
    for object in &objects {
        match (object.zone, object.owner) {
            (ZoneName::Battlefield, _) => view.battlefield.push(object.slot),
            (ZoneName::Library, Side::You) => view.players.you.library.push(object.slot),
            (ZoneName::Library, Side::Opponent) => view.players.opponent.library.push(object.slot),
            _ => {}
        }
    }
    view.objects = objects;
    view.stack = stack;
    view.script = Some(testing::script(Some(reset), Vec::new(), Vec::new()));
    view
}

/// Adds one beat timed by the same code that times live scripts. Use the replayed table to
/// build the next view, keeping ordered zones consistent with the beat's acts.
fn next(before: &PlayerView, table: &Table, acts: Vec<Act>) -> (PlayerView, Table) {
    let beat = acts
        .into_iter()
        .map(|act| Step { act, cause: None })
        .collect();
    let mut script = testing::script(None, vec![beat], Vec::new());
    script.timing = time_script(&script, table);
    let after = replay(table.clone(), &script).expect("stress beat replays");
    let mut view = before.clone();
    view.objects = after.objects.values().cloned().collect();
    view.battlefield.clone_from(&after.battlefield);
    view.stack = after.stack.iter().map(|entry| entry.item.clone()).collect();
    view.players.you.library.clone_from(&after.you.library);
    view.players.you.graveyard.clone_from(&after.you.graveyard);
    view.players
        .opponent
        .library
        .clone_from(&after.opponent.library);
    view.players
        .opponent
        .graveyard
        .clone_from(&after.opponent.graveyard);
    view.script = Some(script);
    (view, after)
}

fn recording(views: Vec<PlayerView>) -> Stress {
    let timings = views
        .iter()
        .skip(1)
        .map(|view| {
            view.script
                .as_ref()
                .expect("every view carries a script")
                .timing
                .clone()
        })
        .collect();
    Stress { views, timings }
}

fn card(index: u16, zone: ZoneName, side: Side) -> ObjectView {
    ObjectView::card(
        ObjectId(index + 1),
        format!("Stress {:03}", index + 1),
        zone,
        side,
        side,
    )
}

fn ability(source: &ObjectView, targets: Vec<TargetRef>) -> StackItem {
    StackItem::Ability {
        source: Some(source.slot),
        ability: 0,
        granted: false,
        name: source.name.clone().expect("a stress permanent has a name"),
        triggered: true,
        taps_to_pay: None,
        nth: 0,
        text: None,
        controller: source.controller,
        targets,
        x: 0,
        manual: None,
    }
}

fn subject(index: u16) -> Subject {
    Subject::Object {
        slot: ObjectId(index + 1),
    }
}

fn point(span: Span, arrow: Arrow, from: Subject, to: Subject) -> Act {
    Act::Point {
        span,
        arrow,
        from,
        to,
    }
}

/// Sixty cards in the viewer's hand, while the playback moves the pointer over the fan.
fn hand() -> Stress {
    recording(vec![start(
        (0..60)
            .map(|index| card(index, ZoneName::Hand, Side::You))
            .collect(),
        Vec::new(),
        Reset::default(),
    )])
}

/// The object ranges give each verb its own cards, except for arrows and marks that follow
/// moving objects. Ten tokens take the places of the ten that vanish.
fn verbs() -> Stress {
    let mut objects: Vec<_> = (0..100).map(permanent).collect();
    for (index, object) in objects.iter_mut().enumerate() {
        let (zone, side) = match index {
            0..20 => (ZoneName::Hand, Side::You),
            20..30 => (ZoneName::Library, Side::You),
            30..33 => (ZoneName::Library, Side::Opponent),
            33..43 => {
                object.token = true;
                continue;
            }
            _ => continue,
        };
        *object = card(object.slot.0 - 1, zone, side);
        if zone == ZoneName::Library {
            object.name = None;
        }
    }
    let first = start(objects, Vec::new(), Reset::default());
    // Apply instant record changes before writing motion on the same rows.
    let mut acts: Vec<_> = first.objects[89..94]
        .iter()
        .cloned()
        .map(|mut object| {
            object.summoning_sick = !object.summoning_sick;
            Act::Set {
                fact: Fact::Object { object },
            }
        })
        .collect();
    acts.extend(verb_moves(&first));
    acts.extend(verb_changes(&first));
    let (mut second, _) = next(&first, &Table::of(&first), acts);
    // The live timings sequence repeated moves for readability. Start them together here
    // to exercise all verb paths in the same frames, keeping their durations, phases and effects.
    for timing in &mut second.script.as_mut().expect("one beat").timing {
        timing.delay = 0.0;
        timing.stagger = 0.0;
        timing.after.clear();
        timing.leads = false;
        timing.alone = false;
        timing.passes = false;
    }
    second.handling = Some(CardHandling::Library {
        side: Side::Opponent,
        count: Some(3),
        scry: None,
    });
    recording(vec![first, second])
}

fn verb_moves(first: &PlayerView) -> Vec<Act> {
    let mut acts = Vec::new();
    for index in (20..30).rev() {
        acts.push(Act::Fly {
            from: ZoneName::Library,
            why: Why::Draw,
            object: card(index, ZoneName::Hand, Side::You),
            at: None,
            item: None,
            origin: None,
        });
    }
    for index in 0..20 {
        let mut object = permanent(index);
        object.owner = Side::You;
        object.controller = Side::You;
        object.tapped = false;
        acts.push(Act::Fly {
            from: ZoneName::Hand,
            why: Why::Play,
            object,
            at: None,
            item: None,
            origin: None,
        });
    }
    for index in 100..110 {
        let mut object = permanent(index);
        object.token = true;
        acts.push(Act::Appear {
            from: ZoneName::Nowhere,
            why: Why::Created,
            object,
            at: None,
            item: None,
        });
    }
    for index in 33..43 {
        acts.push(Act::Vanish {
            subject: subject(index),
            from: ZoneName::Battlefield,
            why: Why::Destroy,
            object: None,
            at: None,
        });
    }
    for id in 1..=5 {
        acts.push(Act::Emerge {
            id,
            item: ability(&first.objects[42 + id as usize], Vec::new()),
            at: None,
        });
    }
    acts
}

fn verb_changes(first: &PlayerView) -> Vec<Act> {
    let mut acts = Vec::new();
    for mut object in first.objects[48..53].iter().cloned() {
        object.facing = Facing::Down;
        acts.push(Act::Flip { object });
    }
    for mut object in first.objects[53..63].iter().cloned() {
        object.tapped = !object.tapped;
        acts.push(Act::Tap { object });
    }
    for index in 0..5 {
        let mut hit = first.objects[74 + usize::from(index)].clone();
        hit.damage = 1;
        acts.push(Act::Strike {
            source: ObjectId(65 + index * 2),
            to: subject(74 + index),
            amount: 1,
            object: Some(hit),
        });
        let mut hit = first.objects[79 + usize::from(index)].clone();
        hit.damage = 1;
        acts.push(Act::Streak {
            from: Some(Subject::Ability {
                id: u32::from(index) + 1,
            }),
            to: subject(79 + index),
            stream: Stream::Damage,
            amount: 1,
            object: Some(hit),
        });
    }
    for mut object in first.objects[84..89].iter().cloned() {
        let pt = object.pt.get_or_insert(PowerToughness {
            power: 2,
            toughness: 2,
        });
        pt.power += 1;
        acts.push(Act::Count {
            tally: Tally::Stats { object },
        });
    }
    acts.extend(
        first.objects[94..97]
            .iter()
            .cloned()
            .map(|object| Act::Showcase { object }),
    );
    acts.push(Act::Dwell {
        subject: Subject::Ability { id: 5 },
    });
    acts.push(Act::Announce {
        notice: Notice::Turn { active: Side::You },
    });
    for index in 0..5 {
        acts.push(point(
            Span::Begin,
            Arrow::HostileTarget,
            Subject::Ability {
                id: u32::from(index) + 1,
            },
            subject(48 + index),
        ));
        acts.push(Act::Mark {
            span: Span::Begin,
            mark: Mark::Eligible,
            on: subject(53 + index),
        });
    }
    acts.push(Act::Lift {
        span: Span::Begin,
        handling: CardHandling::Library {
            side: Side::Opponent,
            count: Some(3),
            scry: None,
        },
    });
    acts
}

/// The stack opens with both kinds of item already in place. The next view opens their
/// arrows together, and the last one closes the arrow of the ability that resolves.
fn stack() -> Stress {
    let mut objects: Vec<_> = (0..50).map(permanent).collect();
    let mut items = Vec::new();
    let mut arrows = Vec::new();
    for index in 0..25 {
        let target = |offset| {
            vec![TargetRef::Object {
                slot: ObjectId(index * 2 + offset),
            }]
        };
        let spell = card(50 + index, ZoneName::Stack, Side::You);
        items.push(StackItem::Spell {
            slot: spell.slot,
            targets: target(1),
            x: 0,
            paid: Vec::new(),
        });
        objects.push(spell);
        items.push(ability(&objects[usize::from(index * 2)], target(2)));
        arrows.push(point(
            Span::Begin,
            Arrow::FriendlyTarget,
            subject(50 + index),
            subject(index * 2),
        ));
        arrows.push(point(
            Span::Begin,
            Arrow::FriendlyTarget,
            Subject::Ability {
                id: u32::from(index) + 1,
            },
            subject(index * 2 + 1),
        ));
    }
    let first = start(
        objects,
        items,
        Reset {
            abilities: (1..=25).collect(),
        },
    );
    let table = replay(Table::of(&first), first.script.as_ref().expect("reset"))
        .expect("initial stack numbers its abilities");
    let (pointing, table) = next(&first, &table, arrows);
    let top = Subject::Ability { id: 25 };
    let (resolved, _) = next(
        &pointing,
        &table,
        vec![
            point(Span::End, Arrow::FriendlyTarget, top, subject(49)),
            Act::Vanish {
                subject: top,
                from: ZoneName::Stack,
                why: Why::Resolve,
                object: None,
                at: None,
            },
        ],
    );
    recording(vec![first, pointing, resolved])
}

/// Number of frames the full stack rests with all of its arrows open.
pub(crate) const STACK_REST_FRAMES: usize = 120;

/// A four-second round trip along the inner edge of the fan in its resting frame.
/// The position follows the curvature of the edge across window shapes.
pub(crate) fn pointer_moves(
    frame: &beryl_scene::frame::Frame,
) -> Vec<(u64, beryl_scene::input::Pointer)> {
    use beryl_scene::driver::TICK;
    use beryl_scene::frame::{Place, Role};
    use beryl_scene::input::{Button, Modifiers, Pointer, PointerKind, PointerPhase};

    const HALF: u16 = 120;
    let points: Vec<_> = frame
        .resolved
        .iter()
        .filter(|element| {
            element.role == Role::Card
                && matches!(
                    element.place,
                    Place::Card {
                        zone: ZoneName::Hand,
                        side: Side::You,
                        ..
                    }
                )
        })
        .map(|element| {
            let [top_left, top_right, _, bottom_left] = element.corners;
            top_left.lerp(top_right, 0.09) + 0.08 * (bottom_left - top_left)
        })
        .collect();
    assert_eq!(points.len(), 60);
    let gaps = u16::try_from(points.len() - 1).expect("60 points");
    (1..=2 * HALF)
        .map(|step| {
            let (distance, length) = if step <= HALF {
                (step - 1, HALF - 1)
            } else {
                (2 * HALF - step, HALF)
            };
            let along = distance * gaps;
            let index = usize::from(along / length);
            let position = points.get(index + 1).map_or(points[index], |next| {
                points[index].lerp(*next, f32::from(along % length) / f32::from(length))
            });
            (
                u64::from(step) * TICK.0,
                Pointer {
                    id: 0,
                    kind: PointerKind::Mouse,
                    phase: PointerPhase::Move,
                    position,
                    button: Button::None,
                    #[expect(
                        clippy::default_trait_access,
                        reason = "the vector type comes from the frame"
                    )]
                    wheel: Default::default(),
                    modifiers: Modifiers::default(),
                },
            )
        })
        .collect()
}

/// Gives the mixed stress boards their own placeholder faces. The object-limit boards
/// retain their existing load with just the common card back.
pub(crate) fn card_records(name: &str, board: &Stress) -> CardRecords {
    let mut records = CardRecords::none();
    if matches!(name, "stress-255" | "stress-1024") {
        return records;
    }
    for object in board.views.iter().flat_map(|view| &view.objects) {
        let Some(name) = &object.name else { continue };
        let entries = if object.token {
            &mut records.tokens
        } else {
            &mut records.cards
        };
        entries.entry(name.clone()).or_insert_with(|| CardEntry {
            name: name.clone(),
            scryfall_id: String::new(),
            set: String::new(),
            collector_number: String::new(),
            layout: "normal".to_owned(),
            artist: String::new(),
            faces: vec![CardFace {
                name: name.clone(),
                mana_cost: None,
                type_line: String::new(),
                oracle_text: None,
                power: None,
                toughness: None,
                loyalty: None,
                colors: Vec::new(),
                images: CardImages {
                    art_crop: String::new(),
                    normal: String::new(),
                    large: format!("stress-{}", object.slot.0),
                },
            }],
        });
    }
    records
}
