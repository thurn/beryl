//! Tests for prototype spells, `Condition::Cast`, `Filter::ManaValueAtMostSourcePower`,
//! and copying cards from a zone and casting the copies.

use beryl_card::effect::build::{
    copy_card, copy_spell, exile, may_cast_free, permit_cast, sequence, target, those, you,
};
use beryl_card::phrases::when_enters;
use beryl_card::{CastTerms, Condition, Duration, Filter, TargetKind, TargetSpec};

use super::cast::{Game, SAGE, cast};
use super::{DUAL, LAND, SPELL_A, SPELL_B};
use crate::card::{
    Ability, CardDef, CastingOption, Castings, Colors, Face, FaceIx, Keywords, ManaColor, ManaCost,
    ManaSet, Players, Supertypes, Types,
};
use crate::state::Zone;
use crate::{CardTable, Choice, DecisionKind, Decklist, Opt, Profile};

const PROTOTYPE_DEF: CardDef = CardDef {
    name: "Test Proto",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: ManaCost::generic(5),
        types: Types::ARTIFACT.union(Types::CREATURE),
        power: Some(4),
        toughness: Some(3),
        casting: &[CastingOption::prototype(
            ManaCost::new(1, [0, 2, 0, 0, 0, 0]),
            2,
            1,
        )],
        abilities: &[when_enters(&sequence(&[
            exile(target()),
            copy_card(those()),
            may_cast_free(you(), those()),
        ]))
        .only_if(&Condition::Cast)
        .target(&TargetSpec::graveyard_card(Filter::All(&[
            Filter::Types(Types::INSTANT.union(Types::SORCERY)),
            Filter::ManaValueAtMostSourcePower,
            Filter::Whose(&Players::You),
        ])))],
        ..Face::EMPTY
    }],
};

static MV4: CardDef = CardDef {
    name: "Test Four",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: ManaCost::new(3, [0, 1, 0, 0, 0, 0]),
        types: Types::SORCERY,
        ..Face::EMPTY
    }],
};

static ISLAND: CardDef = CardDef {
    name: "Test Island",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        supertypes: Supertypes::BASIC,
        types: Types::LAND,
        abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::Blue))],
        ..Face::EMPTY
    }],
};

static COPY_SPELL: CardDef = CardDef {
    name: "Test Copy Spell",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: ManaCost::new(0, [0, 1, 0, 0, 0, 0]),
        types: Types::INSTANT,
        abilities: &[Ability::spell()
            .target(&TargetSpec::one(TargetKind::Spell, Filter::Any))
            .effect(&copy_spell(target()))],
        ..Face::EMPTY
    }],
};

static FREE_CAST: CardDef = CardDef {
    name: "Test Free Cast",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: ManaCost::new(0, [0, 1, 0, 0, 0, 0]),
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&may_cast_free(
            you(),
            &beryl_card::Objects::All {
                zone: Zone::Hand,
                filter: Filter::Types(Types::CREATURE),
            },
        ))],
        ..Face::EMPTY
    }],
};

static YAWGMOTH_PERMIT: CardDef = CardDef {
    name: "Test Permit",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: ManaCost::new(0, [0, 1, 0, 0, 0, 0]),
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&permit_cast(
            you(),
            &beryl_card::Objects::All {
                zone: Zone::Graveyard,
                filter: Filter::All(&[
                    Filter::Types(Types::CREATURE),
                    Filter::Whose(&Players::You),
                ]),
            },
            Duration::EndOfTurn,
            CastTerms::PLAIN,
        ))],
        ..Face::EMPTY
    }],
};

static FLASH_PROTO: CardDef = CardDef {
    name: "Test Flash Proto",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: ManaCost::generic(5),
        types: Types::ARTIFACT.union(Types::CREATURE),
        keywords: Keywords::FLASH,
        power: Some(4),
        toughness: Some(3),
        casting: &[CastingOption::prototype(
            ManaCost::new(1, [0, 2, 0, 0, 0, 0]),
            2,
            1,
        )],
        ..Face::EMPTY
    }],
};

static CARDS: CardTable = CardTable::new(&[
    &LAND,
    &ISLAND,
    &DUAL,
    &SAGE,
    &SPELL_A,
    &SPELL_B,
    &PROTOTYPE_DEF,
    &MV4,
    &COPY_SPELL,
    &FREE_CAST,
    &YAWGMOTH_PERMIT,
    &FLASH_PROTO,
]);
static DECK: &Decklist = &[
    (16, "Test Island"),
    (6, "Test Proto"),
    (4, "Test Four"),
    (4, "Test Sage"),
    (4, "Test Copy Spell"),
    (2, "Test Free Cast"),
    (2, "Test Permit"),
    (2, "Test Flash Proto"),
];

