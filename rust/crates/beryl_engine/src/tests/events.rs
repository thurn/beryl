//! Events, triggers, replacement effects, state-based actions and layers, on positions built
//! through the event pipeline with test-only cards.

// Bodies share the `Flow` signature of `Ability::body`, whether or not they can pause.
#![allow(clippy::unnecessary_wraps)]

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityKind, Affected, CardDef, CastingOption, Castings, Cost, Delayed, Face, FaceIx,
    Keywords, LayerSpec, ManaColor, ManaCost, Modification, Replacement, StaticCondition, Subtype,
    Subtypes, TargetKind, TargetSpec, Trigger, Types,
};
use crate::effect::EffectCtx;
use crate::event::{self, Event, Reason, Recipient, TokenEntry};
use crate::filter::{Filter, Whose};
use crate::fixed::Fixed;
use crate::ids::{Seat, Slot};
use crate::state::{
    Continuous, CounterKind, Duration, ManaPool, ObjectFlags, PendingTrigger, TurnStep, Zone,
};
use crate::{
    CardTable, Choice, Decision, DecisionKind, Decklist, GameTable, Opt, Outcome, Profile, Replay,
    Scratch, State, Step, apply, next,
};
use crate::{Layout, bodies, conditions};

use super::{LAND, begin};

fn gain_one<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    ctx.gain_life(ctx.controller(), 1);
    Ok(())
}

fn draw_one<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    ctx.draw(ctx.controller(), 1);
    Ok(())
}

