//! The cards of the vocabulary's paper test that it can already say
//! (`docs/project-50k.md`, milestone 2): definitions of cards Beryl does not implement,
//! drawn at random from Scryfall's Oracle data, written here so that the claim "this card
//! fits" is one the compiler checks. Their Oracle text is quoted above each.
//!
//! Printed characteristics are generated from Scryfall (plan K2), so a subtype today's
//! `Subtype` lacks is left out; what is under test is the abilities.

use beryl_card::effect::build::{
    EACH_OPPONENT, EACH_PLAYER, TARGET, THAT_PLAYER, THIS, YOU, counter, create_tokens, destroy,
    draw, sequence,
};
use beryl_card::{
    Ability, AbilityKind, Amount, Behavior, CardDef, Colors, Comparison, Condition, Cost,
    CostAction, CostPart, CounterKind, Effect, Face, Filter, Keywords, LayerSpec, Modification,
    Objects, Subject, Subtype, Subtypes, Supertypes, TargetKind, TargetSpec, Timing, Trigger,
    TriggerEvent, TurnStep, Types, Zone, mana,
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
            cost: Cost::of(mana("{2}"), &[CostPart::SACRIFICE]),
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
    filter: Filter::Whose(&YOU),
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

/// "The sacrificed creature": what the cost's sacrifice took.
const SACRIFICED: Objects = Objects::Paid(CostAction::Sacrifice);

/// Slagdrill Scrapper {R}, Artifact Creature — Robot Scout 2/2. "{2}, {T}, Sacrifice another
/// artifact or land: Draw a card."
static SLAGDRILL_SCRAPPER: CardDef = CardDef {
    name: "Slagdrill Scrapper",
    faces: &[Face {
        cost: mana("{R}"),
        types: Types::ARTIFACT.union(Types::CREATURE),
        colors: Colors::RED,
        power: Some(2),
        toughness: Some(2),
        abilities: &[Ability {
            kind: AbilityKind::Activated,
            cost: Cost::of(
                mana("{2}"),
                &[
                    CostPart::TAP,
                    CostPart::sacrifice_another(Filter::Types(Types::ARTIFACT.union(Types::LAND))),
                ],
            ),
            body: Behavior::Data(&draw(YOU, 1)),
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// Severed Strands {1}{B}, Sorcery. "As an additional cost to cast this spell, sacrifice a
/// creature. / You gain life equal to the sacrificed creature's toughness. Destroy target
/// creature an opponent controls."
static SEVERED_STRANDS: CardDef = CardDef {
    name: "Severed Strands",
    faces: &[Face {
        cost: mana("{1}{B}"),
        types: Types::SORCERY,
        colors: Colors::BLACK,
        additional_cost: &[CostPart::sacrifice(Filter::Types(Types::CREATURE))],
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            timing: Timing::Sorcery,
            targets: &[TargetSpec::one(
                TargetKind::Permanent,
                Filter::All(&[
                    Filter::Types(Types::CREATURE),
                    Filter::Whose(&EACH_OPPONENT),
                ]),
            )
            .removal()],
            body: Behavior::Data(&sequence(&[
                Effect::GainLife {
                    player: YOU,
                    amount: Amount::Toughness(SACRIFICED),
                },
                destroy(TARGET),
            ])),
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// Teferi's Care {2}{W}, Enchantment. "{W}, Sacrifice an enchantment: Destroy target
/// enchantment. / {3}{U}{U}: Counter target enchantment spell."
static TEFERIS_CARE: CardDef = CardDef {
    name: "Teferi's Care",
    faces: &[Face {
        cost: mana("{2}{W}"),
        types: Types::ENCHANTMENT,
        colors: Colors::WHITE,
        abilities: &[
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::of(
                    mana("{W}"),
                    &[CostPart::sacrifice(Filter::Types(Types::ENCHANTMENT))],
                ),
                targets: &[TargetSpec::one(
                    TargetKind::Permanent,
                    Filter::Types(Types::ENCHANTMENT),
                )
                .removal()],
                body: Behavior::Data(&destroy(TARGET)),
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::mana(mana("{3}{U}{U}")),
                targets: &[
                    TargetSpec::one(TargetKind::Spell, Filter::Types(Types::ENCHANTMENT)).counter(),
                ],
                body: Behavior::Data(&counter(TARGET)),
                ..Ability::NONE
            },
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// To forage ([CR 701.61a](https://yawgatog.com/resources/magic-rules/#R70161a)): "Exile
/// three cards from your graveyard or sacrifice a Food." Food is a subtype `Subtype` lacks,
/// left out as the printed ones are.
const FORAGE: CostPart = CostPart::OneOf(&[
    Cost::of_parts(&[CostPart::exile(3, Zone::Graveyard, Filter::Any)]),
    Cost::of_parts(&[CostPart::sacrifice(Filter::Types(Types::ARTIFACT))]),
]);

/// The costs of the paper test's misfits. Of its 60 cards, 8 asked for a part of a cost paid
/// with other objects, or an additional cost that is one. A cost is now mana and parts, so
/// each is said here under its card and its words; the three cards with nothing else missing
/// are written whole above.
///
/// The engine pays few of these yet (`beryl_kernel`'s `payable`), and a card table refuses the
/// rest: what is under test is that the vocabulary says them.
const COSTS: [(&str, &str, Cost); 8] = [
    (
        "Flamekin Bladewhirl",
        "As an additional cost to cast this spell, reveal an Elemental card from your hand or \
         pay {3}",
        Cost::of_parts(&[CostPart::OneOf(&[
            Cost::of_parts(&[CostPart::reveal(Filter::Subtype(Subtype::Elemental))]),
            Cost::mana(mana("{3}")),
        ])]),
    ),
    (
        "Slagdrill Scrapper",
        "{2}, {T}, Sacrifice another artifact or land",
        SLAGDRILL_SCRAPPER.faces[0].abilities[0].cost,
    ),
    (
        "Camellia, the Seedmiser",
        "{2}, Forage",
        Cost::of(mana("{2}"), &[FORAGE]),
    ),
    (
        "Channel",
        "you may pay 1 life",
        Cost::of_parts(&[CostPart::Life(1)]),
    ),
    (
        "Gríma Wormtongue",
        "{T}, Sacrifice another creature",
        Cost::of_parts(&[
            CostPart::TAP,
            CostPart::sacrifice_another(Filter::Types(Types::CREATURE)),
        ]),
    ),
    (
        "Severed Strands",
        "As an additional cost to cast this spell, sacrifice a creature",
        Cost::of_parts(SEVERED_STRANDS.faces[0].additional_cost),
    ),
    (
        "Teferi's Care",
        "{W}, Sacrifice an enchantment",
        TEFERIS_CARE.faces[0].abilities[0].cost,
    ),
    (
        "Lluwen, Exchange Student",
        "Exile a creature card from your graveyard",
        Cost::of_parts(&[CostPart::exile(
            1,
            Zone::Graveyard,
            Filter::Types(Types::CREATURE),
        )]),
    ),
];

/// Gríma Wormtongue: "If the sacrificed creature was legendary, amass Orcs 2." The condition
/// names what the cost took.
const SACRIFICED_WAS_LEGENDARY: Condition =
    Condition::Is(SACRIFICED, Filter::Supertypes(Supertypes::LEGENDARY));

/// Every misfit's cost has a part that is not mana, and the two effects that name "the
/// sacrificed creature" read what a cost took.
#[test]
fn the_misfits_costs_are_parts_and_an_effect_names_what_one_took() {
    for (card, words, cost) in COSTS {
        assert!(!cost.parts().is_empty(), "{card}: {words}");
    }
    assert!(SACRIFICED_WAS_LEGENDARY.names_paid());
    let [strands] = SEVERED_STRANDS.faces[0].abilities else {
        panic!("one spell ability");
    };
    assert!(strands.names_paid());
    assert!(!TEFERIS_CARE.faces[0].abilities[0].names_paid());
}

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
                Filter::Whose(&YOU),
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

const CREATURES: Filter = Filter::Types(Types::CREATURE);

/// The static abilities of the paper test's misfits. Six of its cards asked for a static
/// ability over a filtered set, and four for an "as long as" that is a general condition. A
/// static ability's affected side is now the object expression effects use and its "as long
/// as" a `Condition`, so those whose every other entry exists are said whole, each under its
/// card and its words.
///
/// Not here, for want of an entry elsewhere in the vocabulary: "other Squirrels you control
/// have menace" (Camellia, the Seedmiser: a `Filter` for an object other than this one, and
/// menace), "attacking Pirates you control get +2/+0" (Dire Fleet Neckbreaker: an attacking
/// `Filter`), "commander creatures you own have ..." (Acolyte of Bahamut: a commander
/// `Filter`, a `Filter` for a permanent's owner, and a cost-changing static ability),
/// "legendary creature spells you cast have convoke" and "all creatures have double strike"
/// (We Ride at Dawn, Avatar of Slaughter: the keywords; their sets are in [`AFFECTED`]),
/// "all Goblins are black" (Dralnu's Crusade: a color `Modification`), "as long as you
/// control a red creature" (Blazethorn Scarecrow: a color `Filter`, and haste), "as long as
/// you've completed a dungeon" and "as long as two or more nonland permanents entered the
/// battlefield under your control this turn" (Cloister Gargoyle, Tuinvale Guide: something
/// read from a tracker), and Vadmir's menace and lifelink (its condition is in
/// [`AS_LONG_AS`]).
const STATICS: [(&str, &str, LayerSpec); 1] = [
    // Goblin is a subtype `Subtype` lacks, left out as the printed ones are.
    (
        "Dralnu's Crusade",
        "All Goblins get +1/+1",
        LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: CREATURES,
            },
            condition: None,
            modification: Modification::ModifyPt(1, 1),
        },
    ),
];

/// The sets the misfits' static abilities affect, where the set is all the vocabulary can
/// say of the ability: what it does to them is a keyword `Keywords` lacks.
const AFFECTED: [(&str, &str, Objects); 2] = [
    (
        "We Ride at Dawn",
        "Legendary creature spells you cast",
        Objects::All {
            zone: Zone::Stack,
            filter: Filter::All(&[
                Filter::Supertypes(Supertypes::LEGENDARY),
                CREATURES,
                Filter::Whose(&YOU),
            ]),
        },
    ),
    (
        "Avatar of Slaughter",
        "All creatures",
        Objects::All {
            zone: Zone::Battlefield,
            filter: CREATURES,
        },
    ),
];

/// The misfits' "as long as", where the condition is all the vocabulary can say of the
/// ability.
const AS_LONG_AS: [(&str, &str, Condition); 1] = [(
    "Vadmir, New Blood",
    "As long as Vadmir has four or more +1/+1 counters on it",
    Condition::Compare(
        Amount::Counters(THIS, CounterKind::PlusOne),
        Comparison::AtLeast,
        Amount::Fixed(4),
    ),
)];

/// "[Player] controls" in the misfits' filters. Three cards asked for it of a player who is
/// neither you nor an opponent; a filter now takes any `Players` value, so the one whose
/// player has an entry is said.
///
/// Not here: "defending player controls more lands than you" (Aerial Surveyor: a `Players`
/// entry for the defending player) and "if that player controls a Plains" (Spiritual
/// Sanctuary: a `Players` entry for the player of the trigger's event).
const CONTROLS: [(&str, &str, Filter); 1] = [(
    "Hurloon Shaman",
    "Each player sacrifices a land of their choice",
    Filter::All(&[Filter::Types(Types::LAND), Filter::Whose(&THAT_PLAYER)]),
)];

/// A static ability is read with no spell or ability resolving (CR 604.1), so its text
/// says nothing only a resolution has; "that player" is such a thing, said within "each
/// player".
#[test]
fn the_misfits_static_abilities_need_no_resolution() {
    for (card, words, spec) in STATICS {
        assert!(!spec.needs_a_resolution(), "{card}: {words}");
    }
    for (card, words, affected) in AFFECTED {
        assert!(!affected.needs_a_resolution(), "{card}: {words}");
    }
    for (card, words, condition) in AS_LONG_AS {
        assert!(!condition.needs_a_resolution(), "{card}: {words}");
    }
    for (card, words, filter) in CONTROLS {
        assert!(filter.needs_a_resolution(), "{card}: {words}");
    }
}

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
        &SLAGDRILL_SCRAPPER,
        &SEVERED_STRANDS,
        &TEFERIS_CARE,
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
