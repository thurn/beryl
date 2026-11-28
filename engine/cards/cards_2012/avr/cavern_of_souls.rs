// Generated from the card store by `just card header`: notes go below the end marker.
//! Cavern of Souls
//! Land
//!
//! As this land enters, choose a creature type.
//! {T}: Add {C}.
//! {T}: Add one mana of any color. Spend this mana only to cast a creature spell of the chosen
//! type, and that spell can't be countered.
// End of the generated card header.

use crate::prelude::*;

static CANT_BE_COUNTERED: RuleChange = RuleChange {
    players: Players::EachPlayer,
    what: Governed::Counter(Objects::This),
    when: When::Never,
};

static CREATURE_OF_THE_CHOSEN_TYPE: ManaRestriction = ManaRestriction {
    cast: creature(),
    of_chosen_type: true,
    rider: Some(&CANT_BE_COUNTERED),
};

/// The colored mana can pay a qualifying spell's kicker or other additional cost. Spending
/// it there still protects the spell from counters ([CR 106.6]).
///
/// [CR 106.6]: https://yawgatog.com/resources/magic-rules/#R1066
pub static CARD: CardDef = Printed::CavernOfSouls
    .abilities(&[
        Ability::replacing(&Replacement::choose_as_enters(ChosenKind::CreatureType)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Colorless)),
        Ability::tap_for_mana(ManaSet::any_color(1)).spent_only(&CREATURE_OF_THE_CHOSEN_TYPE),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// `Prop::Vanilla` is the creature spell of the chosen type, an Elk.
    pub const SCENARIOS: &[Scenario] = &[
        // Universal Automaton has changeling, so it is an Elk as it is cast: the Cavern's colored
        // mana pays its {1}, and Counterspell does nothing to it. The Cavern's {C} could pay it
        // too, so two payments are offered, the colored first. The Elk in the library makes
        // "Elk" one of the creature types the Cavern offers.
        Scenario::new("Cavern of Souls' mana pays for a changeling of the chosen type")
            .interaction(&[Card::UniversalAutomaton])
            .hand(P0, &[Card::CavernOfSouls, Card::UniversalAutomaton])
            .library(P0, &[Prop::Vanilla])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Card::Counterspell])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Elk"),
                ME.casts(Card::UniversalAutomaton),
                ME.deciding(DecisionKind::PaymentPlan, 2),
                ME.answers(DecisionKind::PaymentPlan, Pick::Plan { i: 0 }),
                ME.tapped(Card::CavernOfSouls, 1),
                THEM.casts(Card::Counterspell),
                Resolve,
                THEM.has(Zone::Graveyard, Card::Counterspell, 1),
                ME.controls(Card::UniversalAutomaton, 1),
            ]),
        // A changeling is every creature type, including one that no card of the game names.
        Scenario::new("Cavern of Souls' mana pays for a changeling of another type")
            .interaction(&[Card::UniversalAutomaton])
            .hand(P0, &[Card::CavernOfSouls, Card::UniversalAutomaton])
            .library(P0, &[Prop::Vanilla])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Card::Counterspell])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Another creature type"),
                ME.casts(Card::UniversalAutomaton),
                ME.deciding(DecisionKind::PaymentPlan, 2),
                ME.answers(DecisionKind::PaymentPlan, Pick::Plan { i: 0 }),
                ME.tapped(Card::CavernOfSouls, 1),
                THEM.casts(Card::Counterspell),
                Resolve,
                THEM.has(Zone::Graveyard, Card::Counterspell, 1),
                ME.controls(Card::UniversalAutomaton, 1),
            ]),
        // Force of Will counters with the same effect as Counterspell, and the rider stops it too.
        Scenario::new("Cavern of Souls' creature spell can't be countered by Force of Will")
            .interaction(&[Card::ForceOfWill])
            .battlefield(P0, &[Card::Plains])
            .hand(P0, &[Card::CavernOfSouls, Prop::Vanilla])
            .battlefield(P1, &[Card::Island; 5])
            .hand(P1, &[Card::ForceOfWill])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Elk"),
                ME.casts(Prop::Vanilla),
                THEM.casts(Card::ForceOfWill),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
            ]),
        // "Counter target spell" does nothing to a spell that can't be countered, but the
        // spell is still a legal target and Mana Drain's second sentence is unconditional:
        // P1 gets {C} equal to the Elk's mana value, 2, at their next main phase.
        Scenario::new("Cavern of Souls' spell drained still gives the Mana Drain mana")
            .interaction(&[Card::ManaDrain])
            .battlefield(P0, &[Card::Plains])
            .hand(P0, &[Card::CavernOfSouls, Prop::Vanilla])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Card::ManaDrain, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .library(P0, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Elk"),
                ME.casts(Prop::Vanilla),
                THEM.casts(Card::ManaDrain),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.passes(),
                THEM.mana(ManaColor::Colorless, 2),
            ]),
        // Swamps cover the base price and the generic part of kicker. Only the
        // Cavern can pay kicker's blue mana, and that alone protects the spell.
        Scenario::new("Cavern of Souls' mana protects a spell when spent on kicker")
            .interaction(&[Card::UrborgEmissary])
            .battlefield(P0, &[Card::Swamp; 4])
            .hand(P0, &[Card::CavernOfSouls, Card::UrborgEmissary])
            .battlefield(P1, &[Card::Island, Card::Island, Prop::Flier])
            .hand(P1, &[Prop::Instant])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Wizard"),
                Action::Cast {
                    player: P0,
                    card: Card::UrborgEmissary,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.tapped(Card::Swamp, 4),
                ME.tapped(Card::CavernOfSouls, 1),
                THEM.casts(Prop::Instant),
                Resolve,
                ME.targets(Prop::Flier),
                Resolve,
                THEM.has(Zone::Graveyard, Prop::Instant, 1),
                THEM.has(Zone::Hand, Prop::Flier, 1),
                ME.controls(Card::UrborgEmissary, 1),
            ]),
        // Malcolm supplies the alternative cost. The Cavern is the only mana source,
        // and its restricted mana pays all of the Skydiver's kicker.
        Scenario::new("Cavern of Souls' mana protects a free spell when spent on kicker")
            .interaction(&[Card::MalcolmAlluringScoundrel])
            .permanents(
                P0,
                &[Permanent::new(Card::MalcolmAlluringScoundrel).counters(CounterKind::Chorus, 3)],
            )
            .hand(P0, &[Card::CavernOfSouls])
            .library(P0, &[Card::ThievingSkydiver])
            .battlefield(P1, &[Card::Island, Card::Island, Prop::Equipment])
            .hand(P1, &[Card::Counterspell])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Merfolk"),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Card::MalcolmAlluringScoundrel),
                advance_to(1, TurnStep::CombatDamage),
                Resolve,
                ME.deciding(DecisionKind::CastOffer, 3),
                ME.answers(
                    DecisionKind::CastOffer,
                    Pick::Cast {
                        card: Card::ThievingSkydiver,
                        face: FaceIx::FRONT,
                        costs: Castings::only(0),
                    },
                ),
                ME.answers(DecisionKind::PaymentPlan, Pick::Plan { i: 0 }),
                ME.tapped(Card::CavernOfSouls, 1),
                THEM.casts(Card::Counterspell),
                Resolve,
                Resolve,
                THEM.has(Zone::Graveyard, Card::Counterspell, 1),
                ME.controls(Card::ThievingSkydiver, 1),
                ME.controls(Prop::Equipment, 1),
            ]),
        // Under Mana Flare, tapping the Cavern for its colored mana also adds one unrestricted
        // mana of that color. The Gold Myr is not a land, so Mana Flare leaves it a plain
        // source. A payment once counted one mana for the Cavern and so ended with Mana Flare's
        // mana unplanned in the pool, which a debug assertion of the payment caught.
        Scenario::new("Cavern of Souls and Gold Myr pay under Mana Flare")
            .interaction(&[Card::ManaFlare, Card::GoldMyr])
            .battlefield(P0, &[Card::ManaFlare, Card::GoldMyr])
            .hand(P0, &[Card::CavernOfSouls, Prop::Vanilla])
            .library(P0, &[Card::Island; 7])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Elk"),
                ME.casts(Prop::Vanilla),
                ME.tapped(Card::CavernOfSouls, 1),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
            ]),
        // Under Mana Flare the Swamp makes {B}{B}, and the Cavern's colored ability makes two
        // mana of one color, one of them restricted. The Swamp alone can't pay the {2}{B}
        // creature, so both are tapped and one mana is left over. The first plan taps the
        // Cavern for {C}, and the second for colored mana, so Counterspell does nothing. A
        // payment once counted one mana for the Cavern, expected none to be left, and found
        // Mana Flare's in the pool.
        Scenario::new("Cavern of Souls and a Swamp pay under Mana Flare with mana left over")
            .interaction(&[Card::ManaFlare])
            .battlefield(P0, &[Card::ManaFlare, Card::Swamp])
            .hand(P0, &[Card::CavernOfSouls, Prop::FlashCreature])
            .library(P0, &[Card::Island; 7])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Card::Counterspell])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Assassin"),
                ME.casts(Prop::FlashCreature),
                ME.answers(DecisionKind::PaymentPlan, Pick::Plan { i: 1 }),
                ME.tapped(Card::CavernOfSouls, 1),
                ME.tapped(Card::Swamp, 1),
                THEM.casts(Card::Counterspell),
                Resolve,
                THEM.has(Zone::Graveyard, Card::Counterspell, 1),
                ME.controls(Prop::FlashCreature, 1),
            ]),
        // Under Mana Flare two Swamps make {B}{B}{B}{B}, enough for the Emissary's {2}{B} and the
        // generic part of its kicker. Only the Cavern can pay the kicker's {U}, and Mana Flare
        // adds a second blue mana, so one mana is left over.
        Scenario::new("Cavern of Souls pays a kicker under Mana Flare")
            .interaction(&[Card::ManaFlare, Card::UrborgEmissary])
            .battlefield(P0, &[Card::ManaFlare, Card::Swamp, Card::Swamp])
            .hand(P0, &[Card::CavernOfSouls, Card::UrborgEmissary])
            .battlefield(P1, &[Prop::Flier])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Wizard"),
                Action::Cast {
                    player: P0,
                    card: Card::UrborgEmissary,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.tapped(Card::Swamp, 2),
                ME.tapped(Card::CavernOfSouls, 1),
                Resolve,
                ME.targets(Prop::Flier),
                Resolve,
                THEM.has(Zone::Hand, Prop::Flier, 1),
                ME.controls(Card::UrborgEmissary, 1),
            ]),
    ];
}