static GAIN_LAND: CardDef = CardDef {
    name: "Test Gain Land",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::triggered(Trigger::Enters, bodies!(gain_one))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static TAPLAND: CardDef = CardDef {
    name: "Test Tapland",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::replacing(Replacement::EntersTapped)],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static COAST: CardDef = CardDef {
    name: "Test Coast",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::replacing(Replacement::EntersTappedUnlessFewLands(
            2,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static SPRINGS: CardDef = CardDef {
    name: "Test Springs",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::replacing(Replacement::EntersTappedUnlessPayLife(
            3,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

fn crypt_flip<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    let you = ctx.controller();
    if !ctx.flip_coin(you) {
        ctx.deal_damage(Recipient::Player(you), 3);
    }
    Ok(())
}

/// Mana Crypt's upkeep trigger, on a land so random games can play it.
static CRYPT_LAND: CardDef = CardDef {
    name: "Test Crypt Land",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::triggered(
            Trigger::Step(TurnStep::Upkeep, Whose::You),
            bodies!(crypt_flip),
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static WATCHER: CardDef = CardDef {
    name: "Test Watcher",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::triggered(
            Trigger::Step(TurnStep::Upkeep, Whose::Opponent),
            bodies!(gain_one),
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static ENDER: CardDef = CardDef {
    name: "Test Ender",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::triggered(
            Trigger::Step(TurnStep::End, Whose::You),
            bodies!(draw_one),
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

fn drain_delay<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    ctx.delay(1, 2);
    Ok(())
}

fn drain_mana<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    let amount = ctx.trigger().amount;
    ctx.add_mana(ctx.controller(), ManaColor::Colorless, amount);
    Ok(())
}

/// Mana Drain's delayed trigger, created by a land's enters trigger.
static DRAIN_LAND: CardDef = CardDef {
    name: "Test Drain Land",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[
            Ability::triggered(Trigger::Enters, bodies!(drain_delay)),
            Ability::triggered(
                Trigger::Delayed(Delayed::YourNextMainPhase),
                bodies!(drain_mana),
            ),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

fn opponent_loses_one<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    ctx.lose_life(ctx.controller().opponent(), 1);
    Ok(())
}

static EMBLEM: CardDef = CardDef {
    name: "Test Emblem",
    faces: &[Face {
        abilities: &[Ability::triggered(
            Trigger::YouCast,
            bodies!(opponent_loses_one),
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

fn get_emblem<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    ctx.create_emblem(ctx.controller(), &EMBLEM);
    Ok(())
}

static EMBLEM_LAND: CardDef = CardDef {
    name: "Test Emblem Land",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::triggered(Trigger::Enters, bodies!(get_emblem))],
        ..Face::EMPTY
    }],
    tokens: &[&EMBLEM],
};

static BEAR: CardDef = CardDef {
    name: "Test Bear",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};

fn not_a_token<L: Layout>(_: &State<L>, _: &GameTable, tr: &PendingTrigger<L>) -> bool {
    !tr.info.lki.token
}

fn copy_the_dead<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    let card = ctx.trigger().lki.card;
    ctx.create_tokens_of(card, 1, TokenEntry::default());
    Ok(())
}

/// Sphinx of False Conclusions's dies trigger: "if it wasn't a token, create a token that's a
/// copy of it".
static SPHINX: CardDef = CardDef {
    name: "Test Sphinx",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(3),
        toughness: Some(3),
        abilities: &[Ability {
            condition: Some(conditions!(not_a_token)),
            ..Ability::triggered(Trigger::Dies, bodies!(copy_the_dead))
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// Thundertrap Trainer stand-in: a creature with an enters trigger, for token copies.
static TRAINER: CardDef = CardDef {
    name: "Test Trainer",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(3),
        toughness: Some(3),
        abilities: &[Ability::triggered(Trigger::Enters, bodies!(gain_one))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static RAIDER: CardDef = CardDef {
    name: "Test Raider",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        abilities: &[
            Ability::triggered(Trigger::Attacks, bodies!(gain_one)),
            Ability::triggered(Trigger::CombatDamageToPlayer, bodies!(draw_one)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static HUMAN: CardDef = CardDef {
    name: "Test Human",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

fn human_attacking<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    ctx.create_tokens(
        &HUMAN,
        1,
        TokenEntry {
            tapped: true,
            attacking: true,
            ..TokenEntry::default()
        },
    );
    Ok(())
}

/// Adeline, Resplendent Cathar: power equal to the creatures you control; whenever you
/// attack, a 1/1 Human tapped and attacking.
static ADELINE: CardDef = CardDef {
    name: "Test Adeline",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(0),
        toughness: Some(4),
        abilities: &[
            Ability::statik(LayerSpec {
                affected: Affected::Itself,
                condition: StaticCondition::Always,
                modification: Modification::PowerIsCreaturesYouControl,
            }),
            Ability::triggered(Trigger::YouAttack, bodies!(human_attacking)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[&HUMAN],
};

static INSECT: CardDef = CardDef {
    name: "Test Insect",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static MAP: CardDef = CardDef {
    name: "Test Map",
    faces: &[Face {
        types: Types::ARTIFACT,
        ..Face::EMPTY
    }],
    tokens: &[],
};

fn two_insects<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    ctx.create_tokens(&INSECT, 2, TokenEntry::default());
    Ok(())
}

/// Overlord of the Mistmoors: not a creature while it has a time counter (impending); when
/// it enters, two Insects.
static OVERLORD: CardDef = CardDef {
    name: "Test Overlord",
    faces: &[Face {
        types: Types::ENCHANTMENT.union(Types::CREATURE),
        power: Some(5),
        toughness: Some(5),
        abilities: &[
            Ability::statik(LayerSpec {
                affected: Affected::Itself,
                condition: StaticCondition::HasCounter(CounterKind::Time),
                modification: Modification::RemoveTypes(Types::CREATURE),
            }),
            Ability::triggered(Trigger::Enters, bodies!(two_insects)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[&INSECT],
};

fn story_counter<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    ctx.add_counters(ctx.source(), CounterKind::Story, 1);
    Ok(())
}

fn two_maps<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    ctx.create_tokens(&MAP, 2, TokenEntry::default());
    Ok(())
}

/// Staff of the Storyteller: a story counter whenever you create one or more creature
/// tokens. Its second ability makes noncreature tokens, for the test that they don't count.
static STAFF: CardDef = CardDef {
    name: "Test Staff",
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[
            Ability::triggered(
                Trigger::YouCreateTokens(Types::CREATURE),
                bodies!(story_counter),
            ),
            Ability::triggered(
                Trigger::Delayed(Delayed::YourNextMainPhase),
                bodies!(two_maps),
            ),
        ],
        ..Face::EMPTY
    }],
    tokens: &[&MAP],
};

static WALKER: CardDef = CardDef {
    name: "Test Walker",
    faces: &[Face {
        types: Types::PLANESWALKER,
        loyalty: Some(3),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// The crew ability Swift Reconfiguration grants ("crew 5"): its body is the combat work's.
const CREW: Ability = Ability {
    kind: AbilityKind::Activated,
    cost: Cost::NONE,
    ..Ability::NONE
};

/// Swift Reconfiguration: "Enchanted permanent is a Vehicle artifact with crew 5 and it loses
/// all other card types."
static RECONFIG: CardDef = CardDef {
    name: "Test Reconfiguration",
    faces: &[Face {
        types: Types::ENCHANTMENT,
        subtypes: Subtypes::of(&[Subtype::Aura]),
        abilities: &[
            Ability::statik(LayerSpec {
                affected: Affected::Enchanted,
                condition: StaticCondition::Always,
                modification: Modification::SetTypes(Types::ARTIFACT),
            }),
            Ability::statik(LayerSpec {
                affected: Affected::Enchanted,
                condition: StaticCondition::Always,
                modification: Modification::AddSubtype(Subtype::Vehicle),
            }),
            Ability::statik(LayerSpec {
                affected: Affected::Enchanted,
                condition: StaticCondition::Always,
                modification: Modification::Grant(0),
            }),
        ],
        grants: &[CREW],
        ..Face::EMPTY
    }],
    tokens: &[],
};

fn exile_an_opposing_creature<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    let them = ctx.controller().opponent();
    let s = ctx.state();
    let target = s
        .battlefield()
        .iter()
        .copied()
        .find(|&slot| s.controller(slot) == them && s.derived(slot).types.contains(Types::CREATURE))
        .map(|slot| s.object_ref(slot));
    if let Some(target) = target {
        ctx.exile_until_source_leaves(target);
    }
    Ok(())
}

/// Aang's Iceberg: "exile ... until this leaves the battlefield".
static ICEBERG: CardDef = CardDef {
    name: "Test Iceberg",
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::triggered(
            Trigger::Enters,
            bodies!(exile_an_opposing_creature),
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[
    &LAND,
    &GAIN_LAND,
    &TAPLAND,
    &COAST,
    &SPRINGS,
    &CRYPT_LAND,
    &WATCHER,
    &ENDER,
    &DRAIN_LAND,
    &EMBLEM_LAND,
    &BEAR,
    &SPHINX,
    &TRAINER,
    &RAIDER,
    &ADELINE,
    &OVERLORD,
    &STAFF,
    &WALKER,
    &RECONFIG,
    &ICEBERG,
]);

/// Every card once, then lands: every test finds what it needs in either library.
const DECK: &Decklist = &[
    (1, "Test Gain Land"),
    (1, "Test Tapland"),
    (2, "Test Coast"),
    (1, "Test Springs"),
    (2, "Test Crypt Land"),
    (2, "Test Watcher"),
    (1, "Test Ender"),
    (1, "Test Drain Land"),
    (1, "Test Emblem Land"),
    (3, "Test Bear"),
    (1, "Test Sphinx"),
    (1, "Test Trainer"),
    (2, "Test Raider"),
    (1, "Test Adeline"),
    (1, "Test Overlord"),
    (1, "Test Staff"),
    (1, "Test Walker"),
    (1, "Test Reconfiguration"),
    (1, "Test Iceberg"),
    (15, "Test Land"),
];

/// A game in the starting player's first main phase, with that player and the table.
fn game() -> (GameTable, State, Seat) {
    game_with_seed(7)
}

/// Each player gets a basic land in hand, so every main phase of theirs is a decision
/// (passing priority never plays it) and tests can stop there.
fn game_with_seed(seed: u64) -> (GameTable, State, Seat) {
    let t = GameTable::new(&CARDS, [DECK, DECK]).unwrap();
    let mut s = State::new(&t, seed, Profile::EXACT);
    begin(&t, &mut s);
    for player in Seat::BOTH {
        let land = (s.zone(player, Zone::Library))
            .iter()
            .find(|&slot| t.def(s.object(slot).card).name == "Test Land")
            .unwrap();
        crate::zone::move_to(&mut s, &mut crate::event::Sink::off(), land, Zone::Hand).unwrap();
    }
    let p = s.turn().starting_player;
    (t, s, p)
}

/// A slot holding `name` in the player's library or hand.
fn find<L: Layout>(s: &State<L>, t: &GameTable, player: Seat, name: &str) -> L::Slot {
    (s.zone(player, Zone::Library) | s.zone(player, Zone::Hand))
        .iter()
        .find(|&slot| t.def(s.object(slot).card).name == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

/// Puts a card onto the battlefield through the pipeline, under its owner's control.
fn put(t: &GameTable, s: &mut State, x: &mut Scratch, player: Seat, name: &str) -> Slot {
    let slot = find(s, t, player, name);
    event::move_one(s, t, &mut x.events, slot, Zone::Battlefield, Reason::Effect).unwrap();
    slot
}

/// Runs `body` as an ability of the permanent in `slot`.
fn with_ctx(
    t: &GameTable,
    s: &mut State,
    x: &mut Scratch,
    slot: Slot,
    body: impl FnOnce(&mut EffectCtx),
) {
    let trigger = PendingTrigger {
        source: Some(s.object_ref(slot)),
        card: s.object(slot).card,
        controller: s.controller(slot),
        ..PendingTrigger::default()
    };
    let mut ctx = EffectCtx::new(s, t, &mut x.events, &trigger, &Ability::NONE);
    body(&mut ctx);
    ctx.finish().unwrap();
}

/// Advances to the next decision, checking the state's invariants there.
fn decide_with(t: &GameTable, s: &mut State, x: &mut Scratch) -> Decision {
    match next(s, t, x, &mut Replay) {
        Step::Decide(d) => {
            assert_eq!(s.check_invariants(t), Ok(()));
            d
        }
        Step::Over(o) => panic!("the game ended: {o:?}"),
    }
}

/// Puts waiting triggers on the stack and passes until both are empty, returning the
/// decision reached.
fn resolve_all(t: &GameTable, s: &mut State, x: &mut Scratch) -> Decision {
    loop {
        let d = decide_with(t, s, x);
        if s.stack().is_empty() && s.triggers().is_empty() {
            return d;
        }
        assert!(matches!(
            d.kind,
            DecisionKind::Priority | DecisionKind::TriggerOrder
        ));
        apply(s, t, x, &mut Replay, Choice(0));
    }
}

/// Passes priority until the active player has priority with an empty stack in `step` of
/// their turn. Only main phases are sure to stop: elsewhere a player with nothing but a pass
/// passes silently.
fn advance_to(
    t: &GameTable,
    s: &mut State,
    x: &mut Scratch,
    active: Seat,
    step: TurnStep,
) -> Decision {
    loop {
        let d = decide_with(t, s, x);
        if s.turn().step == step
            && s.turn().active == active
            && s.stack().is_empty()
            && s.triggers().is_empty()
        {
            return d;
        }
        apply(s, t, x, &mut Replay, Choice(0));
    }
}

fn types<L: Layout>(s: &State<L>, slot: L::Slot) -> Types {
    s.derived(slot).types
}

#[test]
fn an_enters_trigger_resolves_after_the_land_is_played() {
    let (t, mut s, p) = game();
    let mut x = Scratch::recording();
    let land = find(&s, &t, p, "Test Gain Land");
    crate::zone::move_to(&mut s, &mut crate::event::Sink::off(), land, Zone::Hand).unwrap();
    let d = decide_with(&t, &mut s, &mut x);
    let play = (0..d.len() as u16)
        .find(|&i| d.candidates.option(Choice(i)) == Some(Opt::PlayLand(land)))
        .unwrap();
    apply(&mut s, &t, &mut x, &mut Replay, Choice(play));
    assert_eq!(s.triggers().len(), 1);
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p).life, 21);
    let events: Vec<Event> = x.events.drain().collect();
    let entered = events
        .iter()
        .position(|e| {
            matches!(
                e,
                Event::ZoneChange {
                    to: Zone::Battlefield,
                    reason: Reason::Play,
                    ..
                }
            )
        })
        .unwrap();
    let gained = events
        .iter()
        .position(|e| {
            *e == Event::LifeGain {
                player: p,
                amount: 1,
            }
        })
        .unwrap();
    assert!(entered < gained);
}

#[test]
fn a_dies_trigger_looks_back_and_its_intervening_if_skips_tokens() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let sphinx = put(&t, &mut s, &mut x, p, "Test Sphinx");
    with_ctx(&t, &mut s, &mut x, sphinx, |ctx| {
        ctx.deal_damage(Recipient::Permanent(ctx.source()), 3);
    });
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(sphinx).zone, Zone::Graveyard);
    let token = *s
        .battlefield()
        .iter()
        .find(|&&slot| s.object(slot).flags.has(ObjectFlags::TOKEN))
        .expect("a token copy");
    assert_eq!(t.def(s.object(token).card).name, "Test Sphinx");
    assert_eq!((s.derived(token).power, s.derived(token).toughness), (3, 3));
    // The copy is a token: when it dies it triggers nothing, and then ceases to exist.
    let token_ref = s.object_ref(token);
    with_ctx(&t, &mut s, &mut x, token, |ctx| ctx.destroy(token_ref));
    assert!(s.triggers().is_empty());
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(token).zone, Zone::Nowhere);
    assert!(!s.zone(p, Zone::Graveyard).contains(token));
}

#[test]
fn a_token_that_ceases_to_exist_unlinks_the_cards_exiled_with_it() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let card = s.object(bear).card;
    let mut token = None;
    with_ctx(&t, &mut s, &mut x, bear, |ctx| {
        token = ctx
            .create_tokens_of(card, 1, TokenEntry::default())
            .iter()
            .next();
    });
    let token = token.expect("a token copy");
    let land = find(&s, &t, p, "Test Land");
    let land_ref = s.object_ref(land);
    with_ctx(&t, &mut s, &mut x, token, |ctx| ctx.exile_linked(land_ref));
    assert_eq!(s.object(land).linked, Some(token));
    let token_ref = s.object_ref(token);
    with_ctx(&t, &mut s, &mut x, token, |ctx| ctx.destroy(token_ref));
    // The token ceases to exist (CR 704.5d), and nothing links to its freed slot.
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(token).zone, Zone::Nowhere);
    assert_eq!(s.object(land).zone, Zone::Exile);
    assert_eq!(s.object(land).linked, None);
}

#[test]
fn lethal_damage_zero_toughness_and_zero_loyalty_are_state_based() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let hurt = put(&t, &mut s, &mut x, p, "Test Bear");
    let shrunk = put(&t, &mut s, &mut x, p, "Test Bear");
    let walker = put(&t, &mut s, &mut x, p, "Test Walker");
    let healthy = put(&t, &mut s, &mut x, p, "Test Bear");
    assert_eq!(
        s.object(walker).counters.loyalty,
        3,
        "enters with its loyalty"
    );
    let shrunk_ref = s.object_ref(shrunk);
    let walker_ref = s.object_ref(walker);
    with_ctx(&t, &mut s, &mut x, hurt, |ctx| {
        ctx.deal_damage(Recipient::Permanent(ctx.source()), 2);
        ctx.add_effect(
            shrunk_ref,
            Continuous::Layer(Modification::ModifyPt(0, -2)),
            Duration::EndOfTurn,
        );
        ctx.deal_damage(Recipient::Permanent(walker_ref), 3);
    });
    let healthy_ref = s.object_ref(healthy);
    with_ctx(&t, &mut s, &mut x, healthy, |ctx| {
        ctx.deal_damage(Recipient::Permanent(healthy_ref), 1);
    });
    resolve_all(&t, &mut s, &mut x);
    for slot in [hurt, shrunk, walker] {
        assert_eq!(s.object(slot).zone, Zone::Graveyard);
    }
    assert_eq!(s.object(healthy).zone, Zone::Battlefield);
}

#[test]
fn attack_triggers_fire_per_attacker_and_whenever_you_attack_once() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let a = put(&t, &mut s, &mut x, p, "Test Raider");
    let b = put(&t, &mut s, &mut x, p, "Test Raider");
    let adeline = put(&t, &mut s, &mut x, p, "Test Adeline");
    let attackers: Bitboard = [a, b, adeline].into_iter().collect();
    for slot in attackers.iter() {
        s.object_mut(slot).flags.set(ObjectFlags::ATTACKING, true);
    }
    event::settle(
        &mut s,
        &t,
        &mut x.events,
        Event::Attack {
            player: p,
            attackers,
        },
    )
    .unwrap();
    // Two Raider triggers and one Adeline trigger.
    assert_eq!(s.triggers().len(), 3);
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p).life, 22);
    let human = *s
        .battlefield()
        .iter()
        .find(|&&slot| t.def(s.object(slot).card).name == "Test Human")
        .unwrap();
    let o = s.object(human);
    assert!(o.flags.has(ObjectFlags::TAPPED) && o.flags.has(ObjectFlags::ATTACKING));
    // A creature put onto the battlefield attacking was never declared: no new triggers.
    assert!(s.triggers().is_empty());
    // Adeline counts the creatures her controller controls, the Human included.
    assert_eq!(s.derived(adeline).power, 4);
}

#[test]
fn combat_damage_to_a_player_triggers_and_noncombat_damage_does_not() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let raider = put(&t, &mut s, &mut x, p, "Test Raider");
    let source = s.object_ref(raider);
    let q = p.opponent();
    event::deal_damage(
        &mut s,
        &t,
        &mut x.events,
        source,
        Recipient::Player(q),
        2,
        false,
    )
    .unwrap();
    assert!(s.triggers().is_empty());
    event::deal_damage(
        &mut s,
        &t,
        &mut x.events,
        source,
        Recipient::Player(q),
        2,
        true,
    )
    .unwrap();
    assert_eq!(s.triggers().len(), 1);
    assert_eq!(s.triggers()[0].info.player, q);
    let hand = s.zone(p, Zone::Hand).count();
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.zone(p, Zone::Hand).count(), hand + 1);
    assert_eq!(s.player(q).life, 16);
}

#[test]
fn upkeep_triggers_are_ordered_apnap_and_flip_coins() {
    let mut flips = [0, 0];
    for seed in 0..16 {
        let (t, mut s, p) = game_with_seed(seed);
        let q = p.opponent();
        let mut x = Scratch::recording();
        put(&t, &mut s, &mut x, q, "Test Crypt Land");
        put(&t, &mut s, &mut x, p, "Test Watcher");
        x.events.drain();
        advance_to(&t, &mut s, &mut x, q, TurnStep::PrecombatMain);
        // q's upkeep: q's Crypt and p's Watcher trigger. The active player's goes on the stack
        // first, so the Watcher, on top, resolves first.
        let events: Vec<Event> = x.events.drain().collect();
        let upkeep = events
            .iter()
            .position(|e| {
                *e == Event::StepBegins {
                    step: TurnStep::Upkeep,
                    active: q,
                }
            })
            .unwrap();
        let gain = events.iter().position(|e| {
            *e == Event::LifeGain {
                player: p,
                amount: 1,
            }
        });
        let flip = events
            .iter()
            .position(|e| matches!(e, Event::CoinFlip { player, .. } if *player == q));
        let (gain, flip) = (gain.unwrap(), flip.unwrap());
        assert!(
            upkeep < gain && gain < flip,
            "the nonactive player's trigger resolves first"
        );
        let won = events
            .iter()
            .any(|e| matches!(e, Event::CoinFlip { won: true, .. }));
        flips[usize::from(won)] += 1;
        assert_eq!(s.player(q).life, if won { 20 } else { 17 });
    }
    assert!(flips[0] > 0 && flips[1] > 0, "{flips:?}");
}

/// The trigger-order decisions on the way from the starting player's first main phase to
/// their next one, with two Crypt Lands of theirs and two Watchers of the opponent's.
fn order_decisions(distinct: bool) -> (Seat, Vec<(Seat, usize)>) {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let a = put(&t, &mut s, &mut x, p, "Test Crypt Land");
    put(&t, &mut s, &mut x, p, "Test Crypt Land");
    put(&t, &mut s, &mut x, p.opponent(), "Test Watcher");
    put(&t, &mut s, &mut x, p.opponent(), "Test Watcher");
    if distinct {
        let r = s.object_ref(a);
        with_ctx(&t, &mut s, &mut x, a, |ctx| {
            ctx.add_counters(r, CounterKind::Story, 1);
        });
    }
    let mut asked = Vec::new();
    let mut turns = 0;
    loop {
        let d = decide_with(&t, &mut s, &mut x);
        if d.kind == DecisionKind::TriggerOrder {
            asked.push((d.player, d.len()));
        }
        if s.turn().step == TurnStep::PrecombatMain && s.turn().active == p && s.stack().is_empty()
        {
            turns += 1;
            if turns == 2 {
                return (p, asked);
            }
        }
        apply(&mut s, &t, &mut x, &mut Replay, Choice(0));
    }
}

#[test]
fn a_player_orders_their_own_triggers_only_when_they_differ() {
    let (_, same) = order_decisions(false);
    assert!(
        same.is_empty(),
        "equivalent triggers are ordered silently: {same:?}"
    );
    let (p, distinct) = order_decisions(true);
    assert_eq!(distinct, vec![(p, 2)]);
}

#[test]
fn an_end_step_trigger_fires_on_its_controllers_turn_only() {
    let (t, mut s, p) = game();
    let mut x = Scratch::recording();
    put(&t, &mut s, &mut x, p, "Test Ender");
    advance_to(&t, &mut s, &mut x, p.opponent(), TurnStep::PrecombatMain);
    advance_to(&t, &mut s, &mut x, p, TurnStep::PrecombatMain);
    let draws: Vec<(Seat, TurnStep)> = {
        let mut step = TurnStep::Untap;
        x.events
            .drain()
            .filter_map(|e| match e {
                Event::StepBegins { step: now, .. } => {
                    step = now;
                    None
                }
                Event::Draw { player, .. } => Some((player, step)),
                _ => None,
            })
            .collect()
    };
    // p's end step, then each player's draw step.
    assert_eq!(
        draws,
        [
            (p, TurnStep::End),
            (p.opponent(), TurnStep::Draw),
            (p, TurnStep::Draw)
        ]
    );
}

#[test]
fn a_delayed_trigger_fires_once_at_your_next_main_phase() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, p, "Test Drain Land");
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.delayed.len(), 1);
    // Created in the precombat main phase: the next main phase is the postcombat one.
    advance_to(&t, &mut s, &mut x, p, TurnStep::PostcombatMain);
    assert!(s.delayed.is_empty());
    assert_eq!(s.player(p).mana[ManaColor::Colorless], 2);
    advance_to(&t, &mut s, &mut x, p, TurnStep::PrecombatMain);
    assert_eq!(
        s.player(p).mana,
        ManaPool::default(),
        "it triggered only once"
    );
}

#[test]
fn an_emblem_triggers_from_the_command_zone() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let land = put(&t, &mut s, &mut x, p, "Test Emblem Land");
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p).emblems.len(), 1);
    // The emblem stays when its creator leaves.
    with_ctx(&t, &mut s, &mut x, land, |ctx| ctx.destroy(ctx.source()));
    let spell = s.object_ref(find(&s, &t, p, "Test Land"));
    event::settle(
        &mut s,
        &t,
        &mut x.events,
        Event::Cast {
            spell,
            controller: p,
            targets: Fixed::default(),
            x: 0,
            costs: Castings::default(),
            flashback: false,
        },
    )
    .unwrap();
    let them = Event::Cast {
        spell,
        controller: p.opponent(),
        targets: Fixed::default(),
        x: 0,
        costs: Castings::default(),
        flashback: false,
    };
    event::settle(&mut s, &t, &mut x.events, them).unwrap();
    assert_eq!(s.triggers().len(), 1, "only its owner's spells");
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p.opponent()).life, 19);
}

#[test]
fn whenever_you_create_creature_tokens_triggers_once_per_event() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let staff = put(&t, &mut s, &mut x, p, "Test Staff");
    put(&t, &mut s, &mut x, p, "Test Overlord");
    // Overlord's enters trigger makes two Insects: one event, one story counter.
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(staff).counters.get(CounterKind::Story), 1);
    // Noncreature tokens don't count.
    with_ctx(&t, &mut s, &mut x, staff, |ctx| {
        ctx.create_tokens(&MAP, 2, TokenEntry::default());
    });
    assert!(s.triggers().is_empty());
    let insects = s
        .battlefield()
        .iter()
        .filter(|&&slot| t.def(s.object(slot).card).name == "Test Insect")
        .count();
    assert_eq!(insects, 2);
}

#[test]
fn lands_enter_tapped_conditionally_or_unless_life_is_paid() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let tapped = |s: &State, slot: Slot| s.object(slot).flags.has(ObjectFlags::TAPPED);
    let tapland = put(&t, &mut s, &mut x, p, "Test Tapland");
    assert!(tapped(&s, tapland));
    // Seachrome Coast: untapped with two or fewer other lands, tapped with three.
    let coast = put(&t, &mut s, &mut x, p, "Test Coast");
    assert!(!tapped(&s, coast), "one other land");
    put(&t, &mut s, &mut x, p, "Test Land");
    put(&t, &mut s, &mut x, p, "Test Land");
    let late = put(&t, &mut s, &mut x, p, "Test Coast");
    assert!(tapped(&s, late), "four other lands");

    // Soporific Springs, played: its controller is asked.
    for (pay, life, enters_tapped) in [(true, 17, false), (false, 20, true)] {
        let (t, mut s, p) = game();
        let mut x = Scratch::new();
        let springs = find(&s, &t, p, "Test Springs");
        crate::zone::move_to(&mut s, &mut crate::event::Sink::off(), springs, Zone::Hand).unwrap();
        let d = decide_with(&t, &mut s, &mut x);
        let play = (0..d.len() as u16)
            .find(|&i| d.candidates.option(Choice(i)) == Some(Opt::PlayLand(springs)))
            .unwrap();
        apply(&mut s, &t, &mut x, &mut Replay, Choice(play));
        let d = decide_with(&t, &mut s, &mut x);
        assert_eq!((d.kind, d.player), (DecisionKind::PayLife, p));
        let answer = if pay { Opt::Yes } else { Opt::No };
        let c = (0..2)
            .find(|&i| d.candidates.option(Choice(i)) == Some(answer))
            .unwrap();
        apply(&mut s, &t, &mut x, &mut Replay, Choice(c));
        assert_eq!(s.object(springs).zone, Zone::Battlefield);
        assert_eq!(tapped(&s, springs), enters_tapped);
        assert_eq!(s.player(p).life, life);
    }

    // A player who can't pay isn't asked.
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    s.player_mut(p).life = 2;
    let springs = find(&s, &t, p, "Test Springs");
    crate::zone::move_to(&mut s, &mut crate::event::Sink::off(), springs, Zone::Hand).unwrap();
    let d = decide_with(&t, &mut s, &mut x);
    let play = (0..d.len() as u16)
        .find(|&i| d.candidates.option(Choice(i)) == Some(Opt::PlayLand(springs)))
        .unwrap();
    apply(&mut s, &t, &mut x, &mut Replay, Choice(play));
    assert_eq!(s.object(springs).zone, Zone::Battlefield);
    assert!(tapped(&s, springs));
    assert_eq!(s.player(p).life, 2);
}

#[test]
fn exile_instead_of_the_graveyard_replaces_the_move() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let a = put(&t, &mut s, &mut x, p, "Test Bear");
    let b = put(&t, &mut s, &mut x, p, "Test Bear");
    let a_ref = s.object_ref(a);
    with_ctx(&t, &mut s, &mut x, b, |ctx| {
        ctx.add_effect(
            a_ref,
            Continuous::ExileInsteadOfGraveyard,
            Duration::EndOfTurn,
        );
        ctx.destroy(a_ref);
        ctx.destroy(ctx.source());
    });
    assert_eq!(s.object(a).zone, Zone::Exile);
    assert_eq!(s.object(b).zone, Zone::Graveyard);
}

#[test]
fn impending_counters_and_type_changes_apply_in_layer_four() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let overlord = find(&s, &t, p, "Test Overlord");
    let r = s.object_ref(overlord);
    let land = put(&t, &mut s, &mut x, p, "Test Land");
    with_ctx(&t, &mut s, &mut x, land, |ctx| {
        ctx.put_onto_battlefield(r, FaceIx::FRONT, Some((CounterKind::Time, 4)));
    });
    assert_eq!(s.object(overlord).counters.get(CounterKind::Time), 4);
    assert_eq!(types(&s, overlord), Types::ENCHANTMENT, "not a creature");
    let r = s.object_ref(overlord);
    with_ctx(&t, &mut s, &mut x, land, |ctx| {
        ctx.remove_counters(r, CounterKind::Time, 4);
    });
    assert_eq!(
        types(&s, overlord),
        Types::ENCHANTMENT.union(Types::CREATURE)
    );
}