#[test]
fn prototype_cast_triggers_and_uses_last_known_power_when_departed() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    assert!(g.t.can(crate::table::Capability::Prototype));
    assert!(g.t.can(crate::table::Capability::ChecksCastEntry));
    assert!(g.t.can(crate::table::Capability::ReadsDepartedSource));

    let p0 = g.me;
    g.main(p0);
    let proto = g.give(p0, "Test Proto", Zone::Hand);
    let mv4 = g.give(p0, "Test Four", Zone::Graveyard);
    let _sage = g.give(p0, "Test Sage", Zone::Graveyard);
    for _ in 0..5 {
        g.give(p0, "Test Island", Zone::Battlefield);
    }

    // Cast normally: Option NONE.
    g.pick(cast(proto));
    assert_eq!(g.s.mana_value(&g.t, proto), 5);
    assert_eq!(g.s.colors(&g.t, proto), Colors::NONE);

    // Resolve onto battlefield and target MV 4 card.
    g.pass_both();
    assert_eq!(g.decide().kind, DecisionKind::Target);
    g.pick_object(mv4);
    assert_eq!(g.s.stack().len(), 1);

    // Leaving the battlefield before resolution uses last-known power and still resolves.
    let proto_ref = g.s.object_ref(proto);
    let mut sink = crate::replacement::event::Sink::off();
    crate::replacement::event::move_one(
        &mut g.s,
        &g.t,
        &mut sink,
        proto,
        Zone::Graveyard,
        crate::replacement::event::Reason::Destroy,
    )
    .unwrap();
    assert!(g.s.last_known(proto_ref).is_some());
    g.pass_both();
    let offer = g.decide();
    assert_eq!(offer.kind, DecisionKind::CastOffer);

    // Declining to cast the card copy removes the copy at the next SBA check (CR 704.5e).
    g.pick(Opt::Pass);
    assert_eq!(g.s.exile(p0).count(), 1);
}

#[test]
fn copy_of_prototyped_spell_preserves_prototype_characteristics() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let p0 = g.me;
    g.main(p0);
    let proto = g.give(p0, "Test Flash Proto", Zone::Hand);
    let copy_spell = g.give(p0, "Test Copy Spell", Zone::Hand);
    for _ in 0..4 {
        g.give(p0, "Test Island", Zone::Battlefield);
    }

    g.pick(Opt::Cast {
        slot: proto,
        face: FaceIx::FRONT,
        costs: Castings::only(0),
    });
    assert_eq!(g.s.mana_value(&g.t, proto), 3);
    assert_eq!(g.s.colors(&g.t, proto), Colors::BLUE);

    g.pick(cast(copy_spell));
    g.pass_both(); // Resolve Copy Spell.

    // Top of stack is now the copied prototyped spell.
    assert_eq!(g.s.stack().len(), 2);
    let spell_copy = g.s.stack()[1].spell().unwrap().slot;
    assert_eq!(g.s.mana_value(&g.t, spell_copy), 3);
    assert_eq!(g.s.colors(&g.t, spell_copy), Colors::BLUE);

    g.pass_both(); // Resolve copied spell -> 2/1 blue token on battlefield.
    assert!(g.s.object(spell_copy).is_token());
    assert_eq!(g.s.object(spell_copy).zone(), Zone::Battlefield);
    assert_eq!(
        (g.s.power(&g.t, spell_copy), g.s.toughness(&g.t, spell_copy)),
        (2, 1)
    );
    assert_eq!(g.s.colors(&g.t, spell_copy), Colors::BLUE);

    // When the token leaves the battlefield it keeps its copied prototype state briefly before
    // expiring, and the original resolving spell also becomes 2/1 blue.
    g.pass_both();
    assert_eq!(g.s.object(proto).zone(), Zone::Battlefield);
    assert_eq!((g.s.power(&g.t, proto), g.s.toughness(&g.t, proto)), (2, 1));
}

#[test]
fn free_cast_and_graveyard_permission_offer_prototype_option() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let p0 = g.me;
    g.main(p0);
    let proto = g.give(p0, "Test Flash Proto", Zone::Hand);
    let free_cast = g.give(p0, "Test Free Cast", Zone::Hand);
    for _ in 0..2 {
        g.give(p0, "Test Island", Zone::Battlefield);
    }

    // Free cast offers both normal and prototyped casts of Test Flash Proto.
    g.pick(cast(free_cast));
    g.pass_both();
    let offer = g.decide();
    assert_eq!(offer.kind, DecisionKind::CastOffer);
    let proto_opt = Opt::Cast {
        slot: proto,
        face: FaceIx::FRONT,
        costs: Castings::only(0),
    };
    let normal_opt = Opt::Cast {
        slot: proto,
        face: FaceIx::FRONT,
        costs: Castings::NONE,
    };
    assert!(super::cast::index_of(&offer, proto_opt).is_some());
    assert!(super::cast::index_of(&offer, normal_opt).is_some());
    g.pick(proto_opt);
    g.pass_both();
    assert_eq!(g.s.object(proto).zone(), Zone::Battlefield);
    assert_eq!((g.s.power(&g.t, proto), g.s.toughness(&g.t, proto)), (2, 1));
    assert_eq!(g.s.colors(&g.t, proto), Colors::BLUE);

    // Put in graveyard and cast prototyped from graveyard via PermitCast.
    g.s.move_object(
        &mut crate::replacement::event::Sink::off(),
        proto,
        Zone::Graveyard,
        crate::zone::Place::Top,
    )
    .unwrap();
    assert_eq!(g.s.mana_value(&g.t, proto), 5);
    assert_eq!(g.s.colors(&g.t, proto), Colors::NONE);

    let permit = g.give(p0, "Test Permit", Zone::Hand);
    for _ in 0..3 {
        g.give(p0, "Test Island", Zone::Battlefield);
    }
    g.pick(cast(permit));
    g.pass_both();
    let d = g.decide();
    let c = super::cast::index_of(
        &d,
        Opt::Cast {
            slot: proto,
            face: FaceIx::FRONT,
            costs: Castings::only(0),
        },
    )
    .expect("prototype option offered from graveyard");
    g.choose(Choice(c.0));
    assert_eq!(g.s.mana_value(&g.t, proto), 3);
    assert_eq!(g.s.colors(&g.t, proto), Colors::BLUE);
}
