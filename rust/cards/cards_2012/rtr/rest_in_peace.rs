// Generated from the card store by `just card header`: notes go below the end marker.
//! Rest in Peace {1}{W}
//! Enchantment
//!
//! When this enchantment enters, exile all graveyards.
//! If a card or token would be put into a graveyard from anywhere, exile it instead.
// End of the generated card header.

use crate::prelude::*;

/// The enters ability exiles every card in every graveyard.
///
/// The second ability is a replacement effect ([CR 614.1a]): "Effects that use the word
/// 'instead' are replacement effects." A creature exiled instead of being put into a graveyard
/// never dies, so nothing triggers on its dying ([CR 614.6]): "If an event is replaced, it never
/// happens." A card discarded and exiled instead was still discarded, so what triggers on a
/// discard still triggers. Rest in Peace itself is on the battlefield as it is destroyed, so it
/// is exiled too.
///
/// [CR 614.1a]: https://yawgatog.com/resources/magic-rules/#R6141a
/// [CR 614.6]: https://yawgatog.com/resources/magic-rules/#R6146
pub static CARD: CardDef = Printed::RestInPeace
    .abilities(&[
        when_enters(&exile(Objects::All {
            zone: Zone::Graveyard,
            filter: Filter::Any,
        })),
        Ability::replacing(&Replacement::put_into(
            &Filter::Any,
            Zone::Graveyard,
            Zone::Exile,
        )),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const RIP: &str = Card::RestInPeace;
    const SPHINX: &str = Card::SphinxOfFalseConclusions;
    const CONVERTER: &str = Card::CurrencyConverter;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Rest in Peace exiles all graveyards as it enters")
            .battlefield(P0, &[Card::Plains, Card::Island])
            .hand(P0, &[RIP])
            .graveyard(P0, &[Card::Counterspell, Card::Island])
            .graveyard(P1, &[Card::Oust])
            .actions(&[
                ME.casts(RIP),
                Resolve,
                Resolve,
                ME.controls(RIP, 1),
                ME.has_cards(Zone::Graveyard, 0),
                THEM.has_cards(Zone::Graveyard, 0),
                ME.has(Zone::Exile, Card::Counterspell, 1),
                ME.has(Zone::Exile, Card::Island, 1),
                THEM.has(Zone::Exile, Card::Oust, 1),
            ]),
        // Destroyed, the Sphinx is exiled instead: it never dies, so its "when this creature
        // dies" ability makes no copy of it. Get Lost resolves, and is exiled instead of going to
        // its owner's graveyard.
        Scenario::new("a creature destroyed under Rest in Peace is exiled and doesn't die")
            .interaction(&[Card::GetLost, SPHINX])
            .battlefield(P0, &[RIP, Card::Plains, Card::Plains])
            .hand(P0, &[Card::GetLost])
            .battlefield(P1, &[SPHINX])
            .actions(&[
                ME.casts(Card::GetLost),
                ME.targets(SPHINX),
                Resolve,
                stack(0),
                THEM.has(Zone::Exile, SPHINX, 1),
                THEM.has(Zone::Graveyard, SPHINX, 0),
                THEM.controls(SPHINX, 0),
                ME.has(Zone::Exile, Card::GetLost, 1),
                ME.has(Zone::Graveyard, Card::GetLost, 0),
            ]),
        // Two Rest in Peace would both exile the Sphinx: one exile, and nothing to choose.
        Scenario::new("two Rest in Peace exile a creature once")
            .interaction(&[Card::GetLost, SPHINX])
            .battlefield(P0, &[RIP, RIP, Card::Plains, Card::Plains])
            .hand(P0, &[Card::GetLost])
            .battlefield(P1, &[SPHINX])
            .actions(&[
                ME.casts(Card::GetLost),
                ME.targets(SPHINX),
                Resolve,
                THEM.has(Zone::Exile, SPHINX, 1),
                THEM.controls(SPHINX, 0),
                ME.controls(RIP, 2),
            ]),
        // Rest in Peace is on the battlefield as it is destroyed, so it is exiled; Get Lost
        // finishes resolving after it has left, and goes to the graveyard.
        Scenario::new("Rest in Peace destroyed by a spell is exiled; the spell is not")
            .interaction(&[Card::GetLost])
            .battlefield(P0, &[Card::Plains, Card::Plains])
            .hand(P0, &[Card::GetLost])
            .battlefield(P1, &[RIP])
            .actions(&[
                ME.casts(Card::GetLost),
                THEM.has(Zone::Exile, RIP, 1),
                THEM.has(Zone::Graveyard, RIP, 0),
                ME.has(Zone::Graveyard, Card::GetLost, 1),
            ]),
        // The loot discards Oust, which is exiled instead: it was still discarded, so the
        // Converter's discard trigger triggers. Oust never reached the graveyard, so the trigger
        // finds nothing to exile with the Converter and asks nothing.
        Scenario::new("a card discarded under Rest in Peace is exiled and still discarded")
            .interaction(&[CONVERTER])
            .battlefield(P0, &[RIP, CONVERTER, Card::Island, Card::Island])
            .hand(P0, &[Card::Oust])
            .library(P0, &[Card::Plains, Card::Plains, Card::Plains])
            .actions(&[
                ME.activates(CONVERTER, 1),
                Resolve,
                ME.picks(Card::Oust),
                stack(1),
                Resolve,
                ME.has(Zone::Exile, Card::Oust, 1),
                ME.has(Zone::Graveyard, Card::Oust, 0),
                ME.has(Zone::Hand, Card::Plains, 1),
            ]),
        // A spell that is countered is put into its owner's graveyard (CR 701.6a), and is
        // exiled instead.
        Scenario::new("a countered spell is exiled")
            .battlefield(P0, &[Card::Island, Card::Island])
            .hand(P0, &[Card::Counterspell])
            .battlefield(P1, &[RIP, Card::Plains])
            .hand(P1, &[Prop::Artifact, Card::Plains])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(Prop::Artifact),
                ME.casts(Card::Counterspell),
                THEM.has(Zone::Exile, Prop::Artifact, 1),
                THEM.has(Zone::Graveyard, Prop::Artifact, 0),
                ME.has(Zone::Exile, Card::Counterspell, 1),
            ]),
        // The legend rule puts one Thalia into the graveyard (CR 704.5j), where she's exiled
        // instead: she didn't die, so Blood Artist doesn't trigger (CR 614.6).
        Scenario::new("the legend rule's creature is exiled and doesn't die")
            .interaction(&[Card::BloodArtist, Card::ThaliaGuardianOfThraben])
            .battlefield(
                P0,
                &[
                    RIP,
                    Card::BloodArtist,
                    Card::ThaliaGuardianOfThraben,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .hand(P0, &[Card::ThaliaGuardianOfThraben])
            .actions(&[
                ME.casts(Card::ThaliaGuardianOfThraben),
                Resolve,
                ME.picks(Card::ThaliaGuardianOfThraben),
                ME.has(Zone::Exile, Card::ThaliaGuardianOfThraben, 1),
                ME.controls(Card::ThaliaGuardianOfThraben, 1),
                ME.has_cards(Zone::Graveyard, 0),
                ME.life(20),
                THEM.life(20),
            ]),
        // Malcolm can find the card his ability discarded in exile and offer to cast it.
        Scenario::new("Malcolm finds the discarded creature that Rest in Peace exiled")
            .interaction(&[Card::MalcolmAlluringScoundrel])
            .permanents(
                P0,
                &[
                    Permanent::new(RIP),
                    Permanent::new(Card::MalcolmAlluringScoundrel).counters(CounterKind::Chorus, 3),
                ],
            )
            .library(P0, &[Prop::Flier])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Card::MalcolmAlluringScoundrel),
                advance_to(1, TurnStep::CombatDamage),
                Resolve,
                ME.has(Zone::Exile, Prop::Flier, 1),
                ME.chooses(DecisionKind::CastOffer, Prop::Flier),
                Resolve,
                ME.controls(Prop::Flier, 1),
                ME.has(Zone::Graveyard, Prop::Flier, 0),
            ]),
    ];
}
