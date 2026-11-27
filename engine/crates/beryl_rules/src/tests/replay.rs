//! Transactional replay (architecture §7.3, §17.4): a game whose bodies are answered inline
//! and the same game replayed through `Replay` reach the same state at every decision.
//!
//! A replacement effect's choice is answered the same two ways, in a resolution and in the
//! draw step, where the end of the step pauses ([`Pending::StepActions`]); so is a value a
//! permanent chooses as it enters.

use beryl_card::effect::build::{cards_in, draw, end_turn, gain_life, scry, sequence, you};
use beryl_card::{
    Amount, CardDef, CastFace, Cost, Effect, Face, Filter, ManaColor, ManaCost, ManaSet, Players,
    Prompt, Purpose, Replacement, Says, SpecIx, TargetKind, TargetSpec, Trigger, Types,
};

use super::cast::*;
use super::events::find;
use super::{DUAL, LAND, fingerprint};
use crate::bitboard::Bitboard;
use crate::card::{Ability, Flow};
use crate::game::{self, Game};
use crate::ids::{Seat, Slot, SlotId};
use crate::interpret::choices;
use crate::replacement::event::{Cause, Event, StackIx};
use crate::resolution::Resolution;
use crate::rng::SplitMix64;
use crate::state::{Pending, TurnStep, Zone};
use crate::zone::Place;
use crate::{
    Answers, CardTable, Choice, CustomBody, Decision, DecisionKind, Decklist, GameTable, Layout,
    Observation, Opt, Outcome, Payment, Profile, Replay, Scratch, State, Step, apply, custom_body,
    next,
};
use beryl_kernel::format::Format;

/// A 1/1 with dredge 2, to die or be discarded into a graveyard and offer to replace draws
/// from there.
static DREDGER: CardDef = CardDef {
    name: "Test Dredger",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: ManaCost::new(1, [0; 6]),
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::replacing(&Replacement::dredge(2))],
        ..Face::EMPTY
    }],
};

/// The test cards and the dredger.
static DREDGE_CARDS: CardTable = CardTable::new(&[
    &LAND, &ISLAND, &DUAL, &CRYPT, &TREASURE, &STRIP, &HQ, &BEAR, &REMOVAL, &TITHE, &LIES, &PONDER,
    &FLASHER, &DIVER, &TRAINER, &OVERLORD, &STUPOR, &RAVELER, &EMPEROR, &FLASHBACK, &BINDER,
    &LOOTER, &STINGER, &SNAPBACK, &BOUNCE, &ELF, &SAGE, &RUMMAGE, &DREDGER,
]);

/// Lands, cards that discard and kill, and four dredgers.
const DREDGE_DECK: &Decklist = &[
    (10, "Test Land"),
    (6, "Test Island"),
    (2, "Test Dual"),
    (4, "Test Dredger"),
    (2, "Test Bear"),
    (2, "Test Removal"),
    (2, "Test Lies"),
    (2, "Test Ponder"),
    (1, "Test Looter"),
    (1, "Test Rummage"),
    (1, "Test Stinger"),
    (1, "Test Sage"),
];

const CAST_A_SORCERY: Effect = Effect::MayCast {
    player: you(),
    card: cards_in(Zone::Hand, &you(), Filter::Types(Types::SORCERY)),
    free: false,
    free_below: None,
    copy: false,
    face: CastFace::Up,
    within_x: false,
};

/// "You may cast a sorcery card from your hand. Scry 1."
static CAST_THEN_SCRY: CardDef = CardDef {
    name: "Test Cast and Scry",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        types: Types::INSTANT,
        abilities: &[Ability::spell().effect(&sequence(&[CAST_A_SORCERY, scry(you(), 1)]))],
        ..Face::EMPTY
    }],
};

static CAST_THEN_SCRY_REPEATEDLY: CardDef = CardDef {
    name: "Test Cast and Scry Repeatedly",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        types: Types::INSTANT,
        abilities: &[Ability::spell().effect(&sequence(&[
            CAST_A_SORCERY,
            sequence(&[scry(you(), 1); 33]),
        ]))],
        ..Face::EMPTY
    }],
};

