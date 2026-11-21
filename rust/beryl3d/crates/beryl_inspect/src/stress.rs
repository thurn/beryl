//! Stress boards: positions at the engine's capacity, built by hand with no engine, that the
//! frame-time gates draw. The engine knows only a few dozen cards, so a board made by playing
//! it would repeat a handful of names, stack into piles and cost less to draw than a real one.
//! Here every permanent has a name of its own. Nothing reads a clock or a random number, so
//! two calls return equal values.

use beryl_protocol::card_records::{CardEntry, CardFace, CardImages, CardRecords};
use beryl_protocol::protocol::{
    CardHandling, Facing, ObjectId, ObjectView, PlayerState, PowerToughness, Side, StackItem,
    TargetRef, Why, ZoneName,
};
use beryl_protocol::script::{
    Act, Arrow, Curve, Fact, Mark, Notice, Reset, Span, Step, StepTiming, Stream, Subject, Table,
    Tally, replay, time_script,
};
use beryl_protocol::testing::{self, card, permanent, step, timing};
use beryl_protocol::{PlayerView, performance_probe};
use kit_probe::agent::Rule;

/// A synthetic board and any scripted changes to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Stress {
    /// The first view is a reset with every object at rest. Each later view carries a script.
    pub views: Vec<PlayerView>,
    /// For each view after the first, one timing per step of its script, in script order:
    /// `timings[i]` belongs to `views[i + 1]`, whose script carries the same timings as a
    /// session's does.
    pub timings: Vec<Vec<StepTiming>>,
    /// What a page or a playback does besides handing over the views.
    pub drive: Drive,
    /// The rules the interface checks do not hold this board to.
    pub unjudged: &'static [Rule],
}

/// What a page or a playback of a stress board does besides handing over its views.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drive {
    /// Moves the pointer over the viewer's hand, as `pointer_moves` lays out.
    pub pointer_over_hand: bool,
    /// Waits this many frames, once the table is at rest, before handing over the last view.
    pub rest_before_last: Option<usize>,
}

/// A stress board: the one place that maps a board's name to the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Board {
    /// 255 permanents.
    S255,
    /// 1024 permanents.
    S1024,
    /// Every verb at once.
    Verbs,
    /// Sixty cards in hand under a moving pointer.
    Hand,
    /// A full stack of spells and abilities.
    Stack,
}

impl Board {
    /// Every stress board, in the order they are listed and played.
    pub const ALL: [Self; 5] = [
        Self::S255,
        Self::S1024,
        Self::Verbs,
        Self::Hand,
        Self::Stack,
    ];

    /// The board's recording name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::S255 => "stress-255",
            Self::S1024 => "stress-1024",
            Self::Verbs => "stress-verbs",
            Self::Hand => "stress-hand",
            Self::Stack => "stress-stack",
        }
    }

    /// The board called `name`, if there is one.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|board| board.name() == name)
    }

    /// How a page or a playback drives the board.
    #[must_use]
    pub const fn drive(self) -> Drive {
        match self {
            Self::Hand => Drive {
                pointer_over_hand: true,
                rest_before_last: None,
            },
            Self::Stack => Drive {
                pointer_over_hand: false,
                rest_before_last: Some(STACK_REST_FRAMES),
            },
            Self::S255 | Self::S1024 | Self::Verbs => Drive {
                pointer_over_hand: false,
                rest_before_last: None,
            },
        }
    }

    /// The rules the interface checks do not hold the board to. `Verbs` starts every verb's acts
    /// on 100 objects in one beat with no stagger, so that the frame gates draw every verb in
    /// the same frames. No game plays a beat like that, and its steps crowd and cover each other
    /// on purpose, so "not crowded" and "every step shown" do not judge it (docs/beryl-3d.md,
    /// "What the checks run over"). Every other check still runs on it.
    #[must_use]
    pub const fn unjudged(self) -> &'static [Rule] {
        match self {
            Self::Verbs => &[Rule::Crowded, Rule::StepShown],
            _ => &[],
        }
    }

    /// The board's views and timings.
    #[must_use]
    pub fn build(self) -> Stress {
        let mut stress = match self {
            Self::S255 => board(255),
            Self::S1024 => board(1024),
            Self::Verbs => verbs(),
            Self::Hand => hand(),
            Self::Stack => stack(),
        };
        stress.drive = self.drive();
        stress.unjudged = self.unjudged();
        stress
    }
}

