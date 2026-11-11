//! The cards of the vocabulary's paper test that it can already say
//! (`docs/project-50k.md`, milestone 2): definitions of cards Beryl does not implement,
//! drawn at random from Scryfall's Oracle data, written here so that the claim "this card
//! fits" is one the compiler checks. Their Oracle text is quoted above each.
//!
//! Printed characteristics are generated from Scryfall (plan K2), so a subtype today's
//! `Subtype` lacks is left out; what is under test is the abilities.

use beryl_card::effect::build::{TARGET, YOU, create_tokens, destroy, draw, sequence};
use beryl_card::{
    Ability, AbilityKind, Amount, Behavior, CardDef, Colors, Cost, Effect, Face, Filter, Keywords,
    Objects, Subtype, Subtypes, TargetKind, TargetSpec, Trigger, TurnStep, Types, Whose, Zone,
    mana,
};

/// Fire Nation Warship {3}, Artifact — Vehicle 4/4. "Reach / When this Vehicle dies, create a
/// Clue token. (It's an artifact with '{2}, Sacrifice this token: Draw a card.') / Crew 2"
static FIRE_NATION_WARSHIP: CardDef = CardDef {
    name: "Fire Nation Warship",
    faces: &[Face {
        cost: mana("{3}"),
        types: Types::ARTIFACT,
        subtypes: Subtypes::of(&[Subtype::Vehicle]),
        power: Some(4),
        toughness: Some(4),
        keywords: Keywords::REACH,
        abilities: &[
            Ability::triggered(Trigger::Dies, Behavior::Data(&create_tokens(YOU, &CLUE, 1))),
            Ability::crew(2),
        ],
        ..Face::EMPTY
    }],
    tokens: &[&CLUE],
};

static CLUE: CardDef = CardDef {
    name: "Clue Token",
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability {
            kind: AbilityKind::Activated,
            cost: Cost {
                mana: mana("{2}"),
                sacrifice_self: true,
                ..Cost::NONE
            },
            body: Behavior::Data(&draw(YOU, 1)),
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// Serene Offering {1}{W}, Instant. "Destroy target enchantment. You gain life equal to its
/// mana value."
static SERENE_OFFERING: CardDef = CardDef {
    name: "Serene Offering",
    faces: &[Face {
        cost: mana("{1}{W}"),
        types: Types::INSTANT,
        colors: Colors::WHITE,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[
                TargetSpec::one(TargetKind::Permanent, Filter::Types(Types::ENCHANTMENT)).removal(),
            ],
            body: Behavior::Data(&sequence(&[
                destroy(TARGET),
                Effect::GainLife {
                    player: YOU,
                    amount: Amount::ManaValue(TARGET),
                },
            ])),
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// Demonic Lore {2}{B}, Enchantment. "When this enchantment enters, draw three cards. / At
/// the beginning of your end step, you lose 2 life for each card in your hand."
static DEMONIC_LORE: CardDef = CardDef {
    name: "Demonic Lore",
    faces: &[Face {
        cost: mana("{2}{B}"),
        types: Types::ENCHANTMENT,
        colors: Colors::BLACK,
        abilities: &[
            Ability::triggered(Trigger::Enters, Behavior::Data(&draw(YOU, 3))),
            Ability::triggered(
                Trigger::Step(TurnStep::End, Whose::You),
                Behavior::Data(&Effect::LoseLife {
                    player: YOU,
                    amount: Amount::Times(2, &Amount::Count(&CARDS_IN_YOUR_HAND)),
                }),
            ),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

const CARDS_IN_YOUR_HAND: Objects = Objects::All {
    zone: Zone::Hand,
    filter: Filter::Whose(Whose::You),
};

/// Azure Drake {3}{U}, Creature — Drake 2/4. "Flying"
static AZURE_DRAKE: CardDef = CardDef {
    name: "Azure Drake",
    faces: &[Face {
        cost: mana("{3}{U}"),
        types: Types::CREATURE,
        colors: Colors::BLUE,
        power: Some(2),
        toughness: Some(4),
        keywords: Keywords::FLYING,
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// Fugitive Wizard {U}, Creature — Human Wizard 1/1. No text. Woodland Druid {G}, a 1/2
/// Human Druid, is the same card in green.
static FUGITIVE_WIZARD: CardDef = CardDef {
    name: "Fugitive Wizard",
    faces: &[Face {
        cost: mana("{U}"),
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Human, Subtype::Wizard]),
        colors: Colors::BLUE,
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[test]
fn the_cards_that_fit_are_written_wholly_as_data() {
    let cards = [
        &FIRE_NATION_WARSHIP,
        &CLUE,
        &SERENE_OFFERING,
        &DEMONIC_LORE,
        &AZURE_DRAKE,
        &FUGITIVE_WIZARD,
    ];
    for card in cards {
        for ability in card.faces.iter().flat_map(|face| face.abilities) {
            assert!(
                matches!(ability.body, Behavior::Data(_)),
                "{} needs a Rust body",
                card.name
            );
        }
    }
}
