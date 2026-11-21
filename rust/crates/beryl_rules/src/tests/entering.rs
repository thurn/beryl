//! A permanent as it would exist on the battlefield, which the replacement effects that
//! modify how it enters look at ([CR 614.12]): "check the characteristics of the permanent
//! as it would exist on the battlefield, taking into account ... continuous effects from
//! the permanent’s own static abilities that would apply to it once it’s on the
//! battlefield, and continuous effects that already exist and would apply to the
//! permanent."
//!
//! [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412

use beryl_card::{BasicLandTypes, ManaColor, ManaSet, Objects};

use crate::card::{
    Ability, CardDef, Face, FaceIx, LayerSpec, Modification, Replacement, Subtype, SubtypeWords,
    Subtypes, Supertypes, TargetKind, TargetSpec, Types,
};
use crate::continuous::layers::entering::as_it_would_exist;
use crate::filter::Filter;
use crate::ids::Seat;
use crate::replacement::event::{self, Entry, Reason, Sink};
use crate::rng::SplitMix64;
use crate::scratch::Scratch;
use crate::state::{TurnStep, Zone};
use crate::{CardTable, GameTable, Observation, Profile, State, continuous::layers};

use super::events::{find, put};

/// "Nonbasic lands are Mountains."
static MOON: CardDef = CardDef {
    name: "Test Moon",
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::All(&[
                    Filter::Types(Types::LAND),
                    Filter::Not(&Filter::Supertypes(Supertypes::BASIC)),
                ]),
            },
            condition: None,
            modifications: &[Modification::SetLandTypes(BasicLandTypes::MOUNTAIN)],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Each enchantment is a creature in addition to its other types."
static OPAL: CardDef = CardDef {
    name: "Test Opal",
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::Types(Types::ENCHANTMENT),
            },
            condition: None,
            modifications: &[Modification::AddTypes(Types::CREATURE)],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "As this land enters, you may pay 3 life. If you don't, it enters tapped."