/// Whether the interface checks hold the recording `recording` to `rule`.
#[must_use]
pub fn audited(recording: &str, rule: Rule) -> bool {
    Board::parse(recording).is_none_or(|board| !board.unjudged().contains(&rule))
}

/// The synthetic recording called `name`, if there is one.
#[must_use]
pub fn stress(name: &str) -> Option<Stress> {
    Board::parse(name).map(Board::build)
}

/// How many permanents move from the graphics probe's board to the end of the battlefield.
const FLIGHTS: usize = 40;
const FLIGHT_DURATION: f64 = 420.0;
const FLIGHT_STAGGER: f64 = 30.0;

/// A battlefield of `count` permanents, then the same one after 40 of them have moved to its
/// end. The first view is the board the app draws to choose its graphics tier.
fn board(count: u16) -> Stress {
    let first = performance_probe(count);
    let mut second = first.clone();
    let moved = second.objects[..FLIGHTS].to_vec();
    second.battlefield.rotate_left(FLIGHTS);
    second.objects.rotate_left(FLIGHTS);
    let end = count - 1;
    let beat: Vec<Step> = moved
        .into_iter()
        .map(|object| {
            step(Act::Fly {
                from: ZoneName::Battlefield,
                why: Why::Effect,
                object,
                at: Some(end),
                item: None,
                origin: None,
            })
        })
        .collect();
    let flight = StepTiming {
        stagger: FLIGHT_STAGGER,
        curve: Curve::EaseInOut,
        ..timing(FLIGHT_DURATION)
    };
    let timings = vec![flight; beat.len()];
    second.script = Some(testing::script(None, vec![beat], timings.clone()));

    Stress {
        views: vec![first, second],
        timings: vec![timings],
        drive: Drive::default(),
        unjudged: &[],
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
    Stress {
        views,
        timings,
        drive: Drive::default(),
        unjudged: &[],
    }
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
        scry: Some(true),
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
            scry: Some(true),
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

/// A four-second round trip along the inner edge of the fan in its resting frame, card by card
/// in hand order through the cards in view, so through each row of a hand laid in several. The position follows the curvature
/// of the edge across window shapes.
pub(crate) fn pointer_moves(frame: &kit_frame::frame::Frame) -> Vec<(u64, kit_ui::input::Pointer)> {
    use app_client::driver::TICK;
    use kit_frame::frame::{Place, Role};
    use kit_ui::input::{Button, Modifiers, Pointer, PointerKind, PointerPhase};

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
            // The card's top edge on screen is the edge of its two highest corners, and its
            // strip to press runs down from that edge's left end, in each row of the hand.
            let mut corners = element.corners;
            corners.sort_by(|a, b| a.y.total_cmp(&b.y));
            let [a, b, c, d] = corners;
            let (top_left, top_right) = if a.x < b.x { (a, b) } else { (b, a) };
            let bottom_left = if c.x < d.x { c } else { d };
            top_left.lerp(top_right, 0.09) + 0.08 * (bottom_left - top_left)
        })
        .collect();
    // A phone shows as many of the 60 cards as keep a strip to press, and scrolls the rest.
    assert!(points.len() > 1, "the hand shows its cards");
    // A point, or the nearest point below it where a hit test finds a card of the viewer's
    // hand: a step stop beside the viewer's life orb covers the top of a few cards of a hand
    // this large, and the pointer crosses the hand, not the stops.
    let in_hand = |index: u32| {
        let element = &frame.resolved[index as usize];
        element.role == Role::Card
            && matches!(
                element.place,
                Place::Card {
                    zone: ZoneName::Hand,
                    side: Side::You,
                    ..
                }
            )
    };
    let onto_hand = |point| {
        let mut below = point;
        for _ in 0..40 {
            if kit_scene::hit::hit(frame, below).is_some_and(in_hand) {
                return below;
            }
            below.y += 2.0;
        }
        point
    };
    let gaps = u16::try_from(points.len() - 1).expect("at most 60 points");
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
            let position = onto_hand(position);
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

/// Gives every stress board's cards placeholder faces of their own, one per name, so that no
/// card is drawn face up with no face.
pub(crate) fn card_records(board: &Stress) -> CardRecords {
    placeholder_records(&board.views)
}

/// Placeholder faces for the cards of `views`, one per name.
pub(crate) fn placeholder_records(views: &[PlayerView]) -> CardRecords {
    let mut records = CardRecords::none();
    for object in views.iter().flat_map(|view| &view.objects) {
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
