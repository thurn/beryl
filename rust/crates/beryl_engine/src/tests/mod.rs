//! Engine tests, driven through `next` and `apply` with test-only cards.

mod banned;
mod game;
mod invariants;
mod mulligan;
mod rules;

use std::hash::{DefaultHasher, Hash, Hasher};

use crate::card::{
    Ability, CardDef, Face, ManaColor, ManaSet, Subtype, Subtypes, Supertypes, Types,
};
use crate::{
    CardTable, Choice, Decision, DecisionKind, Decklist, GameTable, Observation, Opt, Outcome,
    Replay, Scratch, State, Step, apply, next,
};

pub(crate) static LAND: CardDef = CardDef {
    name: "Test Land",
    faces: &[Face {
        supertypes: Supertypes::BASIC,
        types: Types::LAND,
        subtypes: Subtypes::of(&[Subtype::Plains]),
        abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::White))],
        ..Face::EMPTY
    }],
};

pub(crate) static DUAL: CardDef = CardDef {
    name: "Test Dual",
    faces: &[Face {
        types: Types::LAND,
        subtypes: Subtypes::of(&[Subtype::Plains, Subtype::Island]),
        abilities: &[
            Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
            Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        ],
        ..Face::EMPTY
    }],
};

pub(crate) static LEGEND: CardDef = CardDef {
    name: "Test Legend",
    faces: &[Face {
        supertypes: Supertypes::LEGENDARY,
        types: Types::LAND,
        ..Face::EMPTY
    }],
};

/// Nonland cards nothing can cast yet: they only sit in hands and libraries.
pub(crate) static SPELL_A: CardDef = CardDef {
    name: "Test Spell A",
    faces: &[Face {
        types: Types::SORCERY,
        ..Face::EMPTY
    }],
};

pub(crate) static SPELL_B: CardDef = CardDef {
    name: "Test Spell B",
    faces: &[Face {
        types: Types::INSTANT,
        ..Face::EMPTY
    }],
};

pub(crate) static CARDS: CardTable = CardTable::new(&[&LAND, &DUAL, &LEGEND, &SPELL_A, &SPELL_B]);

pub(crate) const LANDS: &Decklist = &[(40, "Test Land")];
pub(crate) const MIXED: &Decklist = &[(20, "Test Land"), (20, "Test Spell A")];

pub(crate) fn table(decks: [&Decklist; 2]) -> GameTable {
    GameTable::new(&CARDS, decks).unwrap()
}

pub(crate) fn fingerprint(s: &State) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Plays the first option of every decision but priority, where it plays a land if it can:
/// the starting player plays first, and everyone keeps.
pub(crate) fn land_go(_obs: Observation<'_>, d: &Decision) -> Choice {
    match d.kind {
        DecisionKind::Priority => Choice((d.len() - 1) as u8),
        _ => Choice(0),
    }
}

/// Plays a game to the end, calling `watch` before every decision.
pub(crate) fn play(
    t: &GameTable,
    s: &mut State,
    mut agent: impl FnMut(Observation<'_>, &Decision) -> Choice,
    mut watch: impl FnMut(&State, &Decision),
) -> Outcome {
    let mut x = Scratch::new();
    loop {
        match next(s, t, &mut x, &mut Replay) {
            Step::Over(outcome) => return outcome,
            Step::Decide(d) => {
                watch(s, &d);
                let c = agent(Observation::new(s, d.player), &d);
                apply(s, t, &mut x, &mut Replay, c);
            }
        }
    }
}

/// Advances to the next decision, which must exist.
pub(crate) fn decide(t: &GameTable, s: &mut State) -> Decision {
    match next(s, t, &mut Scratch::new(), &mut Replay) {
        Step::Decide(d) => d,
        Step::Over(o) => panic!("the game ended: {o:?}"),
    }
}

/// Plays first and keeps, returning the first priority decision of the game.
pub(crate) fn begin(t: &GameTable, s: &mut State) -> Decision {
    loop {
        let d = decide(t, s);
        match d.kind {
            DecisionKind::PlayOrDraw => {
                assert_eq!(d.candidates.option(Choice(0)), Some(Opt::Play));
                apply(s, t, &mut Scratch::new(), &mut Replay, Choice(0));
            }
            DecisionKind::KeepOrMulligan => {
                apply(s, t, &mut Scratch::new(), &mut Replay, Choice(0));
            }
            _ => return d,
        }
    }
}
