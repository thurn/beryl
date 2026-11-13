//! The positions generated scenarios start from: basic lands, the designated props and the
//! card, which the position holds only where the card starts.

use beryl_engine::TurnStep;
use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Cost, Face, FaceIx, Players, TriggerEvent, Types,
};
use beryl_engine::state::CounterKind;
use beryl_scenario::{Action, P0, P1, Permanent, PlayerSetup, Scenario};

use super::PREFIX;
use super::play::Stimulus;
use super::witness::Witness;
use crate::{names, props};

/// The turn a scenario's main phase is in, player 0 active.
pub(super) const TURN: u16 = 3;

/// The last turn a run plays into: the opponent's, where its "each opponent's" steps are.
pub(super) const LAST_TURN: u16 = TURN + 1;

/// Player 0's lands: three each of Plains and Island and two Swamps, enough to cast any card
/// of the pool and pay an "unless" or an offspring beside it.
const LANDS: &[&str] = &[
    names::PLAINS,
    names::PLAINS,
    names::PLAINS,
    names::ISLAND,
    names::ISLAND,
    names::ISLAND,
    names::SWAMP,
    names::SWAMP,
];

/// Player 0's other cards in hand: a land to discard, a sorcery to cast, which a "whenever
/// you cast" ability waits for and a counterspell answers, a creature spell for a response
/// that wants one, and a blue card to exile for an evoke.
const HAND: &[&str] = &[names::PLAINS, props::SORCERY, props::SMALL_CREATURE];

/// The opponent's permanents: one of each kind a target or a sacrifice may want, among them a
/// tapped creature of mana value 2. No planeswalker: neither oracle's exporter can say which
/// a creature put onto the battlefield attacking attacks.
const OPPONENT_PERMANENTS: &[Permanent] = &[
    Permanent::new(props::CREATURE),
    Permanent::new(props::SMALL_CREATURE).tapped(),
    Permanent::new(props::ARTIFACT),
    Permanent::new(props::NONBASIC_LAND),
    Permanent::new(names::ISLAND),
    Permanent::new(names::ISLAND),
];

/// The opponent's hand: a look at it, or a discard from it, finds a choice.
const OPPONENT_HAND: &[&str] = &[props::INSTANT, props::REMOVAL, names::ISLAND];

/// Each library, top first: lands and spells for a draw, a dig or a search to find, and few
/// enough that drawing seven, as an ultimate may, finds it empty.
const LIBRARY: &[&str] = &[
    names::ISLAND,
    props::ARTIFACT,
    names::PLAINS,
    props::CREATURE,
    names::ISLAND,
    props::SORCERY,
];

/// The basic lands a mana source of the card's pays beside, one at a time: whichever its
/// owner taps, the card pays the rest.
const BESIDE: [&str; 2] = [names::ISLAND, names::PLAINS];

/// Where a scenario's card starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Place {
    Hand,
    /// On the battlefield with this face up.
    Battlefield(FaceIx),
    Graveyard,
    /// On the battlefield with this face up, a mana ability's, beside this basic land, the
    /// only other mana its owner has.
    Mana(FaceIx, &'static str),
}

impl Place {
    /// The places a scenario of `def` starts from.
    pub(super) fn of(def: &'static CardDef) -> Vec<Place> {
        let mut places = vec![Place::Hand, Place::Battlefield(FaceIx::FRONT)];
        if def.faces.get(1).is_some_and(|back| back.transformed) {
            places.push(Place::Battlefield(FaceIx::BACK));
        }
        places.push(Place::Graveyard);
        for (f, face) in def.faces.iter().enumerate() {
            let makes_mana = face.abilities.iter().any(|a| a.kind() == AbilityKind::Mana);
            if makes_mana && is_permanent(face) {
                let face = if f == 0 { FaceIx::FRONT } else { FaceIx::BACK };
                places.extend(BESIDE.map(|land| Place::Mana(face, land)));
            }
        }
        places
    }

    pub(super) fn words(self) -> String {
        let face = |face: FaceIx| {
            if face == FaceIx::FRONT {
                ""
            } else {
                " transformed"
            }
        };
        match self {
            Place::Hand => "from hand".to_owned(),
            Place::Battlefield(f) => format!("on the battlefield{}", face(f)),
            Place::Graveyard => "from the graveyard".to_owned(),
            Place::Mana(f, land) => format!("paying beside {land}{}", face(f)),
        }
    }
}

fn is_permanent(face: &Face) -> bool {
    face.types.intersects(
        Types::ARTIFACT
            .union(Types::CREATURE)
            .union(Types::ENCHANTMENT)
            .union(Types::LAND)
            .union(Types::PLANESWALKER),
    )
}