/// A spell with X and a target, and a trigger the cast leaves waiting.
static TARGETED_X: CardDef = CardDef {
    name: "Test Targeted X",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: ManaCost {
            x: 1,
            ..ManaCost::new(0, [1, 0, 0, 0, 0, 0])
        },
        types: Types::SORCERY,
        abilities: &[
            Ability::spell()
                .target(&TargetSpec::one(TargetKind::Player, Filter::Any))
                .effect(&Effect::LoseLife {
                    player: Players::Target(SpecIx::new(0)),
                    amount: Amount::X,
                }),
            Ability::triggered(&Trigger::CAST)
                .on_the_stack()
                .effect(&gain_life(you(), 1)),
        ],
        ..Face::EMPTY
    }],
};

static CAST_AND_SCRY_CARDS: CardTable =
    CardTable::new(&[&LAND, &DUAL, &CAST_THEN_SCRY, &TARGETED_X, &BINDER]);
const CAST_AND_SCRY_DECK: &Decklist = &[
    (16, "Test Land"),
    (4, "Test Dual"),
    (9, "Test Cast and Scry"),
    (9, "Test Targeted X"),
    (2, "Test Binder"),
];

/// "You may gain 1 life. Add {W}." The mana produced is the same either way.
struct ManaWithChoice;

impl CustomBody for ManaWithChoice {
    const SAYS: Says = Says {
        asks: true,
        changes_turn_structure: false,
        starts_subgame: false,
        takes: &[],
    };

    fn resolve<L: Layout>(game: &mut Game<'_, L>, resolution: &mut Resolution<'_, L>) -> Flow {
        let player = resolution.controller();
        let source = resolution.source();
        if choices::yes_no(
            game,
            resolution,
            player,
            Prompt::new(Purpose::May),
            Bitboard::EMPTY,
        )? {
            game::life::gain(game, player, 1)?;
        }
        let added = game
            .state
            .add_mana(player, Some(source), ManaColor::White, 1)
            .unwrap();
        game.sink.note_from(Some(source), added);
        Ok(())
    }
}

static CHOICE_MANA: CardDef = CardDef {
    name: "Test Choice Mana",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::mana(Cost::TAP, ManaSet::one(ManaColor::White))
            .custom(custom_body::<ManaWithChoice>())],
        ..Face::EMPTY
    }],
};
static CHOICE_MANA_CARDS: CardTable =
    CardTable::new(&[&LAND, &CAST_THEN_SCRY, &TARGETED_X, &CHOICE_MANA]);
const CHOICE_MANA_DECK: &Decklist = &[
    (20, "Test Land"),
    (1, "Test Choice Mana"),
    (1, "Test Cast and Scry"),
    (1, "Test Targeted X"),
];

/// The cards and the deck both players play.
type Decks = (&'static CardTable, &'static Decklist);

/// Answers every question at once, at random, as a rollout policy would, and logs its answers.
struct Inline {
    rng: SplitMix64,
    log: Vec<u16>,
}

impl Answers for Inline {
    fn inline(&self) -> bool {
        true
    }

    fn answer(&mut self, _obs: Observation<'_>, d: &Decision) -> Option<Choice> {
        let c = self.rng.below(d.len() as u64) as u16;
        self.log.push(c);
        Some(Choice(c))
    }
}

/// What the inline game did, in order.
#[derive(Debug, PartialEq, Eq)]
enum Logged {
    /// A decision `next` returned: the state's hash, and the answer.
    Decision { hash: u64, choice: u16 },
    /// A question a body asked, answered inline.
    Inline(u16),
}

/// A random game of `decks` with inline answers, under `profile`.
fn inline_game(decks: Decks, seed: u64, profile: Profile) -> (Vec<Logged>, Outcome) {
    let t = GameTable::new(Format::CONSTRUCTED, decks.0, [decks.1, decks.1]).unwrap();
    let mut s = State::new(&t, seed, profile).unwrap();
    let mut x = Scratch::new();
    let mut answers = Inline {
        rng: SplitMix64::new(seed ^ 0xA5A5),
        log: Vec::new(),
    };
    let mut agent = SplitMix64::new(seed);
    let mut logged = Vec::new();
    let drain = |answers: &mut Inline, logged: &mut Vec<Logged>| {
        logged.extend(answers.log.drain(..).map(Logged::Inline));
    };
    let outcome = loop {
        let step = next(&mut s, &t, &mut x, &mut answers);
        drain(&mut answers, &mut logged);
        match step {
            Step::Over(outcome) => break outcome,
            Step::Decide(d) => {
                let choice = agent.below(d.len() as u64) as u16;
                logged.push(Logged::Decision {
                    hash: fingerprint(&s),
                    choice,
                });
                apply(&mut s, &t, &mut x, &mut answers, Choice(choice));
                drain(&mut answers, &mut logged);
            }
        }
    };
    (logged, outcome)
}

