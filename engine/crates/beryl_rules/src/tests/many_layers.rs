//! A resolved effect with parts in several layers, begun together on one object ([CR 613.6]):
//! "If an effect should be applied in different layers and/or sublayers, the parts of the effect
//! each apply in their appropriate ones." "Loses all abilities and becomes a green Elk creature
//! with base power and toughness 3/3" has parts in layers 4, 5, 6 and 7b.
//!
//! [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136

use beryl_card::effect::build::{modify, target};
use beryl_card::{Amount, Colors, Keywords, Objects, Subtype, Subtypes, Supertypes, TargetSpec};

use crate::card::{Ability, CardDef, Face, LayerSpec, Modification, Types};
use crate::filter::Filter;
use crate::ids::{Seat, Slot};
use crate::state::{Duration, LayerChange, TurnStep, Zone};
use crate::{CardTable, GameTable, Profile, State, continuous::layers, reference};
use beryl_kernel::format::Format;

use super::LAND;
use super::events::find;

static THREE: Amount = Amount::Fixed(3);

/// The parts of the Elk effect, as the card says them.
static ELK: [Modification; 5] = [
    Modification::LoseAllAbilities,
    Modification::SetColors(Colors::GREEN),
    Modification::SetTypes(Types::CREATURE),
    Modification::SetSubtype(Subtype::Elk),
    Modification::SetBasePt(&THREE, &THREE),
];