/// `names` without the card `except`.
fn without(names: &[&'static str], except: &str) -> Vec<&'static str> {
    names.iter().copied().filter(|&n| n != except).collect()
}

fn leak<T>(v: Vec<T>) -> &'static [T] {
    Vec::leak(v)
}

/// Whether a face has an ability that triggers before its controller's precombat main phase
/// begins, in their own turn: a scenario that starts there with the card on the battlefield
/// would skip it, where the oracles play turn 3's beginning phase first.
fn triggers_before_main(face: &Face) -> bool {
    face.abilities.iter().any(|a| {
        a.trigger().is_some_and(|trigger| {
            !trigger.delayed
                && !matches!(trigger.whose, Players::EachOpponent)
                && match trigger.event {
                    TriggerEvent::Step(step) => (step as u8) < TurnStep::PrecombatMain as u8,
                    TriggerEvent::MainPhase => true,
                    _ => false,
                }
        })
    })
}

/// Whether the card starts on the battlefield in `place` with an ability that triggers before
/// the main phase, so that its scenarios play the beginning phase first.
pub(super) fn early(def: &'static CardDef, place: Place) -> bool {
    match place {
        Place::Battlefield(f) | Place::Mana(f, _) => triggers_before_main(def.face(f)),
        Place::Hand | Place::Graveyard => false,
    }
}

/// The card as a permanent with face `f` up: a planeswalker with the loyalty its costliest
/// ability takes, and a permanent whose ability removes a counter with one of that kind.
fn permanent(def: &'static CardDef, f: FaceIx) -> Permanent {
    let face = def.face(f);
    let mut permanent = Permanent::new(def.name);
    if f != FaceIx::FRONT {
        permanent = permanent.transformed();
    }
    let costs = || face.abilities.iter().map(Ability::cost);
    let most = costs()
        .filter_map(Cost::loyalty_change)
        .map(|change| -i16::from(change))
        .max();
    if let (Some(most), Some(printed)) = (most, face.loyalty)
        && most > i16::from(printed)
    {
        permanent = permanent.counters(CounterKind::Loyalty, most as u8);
    }
    if let Some(kind) = costs().find_map(Cost::removes) {
        permanent = permanent.counters(kind, 1);
    }
    permanent
}

/// The position a scenario of `def` starting in `place` begins from, before any action, with
/// what `witness` adds.
pub(super) fn start(def: &'static CardDef, place: Place, witness: Option<&Witness>) -> Scenario {
    let card = def.name;
    let lands = match place {
        Place::Mana(_, land) => vec![land],
        _ => without(LANDS, card),
    };
    let mut battlefield: Vec<Permanent> = lands.into_iter().map(Permanent::new).collect();
    let mut hand = without(HAND, card);
    let mut opponent_added: &[Permanent] = &[];
    if let Some(witness) = witness {
        battlefield.extend_from_slice(witness.mine);
        hand.extend_from_slice(witness.hand);
        opponent_added = witness.theirs;
    }
    let mut graveyard = without(&[props::INSTANT], card);
    match place {
        Place::Hand => hand.insert(0, card),
        Place::Battlefield(f) | Place::Mana(f, _) => battlefield.push(permanent(def, f)),
        Place::Graveyard => graveyard.insert(0, card),
    }
    let library = leak(without(LIBRARY, card));
    let early = early(def, place);
    // XMage untaps the permanents of the player active as a scenario starts, so in the
    // opponent's turn theirs start untapped.
    let opponent: Vec<Permanent> = OPPONENT_PERMANENTS
        .iter()
        .chain(opponent_added)
        .filter(|p| p.card != card)
        .map(|&p| Permanent {
            tapped: p.tapped && !early,
            ..p
        })
        .collect();
    let at = if early {
        (TURN - 1, P1, TurnStep::PostcombatMain)
    } else {
        (TURN, P0, TurnStep::PrecombatMain)
    };
    let mut scenario = Scenario::new(PREFIX).at(at.0, at.1, at.2);
    scenario.setup.players = [
        PlayerSetup {
            life: 20,
            hand: leak(hand),
            battlefield: &[],
            permanents: leak(battlefield),
            graveyard: leak(graveyard),
            exile: &[],
            library,
        },
        PlayerSetup {
            life: 20,
            hand: leak(without(OPPONENT_HAND, card)),
            battlefield: &[],
            permanents: leak(opponent),
            graveyard: leak(without(&[props::SORCERY], card)),
            exile: &[],
            library,
        },
    ];
    scenario
}

/// What a scenario does before its opening: for a card on the battlefield with an ability
/// that triggers in its controller's beginning phase, play that phase from the turn before,
/// resolving what triggers.
pub(super) fn preamble(def: &'static CardDef, place: Place) -> Vec<Stimulus> {
    if early(def, place) {
        [TurnStep::Upkeep, TurnStep::PrecombatMain]
            .map(|step| Stimulus::Act(Action::AdvanceTo { turn: TURN, step }))
            .to_vec()
    } else {
        Vec::new()
    }
}