/// How many questions a replay paused at: in a resolution, and as a step ended.
#[derive(Debug, Default)]
struct Paused {
    resolving: usize,
    step_actions: usize,
    after_cast: usize,
}

/// Replays an inline game's answers through `Replay`, checking the state hash at each of
/// its decisions. Returns how many questions paused.
fn replay(
    decks: Decks,
    seed: u64,
    profile: Profile,
    logged: &[Logged],
    outcome: Outcome,
) -> Paused {
    let t = GameTable::new(Format::CONSTRUCTED, decks.0, [decks.1, decks.1]).unwrap();
    let mut s = State::new(&t, seed, profile).unwrap();
    let mut x = Scratch::new();
    let mut log = logged.iter();
    let mut paused = Paused::default();
    loop {
        match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Over(o) => {
                assert_eq!(o, outcome, "seed {seed}");
                assert_eq!(log.next(), None, "seed {seed}: answers left over");
                return paused;
            }
            Step::Decide(d) => {
                if matches!(s.pending(), Pending::Resolving(_)) && d.prompt.purpose == Purpose::Scry
                {
                    let shown = crate::view(&s, &t, &mut x);
                    if shown.stack().last().is_some_and(|entry| {
                        t.def(entry.card()).name == TARGETED_X.name
                            && entry.x > 0
                            && !entry.targets.is_empty()
                    }) {
                        paused.after_cast += 1;
                    }
                }
                let pause = match s.pending() {
                    Pending::Resolving(_) => Some(&mut paused.resolving),
                    Pending::StepActions(_) => Some(&mut paused.step_actions),
                    _ => None,
                };
                let choice = if let Some(count) = pause {
                    *count += 1;
                    match log.next() {
                        Some(&Logged::Inline(c)) => c,
                        other => panic!("seed {seed}: paused where the inline game had {other:?}"),
                    }
                } else {
                    match log.next() {
                        Some(&Logged::Decision { hash, choice }) => {
                            assert_eq!(fingerprint(&s), hash, "seed {seed}: the states diverged");
                            choice
                        }
                        other => {
                            panic!("seed {seed}: a decision where the inline game had {other:?}")
                        }
                    }
                };
                apply(&mut s, &t, &mut x, &mut Replay, Choice(choice));
            }
        }
    }
}

#[test]
fn inline_and_replayed_answers_reach_the_same_state_at_every_decision() {
    let mut paused = 0;
    let mut finished = 0;
    let greedy = Profile {
        payment: Payment::Greedy,
        ..Profile::SMART
    };
    let manual = Profile {
        payment: Payment::Manual,
        ..Profile::EXACT
    };
    let decks = (&CAST_CARDS, EVERYTHING);
    for seed in 0..3 {
        for profile in [Profile::EXACT, Profile::SMART, greedy, manual] {
            let (logged, outcome) = inline_game(decks, seed, profile);
            assert!(
                !matches!(outcome, Outcome::Stalled(_)),
                "seed {seed}: {outcome:?}"
            );
            finished += 1;
            paused += replay(decks, seed, profile, &logged, outcome).resolving;
        }
    }
    assert_eq!(finished, 12);
    assert!(
        paused > 20,
        "only {paused} paused questions: the test exercises too little"
    );
}

/// Inline and replayed answers give identical positions after a paid cast with a target
/// and a nonzero X, followed by a question in the enclosing resolution.
#[test]
fn a_cast_inside_a_resolution_replays_with_its_later_question() {
    let decks = (&CAST_AND_SCRY_CARDS, CAST_AND_SCRY_DECK);
    let mut after_cast = 0;
    for seed in 0..4 {
        for profile in [
            Profile::EXACT,
            Profile::SMART,
            Profile {
                payment: Payment::Manual,
                ..Profile::EXACT
            },
        ] {
            let (logged, outcome) = inline_game(decks, seed, profile);
            assert!(
                !matches!(outcome, Outcome::Stalled(_)),
                "seed {seed}: {outcome:?}"
            );
            after_cast += replay(decks, seed, profile, &logged, outcome).after_cast;
        }
    }
    assert!(
        after_cast > 0,
        "no replay exercised a cast with nonzero X before scrying"
    );
}