#[test]
fn an_aura_makes_a_vehicle_artifact_with_crew_and_crewing_makes_it_a_creature() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let aura = put(&t, &mut s, &mut x, p, "Test Reconfiguration");
    s.object_mut(aura).attached_to = Some(bear);
    s.flags.derived_dirty = true;
    crate::layers::refresh(&mut s, &t, &mut x.events);
    let d = s.derived(bear);
    assert_eq!(d.types, Types::ARTIFACT);
    assert!(d.subtypes.contains(Subtype::Vehicle));
    let grant = d.granted[0].unwrap();
    assert_eq!(grant.card, s.object(aura).card);
    let granted = &t.def(grant.card).front().grants[usize::from(grant.grant)];
    assert_eq!(granted.kind, AbilityKind::Activated);
    // A land loses its land types with its land type (CR 205.1a), and keeps the Vehicle.
    let land = put(&t, &mut s, &mut x, p, "Test Land");
    s.object_mut(aura).attached_to = Some(land);
    s.flags.derived_dirty = true;
    crate::layers::refresh(&mut s, &t, &mut x.events);
    let subtypes = s.derived(land).subtypes;
    assert_eq!(subtypes.iter().collect::<Vec<_>>(), vec![Subtype::Vehicle]);
    assert_eq!(types(&s, bear), Types::CREATURE, "no longer enchanted");
    s.object_mut(aura).attached_to = Some(bear);
    s.flags.derived_dirty = true;
    crate::layers::refresh(&mut s, &t, &mut x.events);
    // Crewed: an artifact creature until end of turn, by a later timestamp.
    let r = s.object_ref(bear);
    with_ctx(&t, &mut s, &mut x, bear, |ctx| {
        ctx.add_effect(
            r,
            Continuous::Layer(Modification::AddTypes(Types::CREATURE)),
            Duration::EndOfTurn,
        );
    });
    assert_eq!(types(&s, bear), Types::ARTIFACT.union(Types::CREATURE));
    advance_to(&t, &mut s, &mut x, p.opponent(), TurnStep::PrecombatMain);
    assert_eq!(types(&s, bear), Types::ARTIFACT);
}