static SPRINGS: CardDef = CardDef {
    name: "Test Springs",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[
            Ability::replacing(&Replacement::enters_tapped_unless_pay_life(3)),
            Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "This land enters tapped."
static TAPLAND: CardDef = CardDef {
    name: "Test Tapland",
    faces: &[Face {
        supertypes: Supertypes::BASIC,
        types: Types::LAND,
        abilities: &[
            Ability::replacing(&Replacement::ENTERS_TAPPED),
            Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// May copy an enchantment as it enters.
static MIRROR: CardDef = CardDef {
    name: "Test Mirror",
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::replacing(&Replacement::enter_as_copy(
            &Filter::Types(Types::ENCHANTMENT),
            None,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// Its back face is an Aura with enchant land.
static SHIFTING_AURA: CardDef = CardDef {
    name: "Test Shifting Aura",
    faces: &[
        Face {
            types: Types::ARTIFACT,
            ..Face::EMPTY
        },
        Face {
            name: "Test Land Aura",
            types: Types::ENCHANTMENT,
            subtypes: SubtypeWords::new(Subtypes::of(&[Subtype::Aura])),
            abilities: &[Ability::spell().target(&TargetSpec::one(
                TargetKind::Permanent,
                Filter::Types(Types::LAND),
            ))],
            transformed: true,
            ..Face::EMPTY
        },
    ],
    tokens: &[],
};

/// The copy enters as the back face of an Aura, so its controller must choose a legal land
/// to enchant ([CR 303.4f]) even though the card entering is a single-faced artifact.
///
/// [CR 303.4f]: https://yawgatog.com/resources/magic-rules/#R3034f
#[test]
fn a_copy_of_a_back_face_aura_enters_attached() {
    static TABLE: CardTable = CardTable::new(&[&MIRROR, &SHIFTING_AURA, &TAPLAND]);
    let player = Seat::P0;
    let deck = &[
        (1, "Test Mirror"),
        (1, "Test Shifting Aura"),
        (1, "Test Tapland"),
    ];
    let t = GameTable::new(&TABLE, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    let mut x = Scratch::new();
    let aura = put(&t, &mut s, &mut x, player, "Test Shifting Aura");
    let land = put(&t, &mut s, &mut x, player, "Test Tapland");
    s.board().set_face(aura, FaceIx::BACK);
    layers::refresh(&mut s, &t, &mut x.events).unwrap();

    // The sole enchantment is the second option, after declining the copy.
    x.events.asking.begin(&[1], false).unwrap();
    let mirror = put(&t, &mut s, &mut x, player, "Test Mirror");
    x.events.asking.end();
    assert_eq!(s.copiable(mirror).face(&t).name, "Test Land Aura");
    assert_eq!(s.object(mirror).attached_to(), Some(land));
}

/// A face-down permanent's card is hidden ([CR 708.2]). Redealing it for the other
/// player can change its original card, but must keep the existing copy effect intact.
///
/// [CR 708.2]: https://yawgatog.com/resources/magic-rules/#R7082
#[test]
fn redealing_the_card_under_a_face_down_copy_preserves_the_copy() {
    let (t, mut s) = game();
    let mut x = Scratch::new();
    let source = put(&t, &mut s, &mut x, Seat::P0, "Test Opal");
    let slot = put(&t, &mut s, &mut x, Seat::P1, "Test Tapland");
    let values = s.copiable(source);
    s.enter_as_copy(s.permanent_at(slot).unwrap(), values)
        .unwrap();
    s.board().set_face(slot, FaceIx::DOWN);
    layers::refresh(&mut s, &t, &mut x.events).unwrap();

    let original = s.card_really_is(slot);
    let view = Observation::new(&s, Seat::P0);
    let mut rng = SplitMix64::new(5);
    let mut changed = false;
    for _ in 0..8 {
        let dealt = view.determinize(&t, &mut rng);
        dealt.check_invariants(&t).unwrap();
        assert_eq!(dealt.copiable(slot), s.copiable(slot));
        changed |= dealt.card_really_is(slot) != original;
    }
    assert!(changed);
}

static CARDS: CardTable = CardTable::new(&[&MOON, &OPAL, &SPRINGS, &TAPLAND]);

/// A game in the first player's main phase, each deck holding some of each card.
fn game() -> (GameTable, State) {
    let deck: &[(u8, &str)] = &[
        (4, "Test Moon"),
        (4, "Test Opal"),
        (4, "Test Springs"),
        (4, "Test Tapland"),
    ];
    let t = GameTable::new(&CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
    (t, s)
}

/// An enchantment that would be a creature only because of an effect already in force is
/// a creature as it would exist, and the real state is left as it was: the card is still
/// in its library, and nothing on the battlefield changed.
#[test]
fn an_object_would_be_a_creature_only_because_of_an_effect() {
    let (t, mut s) = game();
    let mut x = Scratch::new();
    let moon = find(&s, &t, Seat::P0, "Test Moon");
    let before = as_it_would_exist(&s, &t, moon, Entry::default(), None).unwrap();
    assert!(!before.types.intersects(Types::CREATURE));
    put(&t, &mut s, &mut x, Seat::P0, "Test Opal");
    let unchanged = s;
    let after = as_it_would_exist(&s, &t, moon, Entry::default(), None).unwrap();
    assert!(
        after
            .types
            .contains(Types::ENCHANTMENT.union(Types::CREATURE))
    );
    assert_eq!(s, unchanged);
    assert_eq!(s.object(moon).zone(), Zone::Library);
}

/// Under an effect that makes nonbasic lands Mountains, a nonbasic land that would enter
/// tapped unless its controller paid life would be a Mountain with no such ability, so it
/// enters untapped and asks for no life. A basic land keeps its own entering replacement.
#[test]
fn an_entering_replacement_the_permanent_would_lose_does_not_apply() {
    let (t, mut s) = game();
    let mut x = Scratch::new();
    let springs = put(&t, &mut s, &mut x, Seat::P0, "Test Springs");
    assert!(s.object(springs).is_tapped(), "tapped with no Moon");
    put(&t, &mut s, &mut x, Seat::P0, "Test Moon");
    let springs = find(&s, &t, Seat::P0, "Test Springs");
    let entry = Entry::default();
    assert_eq!(
        crate::replacement::life_to_enter(&s, &t, springs, entry),
        Ok(None)
    );
    let life = s.player(Seat::P0).life();
    event::move_one(
        &mut s,
        &t,
        &mut x.events,
        springs,
        Zone::Battlefield,
        Reason::Effect,
    )
    .unwrap();
    assert!(!s.object(springs).is_tapped(), "untapped under the Moon");
    assert_eq!(s.player(Seat::P0).life(), life);
    let tapland = put(&t, &mut s, &mut x, Seat::P0, "Test Tapland");
    assert!(s.object(tapland).is_tapped(), "a basic land keeps its text");
}