static DRAW_WATCHER: CardDef = CardDef {
    name: "Test Draw Watcher",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::triggered(&Trigger::draws(Players::You)).effect(&gain_life(you(), 1))],
        ..Face::EMPTY
    }],
};

/// The draw triggers the watcher. The last instruction must be skipped when the turn ends.
static SCRY_THEN_END_TURN: CardDef = CardDef {
    name: "Test Scry and End Turn",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        types: Types::INSTANT,
        abilities: &[Ability::spell().effect(&sequence(&[
            draw(you(), 1),
            scry(you(), 1),
            end_turn(),
            gain_life(you(), 10),
        ]))],
        ..Face::EMPTY
    }],
};

#[test]
fn ending_the_turn_after_a_question_replays_to_the_same_cleanup() {
    static CARDS: CardTable =
        CardTable::new(&[&LAND, &CAST_THEN_SCRY, &SCRY_THEN_END_TURN, &DRAW_WATCHER]);
    const DECK: &Decklist = &[
        (20, "Test Land"),
        (1, "Test Cast and Scry"),
        (1, "Test Scry and End Turn"),
        (1, "Test Draw Watcher"),
    ];
    let mut g = super::cast::Game::with(&CARDS, DECK, Profile::EXACT);
    g.x = Scratch::recording();
    let below = g.give(g.me, CAST_THEN_SCRY.name, Zone::Hand);
    let ends = g.give(g.them, SCRY_THEN_END_TURN.name, Zone::Hand);
    g.give(g.them, DRAW_WATCHER.name, Zone::Battlefield);
    g.pick(cast(below));
    g.pick(cast(ends));
    g.choose(Choice(0));
    assert_eq!(g.s.turn().priority(), g.me);
    let mut inline = g.s;
    let mut inline_scratch = Scratch::recording();
    let mut answer = Inline {
        rng: SplitMix64::new(1),
        log: Vec::new(),
    };
    apply(
        &mut inline,
        &g.t,
        &mut inline_scratch,
        &mut answer,
        Choice(0),
    );
    assert_eq!(answer.log.len(), 1, "the inline source answered scry");

    g.choose(Choice(0));
    assert!(g.is_paused());
    assert_eq!(g.current().prompt.purpose, Purpose::Scry);
    assert_eq!(g.s.object(ends).zone(), Zone::Stack);
    assert_eq!(g.s.stack().len(), 2);
    assert_eq!(crate::view(&g.s, &g.t, &mut g.x).triggers().len(), 1);
    // next counts the question that appeared during the replay.
    let _ = g.decide();
    g.choose(Choice(answer.log[0]));
    assert_eq!(g.s.turn().step(), TurnStep::Cleanup);
    assert_eq!(g.s.object(below).zone(), Zone::Exile);
    assert_eq!(g.s.object(ends).zone(), Zone::Exile);
    assert_eq!(g.s.player(g.them).life(), 20);
    assert!(g.s.stack().is_empty());
    assert!(
        g.s.triggers().is_empty(),
        "the draw trigger ceased to exist"
    );
    assert_eq!(fingerprint(&g.s), fingerprint(&inline));

    let events: Vec<_> = g.x.events.drain().collect();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::StackDeparture { .. }))
            .count(),
        2
    );
    assert!(events.iter().any(|e| matches!(
        e,
        Event::StepBegins {
            step: TurnStep::Cleanup,
            ..
        }
    )));
}