#[test]
fn an_aura_whose_permanent_leaves_is_attached_to_nothing_and_goes_to_the_graveyard() {
    // A card that leaves and returns before state-based actions are checked is a new object
    // the Aura was never attached to (CR 400.7).
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let aura = put(&t, &mut s, &mut x, p, "Test Reconfiguration");
    s.object_mut(aura).attached_to = Some(bear);
    s.flags.derived_dirty = true;
    crate::layers::refresh(&mut s, &t, &mut x.events);
    let r = s.object_ref(bear);
    with_ctx(&t, &mut s, &mut x, aura, |ctx| {
        ctx.destroy(r);
        let r = ctx.state().object_ref(bear);
        ctx.put_onto_battlefield(r, FaceIx::FRONT, None);
    });
    assert_eq!(s.object(aura).attached_to, None);
    assert_eq!(types(&s, bear), Types::CREATURE);
    decide_with(&t, &mut s, &mut x);
    assert_eq!(s.object(aura).zone, Zone::Graveyard);

    // On a token, the token ceases to exist and the Aura goes, in one check.
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let aura = put(&t, &mut s, &mut x, p, "Test Reconfiguration");
    let mut token = Bitboard::EMPTY;
    with_ctx(&t, &mut s, &mut x, aura, |ctx| {
        token = ctx.create_tokens(&MAP, 1, TokenEntry::default());
    });
    let token = token.first().unwrap();
    s.object_mut(aura).attached_to = Some(token);
    s.flags.derived_dirty = true;
    crate::layers::refresh(&mut s, &t, &mut x.events);
    let r = s.object_ref(token);
    with_ctx(&t, &mut s, &mut x, aura, |ctx| ctx.destroy(r));
    decide_with(&t, &mut s, &mut x);
    assert_eq!(s.object(token).zone, Zone::Nowhere);
    assert_eq!(s.object(aura).zone, Zone::Graveyard);
}