/// "Target artifact or creature loses all abilities and becomes a green Elk creature with base
/// power and toughness 3/3": a sorcery, so that the game numbers the subtype Elk.
static ELKIFY: CardDef = CardDef {
    name: "Test Elkify",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::SORCERY,
        abilities: &[Ability::spell()
            .target(&TargetSpec::permanent(Filter::Types(
                Types::ARTIFACT.union(Types::CREATURE),
            )))
            .effect(&modify(target(), &ELK, beryl_card::Duration::Indefinite))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A legendary white 2/2 Human artifact creature with flying.
static GOLEM: CardDef = CardDef {
    name: "Test Golem",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT.union(Types::CREATURE),
        supertypes: Supertypes::LEGENDARY,
        subtypes: crate::card::SubtypeWords::new(Subtypes::of(&[Subtype::Human])),
        colors: Colors::WHITE,
        power: Some(2),
        toughness: Some(2),
        keywords: Keywords::FLYING,
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// An Equipment: "Equipped creature gets +1/+1 and has vigilance."
static BANNER: CardDef = CardDef {
    name: "Test Banner",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        subtypes: crate::card::SubtypeWords::new(Subtypes::of(&[Subtype::Equipment])),
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::AttachedTo,
            condition: None,
            modifications: &[
                Modification::ModifyPt(1, 1),
                Modification::AddKeywords(Keywords::VIGILANCE),
            ],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &ELKIFY, &GOLEM, &BANNER]);

/// A game in the first player's main phase, each deck holding two of each card.
fn game() -> (GameTable, State) {
    let deck: &[(u8, &str)] = &[
        (2, "Test Elkify"),
        (2, "Test Golem"),
        (2, "Test Banner"),
        (8, "Test Land"),
    ];
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    (t, s)
}

fn refresh(t: &GameTable, s: &mut State) {
    layers::refresh(s, t, &mut crate::replacement::event::Sink::off()).unwrap();
    reference::check_references(s, t).unwrap();
}

fn enters(t: &GameTable, s: &mut State, card: &str) -> Slot {
    let slot = find(s, t, Seat::P0, card);
    s.board().place(slot, Zone::Battlefield).unwrap();
    refresh(t, s);
    slot
}

/// Starts the Elk effect on `slot`, its parts with one timestamp, as the interpreter does.
fn elk(t: &GameTable, s: &mut State, slot: Slot) {
    let you = Seat::P0;
    let changes: Vec<LayerChange> = ELK
        .iter()
        .map(|&m| LayerChange::resolve(m, you, &crate::card::Words::NONE, || Colors::NONE).unwrap())
        .collect();
    let (card, _) = s.copiable(slot).record(t);
    let mut members = crate::bitboard::Bitboard::EMPTY;
    members.insert(slot);
    s.start_effect_on_set(
        card,
        s.permanents_in(members),
        &changes,
        Duration::Indefinite,
    )
    .unwrap();
    refresh(t, s);
}

/// The Elk is a green 3/3 Elk creature with no abilities. It keeps its supertype, and loses
/// its other card types and creature types (Oko, Thief of Crowns's ruling: "It's just a green
/// Elk. The creature keeps any supertypes (such as legendary) it has, but loses any other card
/// types it has (such as artifact)"), as [CR 205.1a] says of an effect that sets a subtype:
/// "the new subtype(s) replaces any existing subtypes from the appropriate set".
///
/// [CR 205.1a]: https://yawgatog.com/resources/magic-rules/#R2051a
#[test]
fn the_elk_is_a_green_elk_creature_and_nothing_else() {
    let (t, mut s) = game();
    let golem = enters(&t, &mut s, "Test Golem");
    elk(&t, &mut s, golem);
    let d = s.derived(golem);
    assert_eq!(d.types, Types::CREATURE);
    assert_eq!(d.supertypes, Supertypes::LEGENDARY);
    assert_eq!(d.colors, Colors::GREEN);
    assert!(d.subtypes.contains(t.subtypes(), Subtype::Elk));
    assert!(!d.subtypes.contains(t.subtypes(), Subtype::Human));
    assert!(!d.keywords.contains(Keywords::FLYING));
    assert_eq!((d.power, d.toughness), (3, 3));
    // Its five parts are five entries with one timestamp (CR 613.7).
    let stamps: Vec<u16> = s.effects().iter().map(|e| e.timestamp).collect();
    assert_eq!(stamps.len(), 5);
    assert!(stamps.iter().all(|&stamp| stamp == stamps[0]));
}

/// An Equipment attached before the effect began gives the Elk only its +1/+1, which layer 7c
/// applies whatever its timestamp; its vigilance is granted in layer 6 before the removal, by
/// timestamp, and so removed. One attached after it, which gets a new timestamp as it is
/// attached ([CR 701.3c]), gives both (the card's ruling: "If the affected creature gains an
/// ability after Oko's second ability resolves, it will keep that ability.").
///
/// [CR 701.3c]: https://yawgatog.com/resources/magic-rules/#R7013c
#[test]
fn equipment_attached_after_the_effect_grants_its_ability_and_before_it_only_its_bonus() {
    let (t, mut s) = game();
    let golem = enters(&t, &mut s, "Test Golem");
    let banner = enters(&t, &mut s, "Test Banner");
    s.attach(
        s.permanent_at(banner).unwrap(),
        s.permanent_at(golem).unwrap(),
    );
    refresh(&t, &mut s);
    assert!(s.derived(golem).keywords.contains(Keywords::VIGILANCE));
    elk(&t, &mut s, golem);
    let d = s.derived(golem);
    assert_eq!((d.power, d.toughness), (4, 4));
    assert!(!d.keywords.contains(Keywords::VIGILANCE));

    let (t, mut s) = game();
    let golem = enters(&t, &mut s, "Test Golem");
    let banner = enters(&t, &mut s, "Test Banner");
    elk(&t, &mut s, golem);
    s.attach(
        s.permanent_at(banner).unwrap(),
        s.permanent_at(golem).unwrap(),
    );
    refresh(&t, &mut s);
    let d = s.derived(golem);
    assert_eq!((d.power, d.toughness), (4, 4));
    assert!(d.keywords.contains(Keywords::VIGILANCE));
}

/// Merging repeated `ModifyPt` effects from one source on one creature leaves layer results
/// identical to starting every effect unmerged, even when `SetBasePt` and `SwitchPt` effects
/// are interleaved across several creatures and sources.
#[test]
fn merged_modify_pt_matches_unmerged_layer_results_across_200_seeds() {
    use crate::state::Continuous;

    let deck: &[(u8, &str)] = &[(3, "Test Golem"), (2, "Test Banner"), (8, "Test Land")];
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [deck, deck]).unwrap();
    let mut base = State::new(&t, 7, Profile::EXACT).unwrap();
    base.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let mut creatures = Vec::new();
    for _ in 0..3 {
        creatures.push(enters(&t, &mut base, "Test Golem"));
    }
    let banner = enters(&t, &mut base, "Test Banner");
    let sources = [
        base.copiable(creatures[0]).record(&t).0,
        base.copiable(banner).record(&t).0,
    ];
    let durations = [
        Duration::EndOfTurn,
        Duration::Indefinite,
        Duration::UntilYourNextTurn(Seat::P0),
    ];

    let mut total_saved = 0usize;
    for seed in 0u64..200 {
        let mut merged = base;
        let mut unmerged = base;
        let mut x = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let count = 20 + (next() as usize % 25);
        for _ in 0..count {
            let slot = creatures[next() as usize % creatures.len()];
            let source = sources[next() as usize % sources.len()];
            let duration = durations[next() as usize % durations.len()];
            let change = match next() % 10 {
                0..=6 => {
                    let p = (next() as i8).rem_euclid(5) - 1;
                    let q = (next() as i8).rem_euclid(5) - 1;
                    LayerChange::ModifyPt(p, q)
                }
                7..=8 => {
                    let p = (next() as i8).rem_euclid(7);
                    let q = (next() as i8).rem_euclid(7);
                    LayerChange::SetBasePt(p, q)
                }
                _ => LayerChange::SwitchPt,
            };
            let perm_m = merged.permanent_at(slot).unwrap();
            let perm_u = unmerged.permanent_at(slot).unwrap();
            merged
                .start_effect(source, Continuous::Layer(perm_m, change), duration)
                .unwrap();
            unmerged
                .start_effect_unmerged(source, Continuous::Layer(perm_u, change), duration)
                .unwrap();
        }
        total_saved += unmerged.effects().len() - merged.effects().len();
        refresh(&t, &mut merged);
        refresh(&t, &mut unmerged);
        for &slot in &creatures {
            assert_eq!(
                merged.derived(slot),
                unmerged.derived(slot),
                "seed {seed}, slot {slot:?}"
            );
        }
    }
    assert!(
        total_saved > 0,
        "expected repeated ModifyPt effects to merge"
    );
}
