// Generated from the card store by `just card header`: notes go below the end marker.
//! Tarmogoyf {1}{G}
//! Creature — Lhurgoyf, */1+*
//!
//! Tarmogoyf's power is equal to the number of card types among cards in all graveyards and its
//! toughness is equal to that number plus 1.
// End of the generated card header.

use crate::prelude::*;

const CARD_TYPES: Amount = Amount::CardTypesAmong(&Objects::All {
    zone: Zone::Graveyard,
    filter: Filter::Any,
});

/// The characteristic-defining ability applies in every zone ([CR 604.3]). In a graveyard,
/// Tarmogoyf can itself supply the creature card type.
///
/// [CR 604.3]: https://yawgatog.com/resources/magic-rules/#R6043
pub static CARD: CardDef = Printed::Tarmogoyf
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: Objects::This,
        condition: None,
        modifications: &[Modification::SetPt(
            &CARD_TYPES,
            &Amount::Plus(1, &CARD_TYPES),
        )],
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const GOYF: &str = Card::Tarmogoyf;
    const BOLT: &str = Card::LightningBolt;

    /// What P0 needs to bolt P1's Tarmogoyf in its main phase: a Mountain to cast it with,
    /// and a land still to play, which keeps the game in that main phase.
    const BOLTER: &[&str] = &[Card::Mountain];
    const BOLTER_HAND: &[&str] = &[BOLT, Card::Mountain];

    pub const SCENARIOS: &[Scenario] = &[
        // Under Rest in Peace nothing reaches a graveyard: the Lightning Bolt P0 casts at P1 is
        // exiled as it resolves, and Tarmogoyf stays 0/1 where the instant would have made it
        // 1/2.
        Scenario::new("Tarmogoyf under Rest in Peace stays 0/1")
            .interaction(&[BOLT, Card::RestInPeace])
            .battlefield(P0, &[Card::Mountain, Card::RestInPeace])
            .hand(P0, BOLTER_HAND)
            .battlefield(P1, &[GOYF])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                ME.has(Zone::Exile, BOLT, 1),
                THEM.life(17),
                THEM.pt(GOYF, 0, 1),
            ]),
        // Both graveyards count. Gold Myr is an artifact and a creature; a second Gold Myr adds
        // no card type, nor does a second creature card.
        Scenario::new("Tarmogoyf counts card types, not cards")
            .interaction(&[Card::GoldMyr])
            .battlefield(P0, &[GOYF])
            .graveyard(P0, &[Card::GoldMyr, Card::GoldMyr])
            .graveyard(P1, &[Prop::Flier])
            .actions(&[ME.pt(GOYF, 2, 3)]),
        // One Tarfire contributes both kindred and instant. The basic land and the
        // legendary creature each contribute just their card type.
        Scenario::new("kindred counts as a card type and supertypes do not")
            .interaction(&[Card::Tarfire, Card::AdelineResplendentCathar])
            .battlefield(P0, &[GOYF])
            .graveyard(P0, &[Card::Tarfire, Card::SnowCoveredIsland])
            .graveyard(P1, &[Card::AdelineResplendentCathar])
            .actions(&[ME.pt(GOYF, 4, 5)]),
        // A creature and a sorcery make it 2/3. Lightning Bolt is in its owner's graveyard by
        // the time state-based actions look at the 3 damage, and as a third card type it makes
        // Tarmogoyf 3/4, which survives.
        Scenario::new("Tarmogoyf counts the instant that damaged it")
            .interaction(&[BOLT])
            .battlefield(P0, BOLTER)
            .hand(P0, BOLTER_HAND)
            .battlefield(P1, &[GOYF])
            .graveyard(P1, &[Prop::Flier, Prop::Sorcery])
            .actions(&[
                THEM.pt(GOYF, 2, 3),
                ME.casts(BOLT),
                ME.targets(GOYF),
                THEM.controls(GOYF, 1),
                THEM.pt(GOYF, 3, 4),
            ]),
        // A creature and an instant make it 2/3, and Lightning Bolt adds no card type, so it
        // dies.
        Scenario::new("Tarmogoyf dies to damage that adds no card type")
            .interaction(&[BOLT])
            .battlefield(P0, BOLTER)
            .hand(P0, BOLTER_HAND)
            .battlefield(P1, &[GOYF])
            .graveyard(P1, &[Prop::Flier, Prop::Instant])
            .actions(&[
                ME.casts(BOLT),
                ME.targets(GOYF),
                THEM.controls(GOYF, 0),
                THEM.has(Zone::Graveyard, GOYF, 1),
                // Its ability works in the graveyard too, where it counts the creature and
                // the two instants again: 2/3.
                THEM.pt_in(Zone::Graveyard, GOYF, 2, 3),
            ]),
        // The same, but P1 sacrifices Strip Mine in response: a land in the graveyard makes
        // Tarmogoyf 3/4 before Lightning Bolt resolves, and it survives the 3 damage. With
        // nothing left to do, both players pass and the stack resolves.
        Scenario::new("Tarmogoyf grows as a land is put into a graveyard in response")
            .interaction(&[BOLT, Card::StripMine])
            .battlefield(P0, BOLTER)
            .hand(P0, BOLTER_HAND)
            .battlefield(P1, &[GOYF, Card::StripMine])
            .graveyard(P1, &[Prop::Flier, Prop::Instant])
            .actions(&[
                ME.casts(BOLT),
                ME.targets(GOYF),
                THEM.activates(Card::StripMine, 1), // {T}, Sacrifice
                THEM.targets(Card::Mountain),
                THEM.controls(GOYF, 1),
                THEM.has(Zone::Graveyard, Card::StripMine, 1),
                ME.has(Zone::Graveyard, BOLT, 1),
                THEM.pt(GOYF, 3, 4),
            ]),
        // A characteristic-defining ability works in every zone (CR 604.3), so Tarmogoyf in a
        // graveyard counts the card types there, its own creature type among them: with a
        // sorcery in the other graveyard, it is 2/3. The Mountain in P0's hand is a land still
        // to play, which keeps the game in P0's main phase.
        Scenario::new("Tarmogoyf in a graveyard counts itself")
            .hand(P0, &[Card::Mountain])
            .graveyard(P0, &[GOYF])
            .graveyard(P1, &[Prop::Sorcery])
            .actions(&[ME.pt_in(Zone::Graveyard, GOYF, 2, 3)]),
        // In a hand it counts the graveyards as well: a land and an instant make it 2/3.
        Scenario::new("Tarmogoyf in a hand counts the graveyards")
            .hand(P0, &[GOYF, Card::Mountain])
            .graveyard(P0, &[Card::Mountain])
            .graveyard(P1, &[Prop::Instant])
            .actions(&[ME.pt_in(Zone::Hand, GOYF, 2, 3)]),
        // Dress Down's creatures lose all abilities in layer 6, before Tarmogoyf's ability would
        // apply in layer 7a, so it has its printed */1+*: 0/1.
        Scenario::new("Tarmogoyf under Dress Down")
            .interaction(&[Card::DressDown])
            .battlefield(P0, &[Card::DressDown, GOYF])
            .graveyard(P0, &[Prop::Flier, Prop::Sorcery])
            .actions(&[ME.pt(GOYF, 0, 1)]),
    ];
}