#[test]
fn a_stolen_creatures_dies_trigger_belongs_to_its_last_controller() {
    // CR 603.3a: "the player who controlled its source when it triggered"; the last-known
    // information of a permanent that left is how it last existed on the battlefield.
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let q = p.opponent();
    let sphinx = put(&t, &mut s, &mut x, q, "Test Sphinx");
    let thief = put(&t, &mut s, &mut x, p, "Test Bear");
    let r = s.object_ref(sphinx);
    with_ctx(&t, &mut s, &mut x, thief, |ctx| {
        ctx.gain_control(r, Duration::Indefinite);
        ctx.destroy(r);
    });
    assert_eq!(s.triggers().len(), 1);
    assert_eq!(s.triggers()[0].controller, p);
    assert!(s.zone(q, Zone::Graveyard).contains(sphinx));
    resolve_all(&t, &mut s, &mut x);
    let copy = *s
        .battlefield()
        .iter()
        .find(|&&slot| s.object(slot).flags.has(ObjectFlags::TOKEN))
        .expect("a token copy");
    assert_eq!((s.object(copy).owner, s.controller(copy)), (p, p));
}

#[test]
fn removing_a_counter_kind_an_object_lacks_changes_nothing() {
    let (t, mut s, p) = game();
    let mut x = Scratch::recording();
    let staff = put(&t, &mut s, &mut x, p, "Test Staff");
    let r = s.object_ref(staff);
    with_ctx(&t, &mut s, &mut x, staff, |ctx| {
        ctx.add_counters(r, CounterKind::Story, 2);
    });
    x.events.drain();
    with_ctx(&t, &mut s, &mut x, staff, |ctx| {
        ctx.remove_counters(r, CounterKind::Time, 1);
    });
    assert_eq!(s.object(staff).counters.get(CounterKind::Story), 2);
    assert!(x.events.events().is_empty());
}

