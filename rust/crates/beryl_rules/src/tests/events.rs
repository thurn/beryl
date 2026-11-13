//! Events, triggers, replacement effects, state-based actions and layers, on positions built
//! through the event pipeline with test-only cards.

use beryl_card::effect::build::{
    add_counters, add_mana_of_kind, carried, create_attacking_tokens, create_copy,
    create_tokens as create_tokens_of_kind, deal_damage_to, delay_carrying, draw, each_opponent,
    flip_coin, for_each_player, gain_life, get_emblem, if_then, if_you_lose_the_flip, lose_life,
    modify, sequence, target, that_player, this, you,
};
use beryl_card::{Amount, Comparison, Effect};

use crate::actions::destroy::destroy;
use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityKind, CardDef, CastingOption, Castings, Condition, Cost, Face, FaceIx,
    Keywords, LayerSpec, ManaColor, ManaCost, Modification, Objects, Part, Players, Prompt,
    Purpose, Replacement, Subtype, Subtypes, TargetKind, TargetSpec, Trigger, Types,
};
use crate::card::{CastingKind, Flow};
use crate::damage;
use crate::event::{self, Event, Reason, Recipient, Sink, TokenEntry};
use crate::filter::{Evaluate, Filter};
use crate::fixed::Fixed;
use crate::game::Game;
use crate::ids::{Seat, Slot};
use crate::profile::Breakpoint;
use crate::resolution::Resolution;
use crate::state::{
    AbilityRef, AttackTarget, Continuous, CounterKind, Duration, LayerChange, ManaPool,
    MoveReplacement, PendingTrigger, StackEntry, StackItem, TriggerInfo, TurnStep, Zone, ZoneKey,
};
use crate::zone::Place;
use crate::zones;
use crate::{
    CardTable, Choice, Decision, DecisionKind, Decklist, GameTable, Opt, Outcome, Profile, Replay,
    Scratch, State, Step, apply, next,
};
use crate::{CustomBody, Layout, custom_body};
use crate::{continuous, counters, linked, one_shot, tokens};
use beryl_kernel::two_player::TwoPlayerSeat as _;

use super::{LAND, begin};

mod causes;
mod groups;

/// An ability for a resolution whose test reads none of it.
static NO_ABILITY: Ability = Ability::spell();

static GAIN_LAND: CardDef = CardDef {
    name: "Test Gain Land",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::triggered(&Trigger::ENTERS).effect(&gain_life(you(), 1))],
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

/// "Flip a coin. If you lose the flip, this deals 3 damage to you."
const CRYPT_FLIP: Effect = sequence(&[
    flip_coin(you()),
    if_then(if_you_lose_the_flip(), &deal_damage_to(you(), 3)),
]);