#[test]
fn a_cast_offer_includes_each_castable_card_and_can_be_declined() {
    let mut g = super::cast::Game::with(&CAST_AND_SCRY_CARDS, CAST_AND_SCRY_DECK, Profile::EXACT);
    let outer = g.give(g.me, CAST_THEN_SCRY.name, Zone::Hand);
    let variable = g.give(g.me, TARGETED_X.name, Zone::Hand);
    let binder = g.give(g.me, BINDER.name, Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.pick(cast(outer));
    g.pass_both();
    let offer = g.current();
    assert_eq!(offer.kind, DecisionKind::CastOffer);
    assert_eq!(offer.len(), 3);
    assert_eq!(offer.candidates.option(Choice(0)), Some(Opt::Pass));
    assert!(index_of(&offer, cast(variable)).is_some());
    assert!(index_of(&offer, cast(binder)).is_some());

    g.pick(Opt::Pass);
    assert_eq!(g.current().prompt.purpose, Purpose::Scry);
    g.pick(Opt::Done);
    assert_eq!(g.s.object(variable).zone(), Zone::Hand);
    assert_eq!(g.s.object(binder).zone(), Zone::Hand);
    assert!(g.s.stack().is_empty());
}

#[test]
fn payment_and_cast_triggers_commit_when_the_enclosing_resolution_finishes() {
    let mut g = super::cast::Game::with(&CHOICE_MANA_CARDS, CHOICE_MANA_DECK, Profile::EXACT);
    g.x = Scratch::recording();
    let outer = g.give(g.me, CAST_THEN_SCRY.name, Zone::Hand);
    let child = g.give(g.me, TARGETED_X.name, Zone::Hand);
    let land = g.give(g.me, CHOICE_MANA.name, Zone::Battlefield);
    g.pick(cast(outer));
    g.pass_both();

    assert_eq!(g.current().kind, DecisionKind::CastOffer);
    g.pick(cast(child));
    assert_eq!(g.current().kind, DecisionKind::Target);
    g.pick(Opt::Player(g.them));
    assert_eq!(g.current().kind, DecisionKind::YesNo);
    assert!(!g.s.object(land).is_tapped());
    assert_eq!(g.s.object(child).zone(), Zone::Hand);
    g.pick(Opt::Yes);

    assert_eq!(g.current().prompt.purpose, Purpose::Scry);
    assert!(g.is_paused());
    assert_eq!(g.s.player(g.me).life(), 20);
    assert!(g.s.triggers().is_empty());
    assert_eq!(g.s.stack().len(), 1);
    assert_eq!(g.s.stack()[0].object().unwrap().slot, outer);
    assert!(
        !g.x.events
            .drain()
            .any(|e| matches!(e, Event::Cast { spell, .. } if spell.slot == child))
    );
    let shown = crate::view(&g.s, &g.t, &mut g.x);
    assert_eq!(shown.player(g.me).life(), 21);
    assert!(shown.object(land).is_tapped());
    assert_eq!(shown.stack()[0].object().unwrap().slot, child);
    assert_eq!(
        shown.triggers().len(),
        1,
        "the trigger waits while the body scries"
    );

    g.pick(Opt::Done);
    assert!(!g.is_paused());
    assert_eq!(g.s.object(outer).zone(), Zone::Graveyard);
    assert_eq!(g.s.object(child).zone(), Zone::Stack);
    assert_eq!(g.s.player(g.me).life(), 21);
    assert!(g.s.object(land).is_tapped());
    let cast_events: Vec<_> =
        g.x.events
            .drain_tagged()
            .filter(|tag| matches!(tag.event, Event::Cast { spell, .. } if spell.slot == child))
            .collect();
    assert_eq!(cast_events.len(), 1);
    assert_eq!(
        cast_events[0].tag.cause,
        Cause::Player {
            player: g.me,
            entry: Some(StackIx::at(1)),
        }
    );
}

/// Cast triggers from a spell cast mid-resolution wait until the enclosing resolution finishes,
/// then go on the stack in APNAP order, and the spell is counted once in `Count::SpellsCast`
/// even when the resolution replays across a later pause.
#[test]
fn mid_resolution_cast_triggers_stack_in_apnap_order_and_spell_is_counted_once() {
    static CAST_WATCHER: CardDef = CardDef {
        name: "Test Cast Watcher",
        shape: beryl_card::definition::Shape::Single,
        tokens: &[],
        faces: &[Face {
            types: Types::ARTIFACT,
            abilities: &[Ability::triggered(&Trigger::casts(
                Players::EachPlayer,
                Filter::Types(Types::SORCERY),
            ))
            .effect(&Effect::GainLife {
                player: you(),
                amount: Amount::SpellsCast(Players::EachPlayer),
            })],
            ..Face::EMPTY
        }],
    };
    static CARDS: CardTable = CardTable::new(&[&LAND, &CAST_THEN_SCRY, &TARGETED_X, &CAST_WATCHER]);
    const DECK: &Decklist = &[
        (20, "Test Land"),
        (1, "Test Cast and Scry"),
        (1, "Test Targeted X"),
        (1, "Test Cast Watcher"),
    ];
    let mut g = super::cast::Game::with(&CARDS, DECK, Profile::EXACT);
    let outer = g.give(g.me, CAST_THEN_SCRY.name, Zone::Hand);
    let child = g.give(g.me, TARGETED_X.name, Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.them, CAST_WATCHER.name, Zone::Battlefield);
    g.pick(cast(outer));
    g.pass_both();

    g.pick(cast(child));
    g.pick(Opt::Player(g.them));
    // Paused at Scry after the cast: committed state has not yet counted the child cast or
    // placed any trigger on the stack.
    assert_eq!(g.current().prompt.purpose, Purpose::Scry);
    assert_eq!(g.s.counted(crate::state::Count::SpellsCast, g.me), 1);
    assert!(g.s.triggers().is_empty());

    g.pick(Opt::Done);
    // Resolution finished: both spells counted once (outer + child = 2), and triggers went on
    // the stack in APNAP order (active player's TARGETED_X cast trigger below non-active
    // player's CAST_WATCHER trigger, so CAST_WATCHER resolves first).
    assert_eq!(g.s.counted(crate::state::Count::SpellsCast, g.me), 2);
    assert_eq!(g.s.stack().len(), 3);
    assert_eq!(g.s.stack()[0].controller(), g.me);
    assert_eq!(g.t.def(g.s.stack()[0].card()).name, TARGETED_X.name);
    assert_eq!(g.s.stack()[1].controller(), g.me);
    assert_eq!(g.t.def(g.s.stack()[1].card()).name, TARGETED_X.name);
    assert_eq!(g.s.stack()[2].controller(), g.them);
    assert_eq!(g.t.def(g.s.stack()[2].card()).name, CAST_WATCHER.name);
    // Non-active player's trigger resolves first (+2 life), then active player's (+1 life).
    g.pass_both();
    assert_eq!(g.s.player(g.them).life(), 22);
    assert_eq!(g.s.player(g.me).life(), 20);
    g.pass_both();
    assert_eq!(g.s.player(g.me).life(), 21);
}

/// The cast stays speculative when the answer record fills. The same position widens
/// and finishes with the cast and its payment made once.
#[test]
fn a_cast_waits_for_the_resolution_that_fills_the_small_answer_record() {
    static CARDS: CardTable = CardTable::new(&[&LAND, &CAST_THEN_SCRY_REPEATEDLY, &TARGETED_X]);
    const DECK: &Decklist = &[
        (20, "Test Land"),
        (1, "Test Cast and Scry Repeatedly"),
        (1, "Test Targeted X"),
    ];
    let mut g = super::cast::Game::with(&CARDS, DECK, Profile::EXACT);
    g.x = Scratch::recording();
    let outer = g.give(g.me, CAST_THEN_SCRY_REPEATEDLY.name, Zone::Hand);
    let child = g.give(g.me, TARGETED_X.name, Zone::Hand);
    let land = g.give(g.me, "Test Land", Zone::Battlefield);
    g.pick(cast(outer));
    g.pass_both();
    g.pick(cast(child));
    g.pick(Opt::Player(g.them));
    // The offer and target used two entries, and 30 scries fill the rest of the record.
    for _ in 0..30 {
        g.pick(Opt::Done);
    }
    assert!(matches!(g.s.pending(), Pending::Resolving(r) if r.record.len() == 32));
    let mut large = g.s.widen();
    g.pick(Opt::Done);
    assert_eq!(
        g.s.outcome(),
        Some(Outcome::Stalled(crate::StallReason::Capacity))
    );
    assert_eq!(g.s.object(child).zone(), Zone::Hand);
    assert!(!g.s.object(land).is_tapped());
    assert!(
        !g.x.events
            .drain()
            .any(|e| matches!(e, Event::Cast { spell, .. } if spell.slot == child))
    );

    let mut scratch = Scratch::recording();
    for _ in 0..3 {
        let Step::Decide(d) = next(&mut large, &g.t, &mut scratch, &mut Replay) else {
            panic!("the widened game offers the remaining scries");
        };
        assert_eq!(d.prompt.purpose, Purpose::Scry);
        assert_eq!(d.candidates.option(Choice(0)), Some(Opt::Done));
        apply(&mut large, &g.t, &mut scratch, &mut Replay, Choice(0));
    }
    assert_eq!(large.stack().len(), 1);
    assert_eq!(g.t.def(large.stack()[0].card()).name, TARGETED_X.name);
    assert_eq!(large.triggers().len(), 1);
    assert_eq!(
        scratch
            .events
            .drain()
            .filter(|e| matches!(e, Event::Cast { .. }))
            .count(),
        1
    );
}

/// Dredge's choice, answered inline as a rollout would and replayed through `Replay`, reaches
/// the same state: in the draw step, where the end of the step pauses, and in a resolution
/// that draws.
#[test]
fn replacement_choices_replay_to_the_same_state_in_a_step_and_a_resolution() {
    let decks = (&DREDGE_CARDS, DREDGE_DECK);
    let mut paused = Paused::default();
    for seed in 0..4 {
        for profile in [Profile::EXACT, Profile::SMART] {
            let (logged, outcome) = inline_game(decks, seed, profile);
            assert!(
                !matches!(outcome, Outcome::Stalled(_)),
                "seed {seed}: {outcome:?}"
            );
            let p = replay(decks, seed, profile, &logged, outcome);
            paused.resolving += p.resolving;
            paused.step_actions += p.step_actions;
        }
    }
    assert!(
        paused.step_actions >= 3 && paused.resolving >= 3,
        "{paused:?}: the test exercises too little"
    );
}

/// Dredge needs enough cards in the library (CR 702.52a). A token waiting for state-based
/// actions (CR 111.6) cannot supply the second card for dredge 2.
#[test]
fn a_library_token_cannot_make_a_draw_eligible_for_dredge() {
    let table = GameTable::new(
        Format::CONSTRUCTED,
        &DREDGE_CARDS,
        [DREDGE_DECK, DREDGE_DECK],
    )
    .unwrap();
    let mut state = State::new(&table, 1, Profile::EXACT).unwrap();
    let mut scratch = Scratch::new();
    let player = Seat::P0;
    let dredger = find(&state, &table, player, "Test Dredger");
    state
        .move_object(&mut scratch.events, dredger, Zone::Graveyard, Place::Top)
        .unwrap();
    let excess = u8::try_from(state.library(player).len() - 2).unwrap();
    state.draw_n(&mut scratch.events, player, excess).unwrap();
    assert_eq!(
        crate::replacement::dredges(&state, &table, player).sole(),
        Some(dredger)
    );

    state.draw_n(&mut scratch.events, player, 1).unwrap();
    let card = *state.library(player).last().unwrap();
    let token = Slot::pool().next().unwrap();
    let values = state.copiable(dredger);
    state.make_token(&table, token, player, values).unwrap();
    state
        .move_object(&mut scratch.events, token, Zone::Battlefield, Place::Top)
        .unwrap();
    state
        .move_object(&mut scratch.events, token, Zone::Library, Place::Top)
        .unwrap();
    assert!(crate::replacement::dredges(&state, &table, player).is_empty());

    let mut game = Game::new(&mut state, &table, &mut scratch.events);
    crate::zones::drawing::draw(&mut game, player, 1).unwrap();
    assert_eq!(state.object(card).zone(), Zone::Hand);
    assert_eq!(state.object(dredger).zone(), Zone::Graveyard);
    assert_eq!(state.library(player), &[token]);
}

/// A value chosen as a permanent enters, answered inline as a rollout would and replayed
/// through `Replay`, reaches the same state: a color and a name chosen as a spell resolves,
/// and a creature type as a land is played, which is a decision of its own both ways.
#[test]
fn values_chosen_as_permanents_enter_replay_to_the_same_state() {
    let decks = (&super::chosen::CARDS, super::chosen::DECK);
    let mut paused = 0;
    for seed in 0..3 {
        for profile in [Profile::EXACT, Profile::SMART] {
            let (logged, outcome) = inline_game(decks, seed, profile);
            assert!(
                !matches!(outcome, Outcome::Stalled(_)),
                "seed {seed}: {outcome:?}"
            );
            paused += replay(decks, seed, profile, &logged, outcome).resolving;
        }
    }
    assert!(
        paused >= 6,
        "only {paused} paused: the test exercises too little"
    );
}