#[test]
fn gaining_control_changes_the_controller_until_the_effect_ends() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let q = p.opponent();
    let theirs = put(&t, &mut s, &mut x, q, "Test Bear");
    let mine = put(&t, &mut s, &mut x, p, "Test Adeline");
    assert_eq!(s.derived(mine).power, 1);
    let r = s.object_ref(theirs);
    with_ctx(&t, &mut s, &mut x, mine, |ctx| {
        ctx.gain_control(r, Duration::Indefinite);
    });
    assert_eq!(s.controller(theirs), p);
    assert_eq!(s.object(theirs).owner, q);
    assert!(s.object(theirs).flags.has(ObjectFlags::SUMMONING_SICK));
    assert_eq!(s.derived(mine).power, 2, "Adeline counts it");
    // It untaps in its new controller's untap step and goes to its owner's graveyard.
    s.object_mut(theirs).flags.set(ObjectFlags::TAPPED, true);
    advance_to(&t, &mut s, &mut x, q, TurnStep::PrecombatMain);
    assert!(
        s.object(theirs).flags.has(ObjectFlags::TAPPED),
        "not in its owner's"
    );
    advance_to(&t, &mut s, &mut x, p, TurnStep::PrecombatMain);
    assert!(!s.object(theirs).flags.has(ObjectFlags::TAPPED));
    assert_eq!(s.controller(theirs), p, "an indefinite effect lasts");
    with_ctx(&t, &mut s, &mut x, mine, |ctx| ctx.destroy(r));
    assert_eq!(s.object(theirs).zone, Zone::Graveyard);
    assert!(s.zone(q, Zone::Graveyard).contains(theirs));
    assert!(s.effects().is_empty(), "the effect ended with its object");
}

#[test]
fn granted_keywords_end_at_cleanup_and_until_your_next_turn_lasts_through_theirs() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let r = s.object_ref(bear);
    with_ctx(&t, &mut s, &mut x, bear, |ctx| {
        ctx.add_effect(
            r,
            Continuous::Layer(Modification::AddKeywords(Keywords::FIRST_STRIKE)),
            Duration::EndOfTurn,
        );
        ctx.add_effect(
            r,
            Continuous::Layer(Modification::ModifyPt(-2, 0)),
            Duration::UntilYourNextTurn(p),
        );
        ctx.add_counters(r, CounterKind::PlusOne, 1);
    });
    let d = s.derived(bear);
    assert!(d.keywords.contains(Keywords::FIRST_STRIKE));
    // Layer 7c: -2/-0 and a +1/+1 counter.
    assert_eq!((d.power, d.toughness), (1, 3));
    advance_to(&t, &mut s, &mut x, p.opponent(), TurnStep::PrecombatMain);
    let d = s.derived(bear);
    assert!(!d.keywords.contains(Keywords::FIRST_STRIKE));
    assert_eq!((d.power, d.toughness), (1, 3), "until your next turn");
    advance_to(&t, &mut s, &mut x, p, TurnStep::PrecombatMain);
    assert_eq!((s.derived(bear).power, s.derived(bear).toughness), (3, 3));
}