/// Mana Crypt's upkeep trigger, on a land so random games can play it.
static CRYPT_LAND: CardDef = CardDef {
    name: "Test Crypt Land",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[
            Ability::triggered(&Trigger::step(TurnStep::Upkeep, Players::You)).effect(&CRYPT_FLIP),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static WATCHER: CardDef = CardDef {
    name: "Test Watcher",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[
            Ability::triggered(&Trigger::step(TurnStep::Upkeep, Players::EachOpponent))
                .effect(&gain_life(you(), 1)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static ENDER: CardDef = CardDef {
    name: "Test Ender",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[
            Ability::triggered(&Trigger::step(TurnStep::End, Players::You)).effect(&draw(you(), 1)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// Mana Drain's delayed trigger, created by a land's enters trigger.
static DRAIN_LAND: CardDef = CardDef {
    name: "Test Drain Land",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[
            Ability::triggered(&Trigger::ENTERS).effect(&delay_carrying(1, Amount::Fixed(2))),
            Ability::triggered(&Trigger::main_phase(Players::You).next())
                .effect(&add_mana_of_kind(you(), ManaColor::Colorless, carried())),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static EMBLEM: CardDef = CardDef {
    name: "Test Emblem",
    faces: &[Face {
        abilities: &[
            Ability::triggered(&Trigger::casts(Players::You, Filter::Any))
                .effect(&lose_life(each_opponent(), 1)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static EMBLEM_LAND: CardDef = CardDef {
    name: "Test Emblem Land",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::triggered(&Trigger::ENTERS).effect(&get_emblem(you(), &EMBLEM))],
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

/// Sphinx of False Conclusions's dies trigger: "if it wasn't a token, create a token that's a
/// copy of it".
static SPHINX: CardDef = CardDef {
    name: "Test Sphinx",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(3),
        toughness: Some(3),
        abilities: &[Ability::triggered(&Trigger::DIES)
            .only_if(&Condition::Not(&Condition::Token(this())))
            .effect(&create_copy(you(), this()))],
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
        abilities: &[Ability::triggered(&Trigger::ENTERS).effect(&gain_life(you(), 1))],
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
            Ability::triggered(&Trigger::ATTACKS).effect(&gain_life(you(), 1)),
            Ability::triggered(&Trigger::DEALS_COMBAT_DAMAGE_TO_A_PLAYER).effect(&draw(you(), 1)),
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

/// "Create a 1/1 Human that's tapped and attacking", put there attacking whatever the step
/// and with nothing chosen for it to attack: the tests declare attackers with no combat.
struct HumanAttacking;

impl CustomBody for HumanAttacking {
    fn resolve<L: Layout>(game: &mut Game<'_, L>, resolution: &mut Resolution<'_, L>) -> Flow {
        let (what, you) = (
            tokens::predefined(game.table(), &HUMAN),
            resolution.controller(),
        );
        let state = game.state();
        let how = TokenEntry {
            tapped: true,
            attacking: state
                .players_in_turn_order(state.defending_players())
                .next(),
        };
        tokens::create(game, you, what, 1, how)?;
        Ok(())
    }
}

/// Adeline's trigger as the card has it: a Human for each opponent, attacking that player
/// or a planeswalker of theirs.
const A_HUMAN_ATTACKING: Effect = for_each_player(each_opponent(), &A_HUMAN_ATTACKING_THEM);
const A_HUMAN_ATTACKING_THEM: Effect = create_attacking_tokens(you(), &HUMAN, 1, that_player());

/// Adeline, Resplendent Cathar: power equal to the creatures you control; whenever you
/// attack, a 1/1 Human tapped and attacking.
static ADELINE: CardDef = CardDef {
    name: "Test Adeline",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(0),
        toughness: Some(4),
        abilities: &[
            Ability::static_ability(&LayerSpec {
                affected: Objects::This,
                condition: None,
                modification: Modification::SetPower(&super::static_abilities::CREATURES),
            }),
            Ability::triggered(&Trigger::attack(Players::You))
                .custom(custom_body::<HumanAttacking>()),
        ],
        ..Face::EMPTY
    }],
    tokens: &[&HUMAN],
};

pub(super) static INSECT: CardDef = CardDef {
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

/// Overlord of the Mistmoors: not a creature while it has a time counter (impending); when
/// it enters, two Insects.
static OVERLORD: CardDef = CardDef {
    name: "Test Overlord",
    faces: &[Face {
        types: Types::ENCHANTMENT.union(Types::CREATURE),
        power: Some(5),
        toughness: Some(5),
        abilities: &[
            Ability::static_ability(&LayerSpec {
                affected: Objects::This,
                condition: Some(Condition::Compare(
                    Amount::Counters(Objects::This, CounterKind::Time),
                    Comparison::AtLeast,
                    Amount::Fixed(1),
                )),
                modification: Modification::RemoveTypes(Types::CREATURE),
            }),
            Ability::triggered(&Trigger::ENTERS).effect(&create_tokens_of_kind(you(), &INSECT, 2)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[&INSECT],
};

/// Staff of the Storyteller: a story counter whenever you create one or more creature
/// tokens. Its second ability makes noncreature tokens, for the test that they don't count.
static STAFF: CardDef = CardDef {
    name: "Test Staff",
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[
            Ability::triggered(&Trigger::creates_tokens(
                Players::You,
                Filter::Types(Types::CREATURE),
            ))
            .effect(&add_counters(this(), CounterKind::Story, 1)),
            Ability::triggered(&Trigger::main_phase(Players::You).next())
                .effect(&create_tokens_of_kind(you(), &MAP, 2)),
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
const CREW: Ability = Ability::activated(Cost::NONE);

/// Swift Reconfiguration: "Enchanted permanent is a Vehicle artifact with crew 5 and it loses
/// all other card types."
static RECONFIG: CardDef = CardDef {
    name: "Test Reconfiguration",
    faces: &[Face {
        types: Types::ENCHANTMENT,
        subtypes: Subtypes::of(&[Subtype::Aura]),
        abilities: &[
            Ability::static_ability(&LayerSpec {
                affected: Objects::AttachedTo,
                condition: None,
                modification: Modification::SetTypes(Types::ARTIFACT),
            }),
            Ability::static_ability(&LayerSpec {
                affected: Objects::AttachedTo,
                condition: None,
                modification: Modification::AddSubtype(Subtype::Vehicle),
            }),
            Ability::static_ability(&LayerSpec {
                affected: Objects::AttachedTo,
                condition: None,
                modification: Modification::grant(0),
            }),
        ],
        grants: &[CREW],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Exile a creature an opponent controls until this leaves the battlefield", with no target:
/// the first there is.
struct ExileAnOpposingCreature;

impl CustomBody for ExileAnOpposingCreature {
    fn resolve<L: Layout>(game: &mut Game<'_, L>, resolution: &mut Resolution<'_, L>) -> Flow {
        let (s, you) = (game.state(), resolution.controller());
        let target = s
            .battlefield()
            .iter()
            .copied()
            .find(|&slot| {
                s.controller(slot) != you && s.derived(slot).types.contains(Types::CREATURE)
            })
            .map(|slot| s.object_ref(slot));
        if let Some(target) = target {
            one_shot::exile_until_leaves(game, target, resolution.source())?;
        }
        Ok(())
    }
}

/// Aang's Iceberg: "exile ... until this leaves the battlefield".
static ICEBERG: CardDef = CardDef {
    name: "Test Iceberg",
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[
            Ability::triggered(&Trigger::ENTERS).custom(custom_body::<ExileAnOpposingCreature>())
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

pub(super) static CARDS: CardTable = CardTable::new(&[
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
pub(super) const DECK: &Decklist = &[
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
pub(super) fn game() -> (GameTable, State, Seat) {
    game_with_seed(7)
}

/// Each player gets a basic land in hand, so every main phase of theirs is a decision
/// (passing priority never plays it) and tests can stop there.
fn game_with_seed(seed: u64) -> (GameTable, State, Seat) {
    let t = GameTable::new(&CARDS, [DECK, DECK]).unwrap();
    let mut s = State::new(&t, seed, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    for player in Seat::BOTH {
        // Any of the copies does.
        let land = s
            .in_candidate_order(s.zone(ZoneKey::Of(player, Zone::Library)))
            .find(|&slot| s.definition_name(&t, slot) == "Test Land")
            .unwrap();
        s.move_object(&mut crate::event::Sink::off(), land, Zone::Hand, Place::Top)
            .unwrap();
    }
    let p = s.turn().starting_player();
    (t, s, p)
}

/// A slot holding `name` in the player's library or hand.
pub(super) fn find<L: Layout>(s: &State<L>, t: &GameTable, player: Seat, name: &str) -> L::Slot {
    // Copies of a card are alike, so any of them does.
    let cards =
        s.zone(ZoneKey::Of(player, Zone::Library)) | s.zone(ZoneKey::Of(player, Zone::Hand));
    s.in_candidate_order(cards)
        .find(|&slot| s.definition_name(t, slot) == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

/// Puts a card onto the battlefield through the pipeline, under its owner's control.
pub(super) fn put(t: &GameTable, s: &mut State, x: &mut Scratch, player: Seat, name: &str) -> Slot {
    let slot = find(s, t, player, name);
    event::move_one(s, t, &mut x.events, slot, Zone::Battlefield, Reason::Effect).unwrap();
    slot
}

/// Changes the game as an ability of the permanent in `slot` does while it resolves: `body`
/// calls the library with the game and that resolution.
pub(super) fn with_game(
    t: &GameTable,
    s: &mut State,
    x: &mut Scratch,
    slot: Slot,
    body: impl FnOnce(&mut Game, &mut Resolution),
) {
    let ability = PendingTrigger {
        of: AbilityRef {
            source: Some(s.object_ref(slot)),
            card: s.copiable(slot).record(t).0,
            controller: s.controller(slot),
            ..AbilityRef::default()
        },
        ..PendingTrigger::default()
    };
    let mut resolution = Resolution::of_trigger(&ability, &NO_ABILITY);
    let mut game = Game::new(s, t, &mut x.events);
    body(&mut game, &mut resolution);
    crate::interpret::finish(&mut game, Ok(())).unwrap();
}

/// Creates `count` tokens of a predefined kind under the control of the resolving ability's
/// controller.
pub(super) fn create_tokens(
    game: &mut Game,
    resolution: &Resolution,
    def: &'static CardDef,
    count: u8,
    how: TokenEntry,
) -> Bitboard<Slot> {
    let what = tokens::predefined(game.table(), def);
    tokens::create(game, resolution.controller(), what, count, how).unwrap()
}

/// Advances to the next decision.
fn decide_with(t: &GameTable, s: &mut State, x: &mut Scratch) -> Decision {
    match next(s, t, x, &mut Replay) {
        Step::Decide(d) => d,
        Step::Over(o) => panic!("the game ended: {o:?}"),
    }
}

/// Puts waiting triggers on the stack and passes until both are empty, returning the
/// decision reached.
pub(super) fn resolve_all(t: &GameTable, s: &mut State, x: &mut Scratch) -> Decision {
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
pub(super) fn advance_to(
    t: &GameTable,
    s: &mut State,
    x: &mut Scratch,
    active: Seat,
    step: TurnStep,
) -> Decision {
    loop {
        let d = decide_with(t, s, x);
        if s.turn().step() == step
            && s.turn().active() == active
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
fn a_copy_in_hand_the_opponent_knows_is_offered_apart_from_one_they_do_not() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    // Any two of the copies do.
    let lands: Vec<Slot> = s
        .in_candidate_order(s.zone(ZoneKey::Of(p, Zone::Library)))
        .filter(|&slot| s.definition_name(&t, slot) == "Test Land")
        .take(2)
        .collect();
    for &land in &lands {
        s.move_object(&mut crate::event::Sink::off(), land, Zone::Hand, Place::Top)
            .unwrap();
    }
    let plays = |s: &mut State, x: &mut Scratch| {
        let d = decide_with(&t, s, x);
        (0..d.len() as u16)
            .filter_map(|i| match d.candidates.option(Choice(i)) {
                Some(Opt::PlayLand(slot)) if s.definition_name(&t, slot) == "Test Land" => {
                    Some(slot)
                }
                _ => None,
            })
            .count()
    };
    assert_eq!(plays(&mut s, &mut x), 1, "copies alike are one candidate");
    s.board().set_known(p.opponent(), lands[1], true);
    assert_eq!(plays(&mut s, &mut x), 2);
}

#[test]
fn an_enters_trigger_resolves_after_the_land_is_played() {
    let (t, mut s, p) = game();
    let mut x = Scratch::recording();
    let land = find(&s, &t, p, "Test Gain Land");
    s.move_object(&mut crate::event::Sink::off(), land, Zone::Hand, Place::Top)
        .unwrap();
    let d = decide_with(&t, &mut s, &mut x);
    let play = (0..d.len() as u16)
        .find(|&i| d.candidates.option(Choice(i)) == Some(Opt::PlayLand(land)))
        .unwrap();
    apply(&mut s, &t, &mut x, &mut Replay, Choice(play));
    assert_eq!(s.triggers().len(), 1);
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p).life(), 21);
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
    with_game(&t, &mut s, &mut x, sphinx, |game, resolution| {
        damage::deal(
            game,
            resolution.source(),
            Recipient::Permanent(resolution.source()),
            3,
            false,
        )
        .unwrap();
    });
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(sphinx).zone(), Zone::Graveyard);
    let token = *s
        .battlefield()
        .iter()
        .find(|&&slot| s.object(slot).is_token())
        .expect("a token copy");
    assert_eq!(s.definition_name(&t, token), "Test Sphinx");
    assert_eq!((s.derived(token).power, s.derived(token).toughness), (3, 3));
    // The copy is a token: when it dies it triggers nothing, and then ceases to exist.
    let token_ref = s.object_ref(token);
    with_game(&t, &mut s, &mut x, token, |game, _| {
        destroy(game, token_ref).unwrap();
    });
    assert!(s.triggers().is_empty());
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(token).zone(), Zone::Nowhere);
    assert!(!s.zone(ZoneKey::Of(p, Zone::Graveyard)).contains(token));
}

#[test]
fn a_token_that_ceases_to_exist_unlinks_the_cards_exiled_with_it() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let values = s.copiable(bear);
    let mut token = None;
    with_game(&t, &mut s, &mut x, bear, |game, resolution| {
        token = tokens::create(
            game,
            resolution.controller(),
            values,
            1,
            TokenEntry::default(),
        )
        .unwrap()
        .sole();
    });
    let token = token.expect("a token copy");
    let land = find(&s, &t, p, "Test Land");
    let land_ref = s.object_ref(land);
    with_game(&t, &mut s, &mut x, token, |game, resolution| {
        linked::exile_linked(game, land_ref, resolution.source()).unwrap();
    });
    assert_eq!(s.memos(land).exiled_with(), Some(token));
    let token_ref = s.object_ref(token);
    with_game(&t, &mut s, &mut x, token, |game, _| {
        destroy(game, token_ref).unwrap();
    });
    // The token ceases to exist (CR 704.5d), and nothing links to its freed slot.
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(token).zone(), Zone::Nowhere);
    assert_eq!(s.object(land).zone(), Zone::Exile);
    assert_eq!(s.memos(land).exiled_with(), None);
}

#[test]
fn an_ability_finds_the_cards_exiled_with_its_source_until_a_new_object_is_there() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let land = find(&s, &t, p, "Test Land");
    let land_ref = s.object_ref(land);
    // An ability of the bear as it is now, which stays that object's wherever it goes.
    let ability = PendingTrigger {
        of: AbilityRef {
            source: Some(s.object_ref(bear)),
            card: s.copiable(bear).record(&t).0,
            controller: p,
            ..AbilityRef::default()
        },
        ..PendingTrigger::default()
    };
    let found = |s: &State| linked::exiled_with(s, ability.of.source.expect("the bear"));
    with_game(&t, &mut s, &mut x, bear, |game, resolution| {
        linked::exile_linked(game, land_ref, resolution.source()).unwrap();
    });
    assert_eq!(found(&s), Bitboard::single(land));

    // The bear dies with the ability still to resolve: it finds the card (CR 607.2a).
    let bear_ref = s.object_ref(bear);
    with_game(&t, &mut s, &mut x, bear, |game, _| {
        destroy(game, bear_ref).unwrap();
    });
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(bear).zone(), Zone::Graveyard);
    assert_eq!(found(&s), Bitboard::single(land));

    // The card returns as a new object (CR 400.7), which exiled nothing: the card in exile
    // stays there, exiled with nothing any more.
    event::move_one(
        &mut s,
        &t,
        &mut x.events,
        bear,
        Zone::Battlefield,
        Reason::Effect,
    )
    .unwrap();
    assert!(found(&s).is_empty());
    assert!(s.exiled_with(bear).is_empty());
    assert_eq!(s.object(land).zone(), Zone::Exile);
    assert_eq!(s.memos(land).exiled_with(), None);
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
        s.object(walker).counters().loyalty,
        3,
        "enters with its loyalty"
    );
    let shrunk_ref = s.object_ref(shrunk);
    let walker_ref = s.object_ref(walker);
    with_game(&t, &mut s, &mut x, hurt, |game, resolution| {
        damage::deal(
            game,
            resolution.source(),
            Recipient::Permanent(resolution.source()),
            2,
            false,
        )
        .unwrap();
        continuous::start(
            game,
            resolution,
            Continuous::Layer(shrunk_ref, LayerChange::ModifyPt(0, -2)),
            Duration::EndOfTurn,
        )
        .unwrap();
        damage::deal(
            game,
            resolution.source(),
            Recipient::Permanent(walker_ref),
            3,
            false,
        )
        .unwrap();
    });
    let healthy_ref = s.object_ref(healthy);
    with_game(&t, &mut s, &mut x, healthy, |game, resolution| {
        damage::deal(
            game,
            resolution.source(),
            Recipient::Permanent(healthy_ref),
            1,
            false,
        )
        .unwrap();
    });
    resolve_all(&t, &mut s, &mut x);
    for slot in [hurt, shrunk, walker] {
        assert_eq!(s.object(slot).zone(), Zone::Graveyard);
    }
    assert_eq!(s.object(healthy).zone(), Zone::Battlefield);
}

#[test]
fn attack_triggers_fire_per_attacker_and_whenever_you_attack_once() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let a = put(&t, &mut s, &mut x, p, "Test Raider");
    let b = put(&t, &mut s, &mut x, p, "Test Raider");
    let adeline = put(&t, &mut s, &mut x, p, "Test Adeline");
    let attackers: Bitboard = [a, b, adeline].into_iter().collect();
    // Each pick of the declaration is marked on its own, with no event.
    for slot in s.in_candidate_order(attackers) {
        s.declare_attacker(slot, p.opponent());
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
    assert_eq!(s.player(p).life(), 22);
    let human = *s
        .battlefield()
        .iter()
        .find(|&&slot| s.definition_name(&t, slot) == "Test Human")
        .unwrap();
    let o = s.object(human);
    assert!(o.is_tapped() && o.is_attacking());
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
    let mut game = Game::new(&mut s, &t, &mut x.events);
    damage::deal(&mut game, source, Recipient::Player(q), 2, false).unwrap();
    assert!(s.triggers().is_empty());
    let mut game = Game::new(&mut s, &t, &mut x.events);
    damage::deal(&mut game, source, Recipient::Player(q), 2, true).unwrap();
    assert_eq!(s.triggers().len(), 1);
    assert_eq!(s.triggers()[0].info.player, q);
    let hand = s.zone(ZoneKey::Of(p, Zone::Hand)).count();
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.zone(ZoneKey::Of(p, Zone::Hand)).count(), hand + 1);
    assert_eq!(s.player(q).life(), 16);
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
        assert_eq!(s.player(q).life(), if won { 20 } else { 17 });
    }
    assert!(flips[0] > 0 && flips[1] > 0, "{flips:?}");
}

/// The trigger-order decisions on the way from the starting player's first main phase to
/// their next one, with two Crypt Lands of theirs and two Watchers of the opponent's.
fn order_decisions(
    tell_apart: impl FnOnce(&mut Game, &mut Resolution),
) -> (Seat, Vec<(Seat, usize)>) {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let a = put(&t, &mut s, &mut x, p, "Test Crypt Land");
    put(&t, &mut s, &mut x, p, "Test Crypt Land");
    put(&t, &mut s, &mut x, p.opponent(), "Test Watcher");
    put(&t, &mut s, &mut x, p.opponent(), "Test Watcher");
    // Done as an ability of the first of the two lands.
    with_game(&t, &mut s, &mut x, a, tell_apart);
    let mut asked = Vec::new();
    let mut turns = 0;
    loop {
        let d = decide_with(&t, &mut s, &mut x);
        if d.kind == DecisionKind::TriggerOrder {
            asked.push((d.player, d.len()));
        }
        if s.turn().step() == TurnStep::PrecombatMain
            && s.turn().active() == p
            && s.stack().is_empty()
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
    let (_, same) = order_decisions(|_, _| {});
    assert!(
        same.is_empty(),
        "equivalent triggers are ordered silently: {same:?}"
    );
    let (p, distinct) = order_decisions(|game, resolution| {
        let source = resolution.source();
        counters::add(game, source, CounterKind::Story, 1).unwrap();
    });
    assert_eq!(distinct, vec![(p, 2)]);
    // Alike in themselves, but one exiled a card until it leaves: which leaves first
    // decides when the card returns (CR 610.3).
    let (p, named) = order_decisions(|game, resolution| {
        let land = resolution.controller().opponent();
        let s = game.state();
        let theirs = s
            .battlefield()
            .iter()
            .copied()
            .find(|&slot| s.controller(slot) == land)
            .map(|slot| s.object_ref(slot))
            .expect("a Watcher");
        one_shot::exile_until_leaves(game, theirs, resolution.source()).unwrap();
    });
    assert_eq!(named, vec![(p, 2)]);
}

/// The triggers a player chooses the next from, of those waiting.
fn firsts(s: &State) -> usize {
    crate::trigger::next_to_stack(s).expect("a trigger").1.len()
}

/// Waiting triggers name their own sources, so they are compared by renaming one's to the
/// other's (§7.2): two abilities that each watch one of two twins leave the twins' own
/// abilities equivalent, and one that watches one twin alone does not.
#[test]
fn triggers_of_twins_are_equivalent_while_the_waiting_triggers_name_both_alike() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let ([first, second], watcher) = twins(&t, &mut s, &mut x, p);
    let trigger = |s: &State, source: Slot, about: Slot| PendingTrigger {
        of: AbilityRef {
            source: Some(s.object_ref(source)),
            card: s.copiable(source).record(&t).0,
            controller: p,
            ..AbilityRef::default()
        },
        info: TriggerInfo {
            object: Some(s.object_ref(about)),
            ..TriggerInfo::default()
        },
    };
    // Each twin's own ability, on its own entering.
    for twin in [first, second] {
        s.queue_trigger(trigger(&s, twin, twin)).unwrap();
    }
    assert_eq!(firsts(&s), 1);

    // A third permanent's ability for each of them: one of each kind to choose from.
    let mut both = s;
    for twin in [first, second] {
        both.queue_trigger(trigger(&both, watcher, twin)).unwrap();
    }
    assert_eq!(firsts(&both), 2);

    // For one of them alone: the twins' abilities are told apart.
    let mut one = s;
    one.queue_trigger(trigger(&one, watcher, first)).unwrap();
    assert_eq!(firsts(&one), 3);

    // A creature that saw itself enter is no twin of one that saw the other.
    let mut crossed = s;
    crossed.take_trigger(1);
    crossed
        .queue_trigger(trigger(&crossed, second, first))
        .unwrap();
    assert_eq!(firsts(&crossed), 2);

    // One of them named off the stack's waiting list: by an ability on the stack.
    let mut stacked = s;
    let tr = trigger(&stacked, watcher, first);
    stacked
        .put_on_stack(
            &mut Sink::off(),
            StackEntry::new(StackItem::Triggered {
                of: AbilityRef {
                    controller: p,
                    ..tr.of
                },
                info: tr.info,
            }),
        )
        .unwrap();
    assert_eq!(firsts(&stacked), 2);
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
    assert_eq!(s.delayed().len(), 1);
    // Created in the precombat main phase: the next main phase is the postcombat one.
    advance_to(&t, &mut s, &mut x, p, TurnStep::PostcombatMain);
    assert!(s.delayed().is_empty());
    assert_eq!(s.player(p).mana()[ManaColor::Colorless], 2);
    advance_to(&t, &mut s, &mut x, p, TurnStep::PrecombatMain);
    assert_eq!(
        s.player(p).mana(),
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
    assert_eq!(s.player(p).emblems().len(), 1);
    // The emblem stays when its creator leaves.
    with_game(&t, &mut s, &mut x, land, |game, resolution| {
        destroy(game, resolution.source()).unwrap();
    });
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
    assert_eq!(s.player(p.opponent()).life(), 19);
}

#[test]
fn whenever_you_create_creature_tokens_triggers_once_per_event() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let staff = put(&t, &mut s, &mut x, p, "Test Staff");
    put(&t, &mut s, &mut x, p, "Test Overlord");
    // Overlord's enters trigger makes two Insects: one event, one story counter.
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.counters_of(staff, CounterKind::Story), 1);
    // Noncreature tokens don't count.
    with_game(&t, &mut s, &mut x, staff, |game, resolution| {
        create_tokens(game, resolution, &MAP, 2, TokenEntry::default());
    });
    assert!(s.triggers().is_empty());
    let insects = s
        .battlefield()
        .iter()
        .filter(|&&slot| s.definition_name(&t, slot) == "Test Insect")
        .count();
    assert_eq!(insects, 2);
}

#[test]
fn lands_enter_tapped_conditionally_or_unless_life_is_paid() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let tapped = |s: &State, slot: Slot| s.object(slot).is_tapped();
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
        s.move_object(
            &mut crate::event::Sink::off(),
            springs,
            Zone::Hand,
            Place::Top,
        )
        .unwrap();
        let d = decide_with(&t, &mut s, &mut x);
        let play = (0..d.len() as u16)
            .find(|&i| d.candidates.option(Choice(i)) == Some(Opt::PlayLand(springs)))
            .unwrap();
        apply(&mut s, &t, &mut x, &mut Replay, Choice(play));
        let d = decide_with(&t, &mut s, &mut x);
        assert_eq!(
            (d.kind, d.prompt.purpose, d.player),
            (DecisionKind::YesNo, Purpose::PayLife, p)
        );
        let answer = if pay { Opt::Yes } else { Opt::No };
        let c = (0..2)
            .find(|&i| d.candidates.option(Choice(i)) == Some(answer))
            .unwrap();
        apply(&mut s, &t, &mut x, &mut Replay, Choice(c));
        assert_eq!(s.object(springs).zone(), Zone::Battlefield);
        assert_eq!(tapped(&s, springs), enters_tapped);
        assert_eq!(s.player(p).life(), life);
    }

    // A player who can't pay isn't asked.
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    s.board().set_life(p, 2);
    let springs = find(&s, &t, p, "Test Springs");
    s.move_object(
        &mut crate::event::Sink::off(),
        springs,
        Zone::Hand,
        Place::Top,
    )
    .unwrap();
    let d = decide_with(&t, &mut s, &mut x);
    let play = (0..d.len() as u16)
        .find(|&i| d.candidates.option(Choice(i)) == Some(Opt::PlayLand(springs)))
        .unwrap();
    apply(&mut s, &t, &mut x, &mut Replay, Choice(play));
    assert_eq!(s.object(springs).zone(), Zone::Battlefield);
    assert!(tapped(&s, springs));
    assert_eq!(s.player(p).life(), 2);
}

#[test]
fn exile_instead_of_the_graveyard_replaces_the_move() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let a = put(&t, &mut s, &mut x, p, "Test Bear");
    let b = put(&t, &mut s, &mut x, p, "Test Bear");
    let a_ref = s.object_ref(a);
    with_game(&t, &mut s, &mut x, b, |game, resolution| {
        continuous::start(
            game,
            resolution,
            Continuous::Move(a_ref, MoveReplacement::ExileInsteadOfGraveyard),
            Duration::EndOfTurn,
        )
        .unwrap();
        destroy(game, a_ref).unwrap();
        destroy(game, resolution.source()).unwrap();
    });
    assert_eq!(s.object(a).zone(), Zone::Exile);
    assert_eq!(s.object(b).zone(), Zone::Graveyard);
}

#[test]
fn impending_counters_and_type_changes_apply_in_layer_four() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let overlord = find(&s, &t, p, "Test Overlord");
    let r = s.object_ref(overlord);
    let land = put(&t, &mut s, &mut x, p, "Test Land");
    let counters = Some((CounterKind::Time, 4));
    let mut game = Game::new(&mut s, &t, &mut x.events);
    zones::put_onto_battlefield(&mut game, r, p, FaceIx::FRONT, counters).unwrap();
    assert_eq!(s.counters_of(overlord, CounterKind::Time), 4);
    assert_eq!(types(&s, overlord), Types::ENCHANTMENT, "not a creature");
    let r = s.object_ref(overlord);
    with_game(&t, &mut s, &mut x, land, |game, _| {
        counters::remove(game, r, CounterKind::Time, 4).unwrap();
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
    s.board().attach(aura, bear);
    crate::layers::refresh(&mut s, &t, &mut x.events).unwrap();
    let d = s.derived(bear);
    assert_eq!(d.types, Types::ARTIFACT);
    assert!(d.subtypes.contains(t.subtypes(), Subtype::Vehicle));
    let grant = d.granted[0].unwrap();
    assert_eq!(grant.card, s.copiable(aura).record(&t).0);
    let granted = &t.def(grant.card).front().grants[grant.grant.index()];
    assert_eq!(granted.kind(), AbilityKind::Activated);
    // A land loses its land types with its land type (CR 205.1a), and keeps the Vehicle.
    let land = put(&t, &mut s, &mut x, p, "Test Land");
    s.board().attach(aura, land);
    crate::layers::refresh(&mut s, &t, &mut x.events).unwrap();
    let subtypes = s.derived(land).subtypes;
    assert_eq!(
        subtypes.iter(t.subtypes()).collect::<Vec<_>>(),
        vec![Subtype::Vehicle]
    );
    assert_eq!(types(&s, bear), Types::CREATURE, "no longer enchanted");
    s.board().attach(aura, bear);
    crate::layers::refresh(&mut s, &t, &mut x.events).unwrap();
    // Crewed: an artifact creature until end of turn, by a later timestamp.
    let r = s.object_ref(bear);
    with_game(&t, &mut s, &mut x, bear, |game, resolution| {
        continuous::start(
            game,
            resolution,
            Continuous::Layer(r, LayerChange::AddTypes(Types::CREATURE)),
            Duration::EndOfTurn,
        )
        .unwrap();
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
    s.board().attach(aura, bear);
    crate::layers::refresh(&mut s, &t, &mut x.events).unwrap();
    let r = s.object_ref(bear);
    with_game(&t, &mut s, &mut x, aura, |game, _| {
        destroy(game, r).unwrap();
        let r = game.state().object_ref(bear);
        zones::put_onto_battlefield(game, r, p, FaceIx::FRONT, None).unwrap();
    });
    assert_eq!(s.object(aura).attached_to(), None);
    assert_eq!(types(&s, bear), Types::CREATURE);
    decide_with(&t, &mut s, &mut x);
    assert_eq!(s.object(aura).zone(), Zone::Graveyard);

    // On a token, the token ceases to exist and the Aura goes, in one check.
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let aura = put(&t, &mut s, &mut x, p, "Test Reconfiguration");
    let mut token = Bitboard::EMPTY;
    with_game(&t, &mut s, &mut x, aura, |game, resolution| {
        token = create_tokens(game, resolution, &MAP, 1, TokenEntry::default());
    });
    let token = token.sole().unwrap();
    s.board().attach(aura, token);
    crate::layers::refresh(&mut s, &t, &mut x.events).unwrap();
    let r = s.object_ref(token);
    with_game(&t, &mut s, &mut x, aura, |game, _| {
        destroy(game, r).unwrap();
    });
    decide_with(&t, &mut s, &mut x);
    assert_eq!(s.object(token).zone(), Zone::Nowhere);
    assert_eq!(s.object(aura).zone(), Zone::Graveyard);
}

#[test]
fn a_stolen_creatures_dies_trigger_belongs_to_its_last_controller() {
    // CR 603.3a: "the player who controlled its source at the time it triggered"; the last-known
    // information of a permanent that left is how it last existed on the battlefield.
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let q = p.opponent();
    let sphinx = put(&t, &mut s, &mut x, q, "Test Sphinx");
    let thief = put(&t, &mut s, &mut x, p, "Test Bear");
    let r = s.object_ref(sphinx);
    with_game(&t, &mut s, &mut x, thief, |game, resolution| {
        continuous::gain_control(game, resolution, r, Duration::Indefinite).unwrap();
        destroy(game, r).unwrap();
    });
    assert_eq!(s.triggers().len(), 1);
    assert_eq!(s.triggers()[0].of.controller, p);
    assert!(s.zone(ZoneKey::Of(q, Zone::Graveyard)).contains(sphinx));
    resolve_all(&t, &mut s, &mut x);
    let copy = *s
        .battlefield()
        .iter()
        .find(|&&slot| s.object(slot).is_token())
        .expect("a token copy");
    assert_eq!((s.object(copy).owner(), s.controller(copy)), (p, p));
}

#[test]
fn removing_a_counter_kind_an_object_lacks_changes_nothing() {
    let (t, mut s, p) = game();
    let mut x = Scratch::recording();
    let staff = put(&t, &mut s, &mut x, p, "Test Staff");
    let r = s.object_ref(staff);
    with_game(&t, &mut s, &mut x, staff, |game, _| {
        counters::add(game, r, CounterKind::Story, 2).unwrap();
    });
    x.events.drain();
    with_game(&t, &mut s, &mut x, staff, |game, _| {
        counters::remove(game, r, CounterKind::Time, 1).unwrap();
    });
    assert_eq!(s.counters_of(staff, CounterKind::Story), 2);
    assert!(x.events.events().is_empty());
}

/// CR 122.1: counters of different names are different counters, so putting one kind on a
/// permanent leaves its others alone, whichever kinds they are.
#[test]
fn a_permanent_has_counters_of_several_kinds_at_once() {
    let (t, mut s, p) = game();
    let mut x = Scratch::recording();
    let staff = put(&t, &mut s, &mut x, p, "Test Staff");
    let r = s.object_ref(staff);
    with_game(&t, &mut s, &mut x, staff, |game, _| {
        counters::add(game, r, CounterKind::Story, 2).unwrap();
        counters::add(game, r, CounterKind::Time, 3).unwrap();
        counters::add(game, r, CounterKind::Loyalty, 1).unwrap();
        counters::remove(game, r, CounterKind::Story, 1).unwrap();
        // No more than it has (CR 609.3).
        counters::remove(game, r, CounterKind::Time, 5).unwrap();
    });
    let counts = [CounterKind::Story, CounterKind::Time, CounterKind::Loyalty]
        .map(|kind| s.counters_of(staff, kind));
    assert_eq!(counts, [1, 0, 1]);
    let removed: Vec<(CounterKind, u8)> = (x.events.events().iter())
        .filter_map(|e| match *e {
            Event::CountersRemoved { kind, amount, .. } => Some((kind, amount)),
            _ => None,
        })
        .collect();
    assert_eq!(removed, [(CounterKind::Story, 1), (CounterKind::Time, 3)]);
}

/// Two creatures of one card are one candidate only while the same effects are on them
/// (§7.2): the same size today by a bonus that ends with the turn and by one that does
/// not, they lead to different games.
#[test]
fn creatures_alike_today_by_effects_that_end_apart_are_not_interchangeable() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let [first, second] = [(); 2].map(|()| put(&t, &mut s, &mut x, p, "Test Bear"));
    let source = put(&t, &mut s, &mut x, p, "Test Adeline");
    let [this_turn, for_good] = [first, second]
        .map(|slot| Continuous::Layer(s.object_ref(slot), LayerChange::ModifyPt(1, 1)));
    // The same bonus for as long on each: still one candidate.
    with_game(&t, &mut s, &mut x, source, |game, resolution| {
        continuous::start(game, resolution, this_turn, Duration::EndOfTurn).unwrap();
        continuous::start(game, resolution, for_good, Duration::EndOfTurn).unwrap();
    });
    assert!(crate::engine::interchangeable(&s, first, second));
    with_game(&t, &mut s, &mut x, source, |game, resolution| {
        continuous::start(game, resolution, this_turn, Duration::EndOfTurn).unwrap();
        continuous::start(game, resolution, for_good, Duration::Indefinite).unwrap();
    });
    assert_eq!(s.derived(first), s.derived(second));
    assert!(!crate::engine::interchangeable(&s, first, second));
}

/// Two Bears of one player on the battlefield, which nothing tells apart, and a third
/// permanent of theirs to be the source of what is done to one of them.
fn twins(t: &GameTable, s: &mut State, x: &mut Scratch, p: Seat) -> ([Slot; 2], Slot) {
    let twins = [(); 2].map(|()| put(t, s, x, p, "Test Bear"));
    let other = put(t, s, x, p, "Test Staff");
    assert!(crate::engine::interchangeable(s, twins[0], twins[1]));
    (twins, other)
}

/// Two permanents of one card are one candidate only while no other record of the state
/// names one of them (§7.2). One that exiled a card until it leaves returns the card as
/// it does ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103): "Some one-shot
/// effects cause an object to change zones 'until' a specified event occurs."), and its
/// twin returns none, or another.
#[test]
fn a_permanent_that_exiled_a_card_until_it_leaves_is_not_interchangeable_with_its_twin() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let ([first, second], _) = twins(&t, &mut s, &mut x, p);
    let q = p.opponent();
    let [theirs, another] = ["Test Bear", "Test Trainer"].map(|name| {
        let slot = put(&t, &mut s, &mut x, q, name);
        s.object_ref(slot)
    });
    with_game(&t, &mut s, &mut x, first, |game, resolution| {
        one_shot::exile_until_leaves(game, theirs, resolution.source()).unwrap();
    });
    assert_eq!(s.exiled_until(first).count(), 1);
    assert!(s.exiled_until(second).is_empty());
    assert_eq!(s.signature(first), s.signature(second));
    assert!(!crate::engine::interchangeable(&s, first, second));
    // Each exiled a card, and not the same one.
    with_game(&t, &mut s, &mut x, second, |game, resolution| {
        one_shot::exile_until_leaves(game, another, resolution.source()).unwrap();
    });
    assert!(!crate::engine::interchangeable(&s, first, second));
}

/// The same for the cards exiled with a permanent, which a linked ability of it finds
/// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a): "The second ability
/// refers only to cards in the exile zone that were put there as a result of an
/// instruction to exile them in the first ability.").
#[test]
fn a_permanent_with_a_card_exiled_with_it_is_not_interchangeable_with_its_twin() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let ([first, second], _) = twins(&t, &mut s, &mut x, p);
    let [land, another] = ["Test Land", "Test Coast"].map(|name| {
        let slot = find(&s, &t, p, name);
        s.object_ref(slot)
    });
    with_game(&t, &mut s, &mut x, first, |game, resolution| {
        linked::exile_linked(game, land, resolution.source()).unwrap();
    });
    assert_eq!(s.exiled_with(first).count(), 1);
    assert_eq!(s.signature(first), s.signature(second));
    assert!(!crate::engine::interchangeable(&s, first, second));
    with_game(&t, &mut s, &mut x, second, |game, resolution| {
        linked::exile_linked(game, another, resolution.source()).unwrap();
    });
    assert!(!crate::engine::interchangeable(&s, first, second));
}

/// A creature with a permanent attached that changes nothing of it today and its twin
/// lead to different games: the attachment goes where its permanent goes
/// ([CR 704.5m](https://yawgatog.com/resources/magic-rules/#R7045m): "If an Aura is
/// attached to an illegal object or player, or is not attached to an object or player,
/// that Aura is put into its owner's graveyard.").
#[test]
fn a_permanent_with_something_attached_is_not_interchangeable_with_its_twin() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let ([first, second], attachment) = twins(&t, &mut s, &mut x, p);
    s.board().attach(attachment, first);
    crate::layers::refresh(&mut s, &t, &mut x.events).unwrap();
    assert_eq!(s.signature(first), s.signature(second));
    assert!(!crate::engine::interchangeable(&s, first, second));
}

/// A creature that an ability on the stack will act on without targeting it, one waiting
/// to be put there or a delayed one, and its twin lead to different games: the ability
/// finds the object its event involved
/// ([CR 603.7c](https://yawgatog.com/resources/magic-rules/#R6037c): "A delayed triggered
/// ability that refers to a particular object still affects it even if the object changes
/// characteristics.").
#[test]
fn a_permanent_a_triggered_ability_names_is_not_interchangeable_with_its_twin() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let ([first, second], source) = twins(&t, &mut s, &mut x, p);
    let trigger = |s: &State, about: Slot| PendingTrigger {
        of: AbilityRef {
            source: Some(s.object_ref(source)),
            card: s.copiable(source).record(&t).0,
            controller: p,
            ..AbilityRef::default()
        },
        info: TriggerInfo {
            object: Some(s.object_ref(about)),
            ..TriggerInfo::default()
        },
    };

    let mut on_the_stack = s;
    let tr = trigger(&on_the_stack, first);
    on_the_stack
        .put_on_stack(
            &mut Sink::off(),
            StackEntry::new(StackItem::Triggered {
                of: tr.of,
                info: tr.info,
            }),
        )
        .unwrap();
    assert_eq!(
        on_the_stack.signature(first),
        on_the_stack.signature(second)
    );
    assert!(!crate::engine::interchangeable(
        &on_the_stack,
        first,
        second
    ));

    let mut waiting = s;
    waiting.queue_trigger(trigger(&waiting, second)).unwrap();
    assert!(!crate::engine::interchangeable(&waiting, first, second));

    let mut delayed = s;
    delayed.delay_trigger(trigger(&delayed, first)).unwrap();
    assert!(!crate::engine::interchangeable(&delayed, first, second));

    // The source of a waiting ability and its twin, too.
    let mut sources = s;
    let own = PendingTrigger {
        of: AbilityRef {
            source: Some(sources.object_ref(first)),
            ..trigger(&sources, source).of
        },
        ..trigger(&sources, source)
    };
    sources.queue_trigger(own).unwrap();
    assert!(!crate::engine::interchangeable(&sources, first, second));
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
    with_game(&t, &mut s, &mut x, mine, |game, resolution| {
        continuous::gain_control(game, resolution, r, Duration::Indefinite).unwrap();
    });
    assert_eq!(s.controller(theirs), p);
    assert_eq!(s.object(theirs).owner(), q);
    assert!(s.object(theirs).is_summoning_sick());
    assert_eq!(s.derived(mine).power, 2, "Adeline counts it");
    // It untaps in its new controller's untap step and goes to its owner's graveyard.
    s.board().set_tapped(theirs, true);
    advance_to(&t, &mut s, &mut x, q, TurnStep::PrecombatMain);
    assert!(s.object(theirs).is_tapped(), "not in its owner's");
    advance_to(&t, &mut s, &mut x, p, TurnStep::PrecombatMain);
    assert!(!s.object(theirs).is_tapped());
    assert_eq!(s.controller(theirs), p, "an indefinite effect lasts");
    with_game(&t, &mut s, &mut x, mine, |game, _| {
        destroy(game, r).unwrap();
    });
    assert_eq!(s.object(theirs).zone(), Zone::Graveyard);
    assert!(s.zone(ZoneKey::Of(q, Zone::Graveyard)).contains(theirs));
    assert!(s.effects().is_empty(), "the effect ended with its object");
}

#[test]
fn an_effect_keeps_its_source_card_once_the_source_is_hidden_again() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let q = p.opponent();
    let theirs = put(&t, &mut s, &mut x, q, "Test Bear");
    let mine = put(&t, &mut s, &mut x, p, "Test Adeline");
    let r = s.object_ref(theirs);
    with_game(&t, &mut s, &mut x, mine, |game, resolution| {
        continuous::gain_control(game, resolution, r, Duration::Indefinite).unwrap();
    });
    // The source goes into its owner's library, whose shuffle may put another card in its
    // slot (`zone::rebind`), as the deal decides what a hidden slot holds.
    event::move_one(
        &mut s,
        &t,
        &mut x.events,
        mine,
        Zone::Library,
        Reason::Effect,
    )
    .unwrap();
    // Any card of another name does.
    let other = s
        .in_candidate_order(s.zone(ZoneKey::Of(p, Zone::Library)))
        .find(|&slot| s.card_behind(slot) != s.card_behind(mine))
        .unwrap();
    s.board().swap_cards(mine, other);
    let turn = s.turn().number() + 1;
    s.set_profile(Profile {
        breakpoint: Some(Breakpoint {
            turn,
            step: TurnStep::Upkeep,
        }),
        ..Profile::EXACT
    });
    advance_to(&t, &mut s, &mut x, q, TurnStep::Upkeep);
    assert_eq!(s.controller(theirs), p);
    assert_eq!(
        s.off_board(&t).as_deref(),
        Some("an effect of Test Adeline on Test Bear lasting indefinitely")
    );
}

#[test]
fn granted_keywords_end_at_cleanup_and_until_your_next_turn_lasts_through_theirs() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let r = s.object_ref(bear);
    with_game(&t, &mut s, &mut x, bear, |game, resolution| {
        continuous::start(
            game,
            resolution,
            Continuous::Layer(r, LayerChange::AddKeywords(Keywords::FIRST_STRIKE)),
            Duration::EndOfTurn,
        )
        .unwrap();
        continuous::start(
            game,
            resolution,
            Continuous::Layer(r, LayerChange::ModifyPt(-2, 0)),
            Duration::UntilYourNextTurn(p),
        )
        .unwrap();
        counters::add(game, r, CounterKind::PlusOne, 1).unwrap();
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
    assert_eq!(s.player(p).life(), 21);
    let values = s.copiable(trainer).except_one_one();
    let mut token = Bitboard::EMPTY;
    with_game(&t, &mut s, &mut x, trainer, |game, resolution| {
        token = tokens::create(
            game,
            resolution.controller(),
            values,
            1,
            TokenEntry::default(),
        )
        .unwrap();
    });
    let token = token.sole().unwrap();
    assert_eq!((s.derived(token).power, s.derived(token).toughness), (1, 1));
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p).life(), 22, "the copy's enters trigger");
}

#[test]
fn an_object_exiled_until_its_exiler_leaves_returns_under_its_owners_control() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let q = p.opponent();
    let bear = put(&t, &mut s, &mut x, q, "Test Bear");
    let iceberg = put(&t, &mut s, &mut x, p, "Test Iceberg");
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(bear).zone(), Zone::Exile);
    let r = s.object_ref(iceberg);
    with_game(&t, &mut s, &mut x, iceberg, |game, _| {
        destroy(game, r).unwrap();
    });
    assert_eq!(s.object(bear).zone(), Zone::Battlefield);
    assert_eq!(s.controller(bear), q);
    assert_eq!(s.memos(bear).exiled_until(), None);

    // CR 610.3b: if the exiler leaves before its trigger resolves, nothing is exiled.
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let q = p.opponent();
    let bear = put(&t, &mut s, &mut x, q, "Test Bear");
    let iceberg = put(&t, &mut s, &mut x, p, "Test Iceberg");
    let r = s.object_ref(iceberg);
    with_game(&t, &mut s, &mut x, iceberg, |game, _| {
        destroy(game, r).unwrap();
    });
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.object(bear).zone(), Zone::Battlefield);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "derived read while dirty")]
fn reading_derived_after_a_raw_change_without_a_refresh_panics() {
    let (t, mut s, p) = game();
    let bear = find(&s, &t, p, "Test Bear");
    s.move_object(
        &mut crate::event::Sink::off(),
        bear,
        Zone::Battlefield,
        Place::Top,
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
        let mut s = State::new(&t, seed, Profile::EXACT).unwrap();
        let mut rng = crate::rng::SplitMix64::new(seed);
        let outcome = super::play(
            &t,
            &mut s,
            |_, d| Choice(rng.below(d.len() as u64) as u16),
            |_, _| {},
        );
        assert!(
            matches!(outcome, Outcome::Win(_) | Outcome::Draw),
            "{outcome:?}"
        );
    }
}

/// Adeline's trigger as the card has it: the Human attacks what its controller chooses.
static CATHAR: CardDef = CardDef {
    name: "Test Cathar",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::triggered(&Trigger::attack(Players::You)).effect(&A_HUMAN_ATTACKING)],
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
    let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let mut x = Scratch::new();
    let cathar = find(&s, &t, Seat::P0, "Test Cathar");
    s.board().place(cathar, Zone::Battlefield).unwrap();
    let walker = put(&t, &mut s, &mut x, Seat::P1, "Test Walker");
    let mut declared = false;
    let human = loop {
        let human = s
            .battlefield()
            .iter()
            .copied()
            .find(|&slot| s.definition_name(&t, slot) == "Test Human");
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
                    .find(|&slot| paused.definition_name(&t, slot) == "Test Human")
                    .expect("the Human on the battlefield as it is asked about");
                assert_eq!(
                    (d.prompt, d.about),
                    (
                        Prompt::new(Purpose::Attack).part(Part::Aim),
                        crate::Bitboard::single(shown)
                    )
                );
                let o = paused.object(shown);
                assert!(o.is_attacking() && o.is_tapped());
                assert_eq!(o.attack_target(), Some(AttackTarget::Player(Seat::P1)));
                pick(Opt::Defender(Some(walker)))
            }
            _ => Choice(0),
        };
        apply(&mut s, &t, &mut x, &mut Replay, choice);
    };
    let o = s.object(human);
    assert!(o.is_attacking() && o.is_tapped());
    assert_eq!(o.attack_target(), Some(AttackTarget::Planeswalker(walker)));
    assert_eq!(
        s.object(cathar).attack_target(),
        Some(AttackTarget::Player(Seat::P1))
    );
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
        abilities: &[Ability::triggered(&Trigger::ENTERS)
            .targeting(&[TargetSpec::one(
                TargetKind::Permanent,
                Filter::All(&[Filter::Types(Types::ARTIFACT), Filter::ManaValueAtMostX]),
            )])
            .only_if(&Condition::Paid(CastingKind::Kicker))
            .effect(&modify(
                target(),
                Modification::Control,
                beryl_card::Duration::Indefinite,
            ))],
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
        let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
        s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
        let mut scratch = Scratch::new();
        let thief = find(&s, &t, Seat::P0, "Test Thief");
        s.board().place(thief, Zone::Hand).unwrap();
        for _ in 0..4 {
            let land = find(&s, &t, Seat::P0, "Test Land");
            s.board().place(land, Zone::Battlefield).unwrap();
        }
        let rocks = ["Test Rock 1", "Test Rock 3"].map(|name| {
            let rock = find(&s, &t, Seat::P1, name);
            s.board().place(rock, Zone::Battlefield).unwrap();
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
            let name = s.definition_name(&t, rock);
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

pub(super) static PRODIGY_CARDS: CardTable = CardTable::new(&[&LAND, &PRODIGY]);

/// [CR 712.6d](https://yawgatog.com/resources/magic-rules/#R7126d): with its back face up, a
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
    let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let prodigy = find(&s, &t, Seat::P0, "Test Prodigy");
    s.board().place(prodigy, Zone::Battlefield).unwrap();
    s.board().set_face(prodigy, FaceIx::BACK);
    assert!(!Filter::ManaValueAtMost(1).matches(&s, &t, prodigy, Seat::P0));
    assert!(Filter::ManaValueAtMost(2).matches(&s, &t, prodigy, Seat::P0));
    let mut x = Scratch::new();
    crate::layers::refresh(&mut s, &t, &mut x.events).unwrap();
    let mut copy = None;
    with_game(&t, &mut s, &mut x, prodigy, |game, _| {
        let values = game.state().copiable(prodigy);
        copy = tokens::create(game, Seat::P1, values, 1, TokenEntry::default())
            .unwrap()
            .sole();
    });
    let copy = copy.expect("a token copy");
    assert_eq!(s.object(copy).face(), FaceIx::BACK);
    assert!(Filter::ManaValueAtMost(0).matches(&s, &t, copy, Seat::P1));
}
