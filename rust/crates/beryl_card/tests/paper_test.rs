//! The cards of the vocabulary's paper test that it can already say
//! (`docs/project-50k.md`, milestone 2): definitions of cards Beryl does not implement,
//! drawn at random from Scryfall's Oracle data, written here so that the claim "this card
//! fits" is one the compiler checks. Their Oracle text is quoted above each.
//!
//! Printed characteristics are generated from Scryfall (plan K2), so a subtype today's
//! `Subtype` lacks is left out; what is under test is the abilities.

use beryl_card::effect::build::{
    EACH_OPPONENT, EACH_PLAYER, TARGET, YOU, create_tokens, destroy, draw, sequence,
};
use beryl_card::{
    Ability, AbilityKind, Amount, Behavior, CardDef, Colors, Cost, Effect, Face, Filter, Keywords,
    Objects, Subject, Subtype, Subtypes, TargetKind, TargetSpec, Trigger, TriggerEvent, TurnStep,
    Types, Whose, Zone, mana,
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
            Ability::triggered(
                &Trigger::DIES,
                Behavior::Data(&create_tokens(YOU, &CLUE, 1)),
            ),
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
            Ability::triggered(&Trigger::ENTERS, Behavior::Data(&draw(YOU, 3))),
            Ability::triggered(
                &Trigger::step(TurnStep::End, YOU),
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

/// The triggers of the paper test's misfits. Of its 60 cards, 17 asked for a `Trigger` entry,
/// nearly all an event that existed with another subject or player. A trigger is now an
/// event, a subject and players, so those whose event the engine raises are said with
/// entries that exist, each under its card and its words.
///
/// Not here, though the engine raises the event, for want of an entry elsewhere in the
/// vocabulary: "your commander attacks" (We Ride at Dawn: a commander `Filter`), "this
/// creature or another Ally you control enters" (Kazandu Blademaster: a `Filter` for an
/// object other than this one), "you cast a spell other than your first spell each turn"
/// (Arcbound Tracker: a count of this turn's spells) and "you cast an artifact or creature
/// spell using mana produced by" this (Barracks of the Thousand: where spent mana came from).
/// The rest wait for an event the engine does not raise for triggers: a lore counter put on
/// a Saga, leaving the battlefield other than by dying, a sacrifice, blocking, being turned
/// face up, a card put into a graveyard from a library, becoming a target, a crime.
const TRIGGERS: [(&str, &str, Trigger); 8] = [
    (
        "Speed, Young Avenger",
        "Whenever you cast a noncreature spell",
        Trigger::casts(YOU, Filter::Not(&Filter::Types(Types::CREATURE))),
    ),
    ("Arcbound Tracker", "When it dies", Trigger::DIES),
    (
        "Horned Stoneseeker",
        "When this creature enters",
        Trigger::ENTERS,
    ),
    (
        "Spiritual Sanctuary",
        "At the beginning of each player's upkeep",
        Trigger::step(TurnStep::Upkeep, EACH_PLAYER),
    ),
    // Frog is a subtype `Subtype` lacks, left out as the printed ones are.
    (
        "Grolnok, the Omnivore",
        "Whenever a Frog you control attacks",
        Trigger::of(
            TriggerEvent::Attacks,
            Subject::A(Filter::All(&[
                Filter::Types(Types::CREATURE),
                Filter::Whose(Whose::You),
            ])),
        ),
    ),
    (
        "Thousand Moons Smithy",
        "When Thousand Moons Smithy enters",
        Trigger::ENTERS,
    ),
    (
        "Thousand Moons Smithy",
        "At the beginning of your first main phase",
        Trigger::step(TurnStep::PrecombatMain, YOU),
    ),
    (
        "Taurean Mauler",
        "Whenever an opponent casts a spell",
        Trigger::casts(EACH_OPPONENT, Filter::Any),
    ),
];

/// A printed trigger is no delayed one: an effect makes those (CR 603.7).
#[test]
fn the_misfits_triggers_are_printed_ones() {
    for (card, words, trigger) in TRIGGERS {
        assert!(!trigger.delayed, "{card}: {words}");
    }
}

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