#[test]
fn a_one_one_token_copy_has_the_cards_abilities() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let trainer = put(&t, &mut s, &mut x, p, "Test Trainer");
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p).life, 21);
    let card = s.object(trainer).card;
    let mut token = Bitboard::EMPTY;
    with_ctx(&t, &mut s, &mut x, trainer, |ctx| {
        token = ctx.create_tokens_of(
            card,
            1,
            TokenEntry {
                one_one: true,
                ..TokenEntry::default()
            },
        );
    });
    let token = token.first().unwrap();
    assert_eq!((s.derived(token).power, s.derived(token).toughness), (1, 1));
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p).life, 22, "the copy's enters trigger");
}

#[test]
fn an_object_exiled_until_its_exiler_leaves_returns_under_its_owners_control() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let q = p.opponent();
    let bear = put(&t, &mut s, &mut x, q, "Test Bear");
    let iceberg = put(&t, &mut s, &mut x, p, "Test Iceberg");
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(bear).zone, Zone::Exile);
    let r = s.object_ref(iceberg);
    with_ctx(&t, &mut s, &mut x, iceberg, |ctx| ctx.destroy(r));
    assert_eq!(s.object(bear).zone, Zone::Battlefield);
    assert_eq!(s.controller(bear), q);
    assert!(!s.object(bear).flags.has(ObjectFlags::EXILED_UNTIL));

    // CR 610.3b: if the exiler leaves before its trigger resolves, nothing is exiled.
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let q = p.opponent();
    let bear = put(&t, &mut s, &mut x, q, "Test Bear");
    let iceberg = put(&t, &mut s, &mut x, p, "Test Iceberg");
    let r = s.object_ref(iceberg);
    with_ctx(&t, &mut s, &mut x, iceberg, |ctx| ctx.destroy(r));
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(bear).zone, Zone::Battlefield);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "derived read while dirty")]
fn reading_derived_after_a_raw_change_without_a_refresh_panics() {
    let (t, mut s, p) = game();
    let bear = find(&s, &t, p, "Test Bear");
    crate::zone::move_to(
        &mut s,
        &mut crate::event::Sink::off(),
        bear,
        Zone::Battlefield,
    )
    .unwrap();
    let _ = s.derived(bear);
}

/// Random games over lands with enters, upkeep, end-step and delayed triggers and entering
/// replacements. Every `derived` read in them asserts it is clean (debug builds), so
/// finishing them shows nothing read `derived` dirty.
#[cfg(debug_assertions)]
#[test]
fn random_games_with_triggers_never_read_derived_dirty() {
    const LANDS: &Decklist = &[
        (6, "Test Gain Land"),
        (4, "Test Tapland"),
        (4, "Test Coast"),
        (4, "Test Springs"),
        (6, "Test Crypt Land"),
        (4, "Test Watcher"),
        (4, "Test Ender"),
        (4, "Test Drain Land"),
        (4, "Test Emblem Land"),
    ];
    let t = GameTable::new(&CARDS, [LANDS, LANDS]).unwrap();
    for seed in 0..20 {
        let mut s = State::new(&t, seed, Profile::EXACT);
        let mut rng = crate::rng::SplitMix64::new(seed);
        let outcome = super::play(
            &t,
            &mut s,
            |_, d| Choice(rng.below(d.len() as u64) as u16),
            |s, _| assert_eq!(s.check_invariants(&t), Ok(())),
        );
        assert!(
            matches!(outcome, Outcome::Win(_) | Outcome::Draw),
            "{outcome:?}"
        );
    }
}

fn cathar_human<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    ctx.create_attacking_tokens(&HUMAN, 1)?;
    Ok(())
}

/// Adeline's trigger as the card has it: the Human attacks what its controller chooses.
static CATHAR: CardDef = CardDef {
    name: "Test Cathar",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability {
            may_pause: true,
            ..Ability::triggered(Trigger::YouAttack, bodies!(cathar_human))
        }],
        ..Face::EMPTY
    }],
    tokens: &[&HUMAN],
};

static CATHAR_CARDS: CardTable = CardTable::new(&[&LAND, &WALKER, &CATHAR]);

/// [CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084): a creature put onto the
/// battlefield attacking attacks what its controller chooses, here the planeswalker the
/// declared attacker left alone.
#[test]
fn a_token_created_attacking_may_attack_a_planeswalker() {
    let t = GameTable::new(
        &CATHAR_CARDS,
        [
            &[(1, "Test Cathar"), (5, "Test Land")],
            &[(1, "Test Walker"), (5, "Test Land")],
        ],
    )
    .unwrap();
    let mut s = State::new(&t, 1, Profile::EXACT);
    s.begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let mut x = Scratch::new();
    let cathar = find(&s, &t, Seat::P0, "Test Cathar");
    s.place(cathar, Zone::Battlefield).unwrap();
    let walker = put(&t, &mut s, &mut x, Seat::P1, "Test Walker");
    let mut declared = false;
    let human = loop {
        let human = s
            .battlefield()
            .iter()
            .copied()
            .find(|&slot| t.def(s.object(slot).card).name == "Test Human");
        if let Some(human) = human {
            break human;
        }
        let d = decide_with(&t, &mut s, &mut x);
        let pick = |o: Opt| {
            let i = (0..d.len() as u16)
                .find(|&i| d.candidates.option(Choice(i)) == Some(o))
                .unwrap_or_else(|| panic!("{o:?} not among {d:?}"));
            Choice(i)
        };
        let choice = match d.kind {
            DecisionKind::Attacker if !declared => {
                declared = true;
                pick(Opt::Attack(cathar))
            }
            DecisionKind::Attacker => pick(Opt::Done),
            // The declared attacker attacks the player; the Human, as it is created, the
            // planeswalker.
            DecisionKind::AttackTarget if s.stack().is_empty() => pick(Opt::Defender(None)),
            DecisionKind::AttackTarget => {
                // Asked with the Human in view, attacking the player until it is told
                // otherwise.
                let mut seen = Scratch::new();
                let paused = crate::view(&s, &t, &mut seen);
                let shown = paused
                    .battlefield()
                    .iter()
                    .copied()
                    .find(|&slot| t.def(paused.object(slot).card).name == "Test Human")
                    .expect("the Human on the battlefield as it is asked about");
                let o = paused.object(shown);
                assert!(o.flags.has(ObjectFlags::ATTACKING) && o.flags.has(ObjectFlags::TAPPED));
                assert_eq!(o.combat, None);
                pick(Opt::Defender(Some(walker)))
            }
            _ => Choice(0),
        };
        apply(&mut s, &t, &mut x, &mut Replay, choice);
    };
    let o = s.object(human);
    assert!(o.flags.has(ObjectFlags::ATTACKING) && o.flags.has(ObjectFlags::TAPPED));
    assert!(o.flags.has(ObjectFlags::ATTACKS_PLANESWALKER));
    assert_eq!(o.combat, Some(walker));
    assert_eq!(s.object(cathar).combat, None);
}

fn steal_target<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> crate::card::Flow {
    if let Some(artifact) = ctx.target_object(0) {
        ctx.gain_control(artifact, Duration::Indefinite);
    }
    Ok(())
}

/// Thieving Skydiver: kicker {X}, X can't be 0; "When this creature enters, if it was
/// kicked, gain control of target artifact with mana value X or less."
static THIEF: CardDef = CardDef {
    name: "Test Thief",
    faces: &[Face {
        cost: ManaCost::generic(1),
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(1),
        abilities: &[Ability {
            targets: &[TargetSpec::one(
                TargetKind::Permanent,
                Filter::All(&[Filter::Types(Types::ARTIFACT), Filter::ManaValueAtMostX]),
            )],
            condition: Some(conditions!(crate::effect::kicked)),
            ..Ability::triggered(Trigger::Enters, bodies!(steal_target))
        }],
        casting: &[CastingOption::Kicker {
            cost: ManaCost::X,
            min_x: 1,
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static ROCK_1: CardDef = CardDef {
    name: "Test Rock 1",
    faces: &[Face {
        cost: ManaCost::generic(1),
        types: Types::ARTIFACT,
        ..Face::EMPTY
    }],
    tokens: &[],
};

static ROCK_3: CardDef = CardDef {
    name: "Test Rock 3",
    faces: &[Face {
        cost: ManaCost::generic(3),
        types: Types::ARTIFACT,
        ..Face::EMPTY
    }],
    tokens: &[],
};

static THIEF_CARDS: CardTable = CardTable::new(&[&LAND, &THIEF, &ROCK_1, &ROCK_3]);

/// The enters ability's X is the kicker's (CR 107.3m): kicked for 1 it can take only the
/// artifact of mana value 1, so that is chosen silently; kicked for 3, either, and it takes
/// the bigger.
#[test]
fn an_enters_ability_targets_by_the_x_its_spell_was_cast_with() {
    for (x, stolen) in [(1u8, "Test Rock 1"), (3, "Test Rock 3")] {
        let t = GameTable::new(
            &THIEF_CARDS,
            [
                &[(1, "Test Thief"), (6, "Test Land")],
                &[(1, "Test Rock 1"), (1, "Test Rock 3"), (4, "Test Land")],
            ],
        )
        .unwrap();
        let mut s = State::new(&t, 1, Profile::EXACT);
        s.begin_at(1, Seat::P0, TurnStep::PrecombatMain);
        let mut scratch = Scratch::new();
        let thief = find(&s, &t, Seat::P0, "Test Thief");
        s.place(thief, Zone::Hand).unwrap();
        for _ in 0..4 {
            let land = find(&s, &t, Seat::P0, "Test Land");
            s.place(land, Zone::Battlefield).unwrap();
        }
        let rocks = ["Test Rock 1", "Test Rock 3"].map(|name| {
            let rock = find(&s, &t, Seat::P1, name);
            s.place(rock, Zone::Battlefield).unwrap();
            rock
        });
        let mut cast = false;
        while !cast || !s.stack().is_empty() || !s.triggers().is_empty() {
            let d = decide_with(&t, &mut s, &mut scratch);
            let option = |o: Opt| {
                (0..d.len() as u16)
                    .map(Choice)
                    .find(|&c| d.candidates.option(c) == Some(o))
            };
            let choice = match d.kind {
                DecisionKind::Priority if !cast => {
                    cast = true;
                    option(Opt::Cast {
                        slot: thief,
                        face: FaceIx::FRONT,
                        costs: Castings::only(0),
                    })
                    .unwrap()
                }
                DecisionKind::X => option(Opt::Number(x)).unwrap(),
                DecisionKind::Target => {
                    assert_eq!(d.len(), 2, "kicked for {x}");
                    let i = (0..d.len() as u16)
                        .find(|&i| d.candidates.object(Choice(i)) == Some(rocks[1]))
                        .unwrap();
                    Choice(i)
                }
                _ => Choice(0),
            };
            apply(&mut s, &t, &mut scratch, &mut Replay, choice);
        }
        for rock in rocks {
            let name = t.def(s.object(rock).card).name;
            let expected = if name == stolen { Seat::P0 } else { Seat::P1 };
            assert_eq!(s.controller(rock), expected, "{name}, kicked for {x}");
        }
    }
}

/// A transforming double-faced card, its back face a planeswalker with no mana cost.
static PRODIGY: CardDef = CardDef {
    name: "Test Prodigy",
    faces: &[
        Face {
            cost: ManaCost::new(1, [0, 1, 0, 0, 0, 0]),
            types: Types::CREATURE,
            power: Some(0),
            toughness: Some(2),
            ..Face::EMPTY
        },
        Face {
            name: "Test Telepath",
            types: Types::PLANESWALKER,
            loyalty: Some(5),
            transformed: true,
            ..Face::EMPTY
        },
    ],
    tokens: &[],
};

static PRODIGY_CARDS: CardTable = CardTable::new(&[&LAND, &PRODIGY]);

/// [CR 712.8e](https://yawgatog.com/resources/magic-rules/#R7128e): with its back face up, a
/// transforming permanent's mana value is still its front face's, but a token copying it
/// with its back face up has mana value 0.
#[test]
fn a_transformed_permanent_has_its_front_faces_mana_value() {
    let t = GameTable::new(
        &PRODIGY_CARDS,
        [
            &[(1, "Test Prodigy"), (5, "Test Land")],
            &[(6, "Test Land")],
        ],
    )
    .unwrap();
    let mut s = State::new(&t, 1, Profile::EXACT);
    s.begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let prodigy = find(&s, &t, Seat::P0, "Test Prodigy");
    s.place(prodigy, Zone::Battlefield).unwrap();
    s.set_face(prodigy, FaceIx::BACK);
    assert!(!Filter::ManaValueAtMost(1).matches(&s, &t, prodigy, Seat::P0));
    assert!(Filter::ManaValueAtMost(2).matches(&s, &t, prodigy, Seat::P0));
    let mut x = Scratch::new();
    crate::layers::refresh(&mut s, &t, &mut x.events);
    let mut copy = None;
    with_ctx(&t, &mut s, &mut x, prodigy, |ctx| {
        let (card, how) = ctx.copiable_values(ctx.state().object_ref(prodigy));
        copy = ctx
            .create_tokens_of_for(Seat::P1, card, 1, how)
            .iter()
            .next();
    });
    let copy = copy.expect("a token copy");
    assert_eq!(s.object(copy).face, FaceIx::BACK);
    assert!(Filter::ManaValueAtMost(0).matches(&s, &t, copy, Seat::P1));
}
