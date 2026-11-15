// Generated from the card store by `just card header`: notes go below the end marker.
//! Show and Tell {2}{U}
//! Sorcery
//!
//! Each player may put an artifact, creature, enchantment, or land card from their hand onto the
//! battlefield.
// End of the generated card header.

use crate::prelude::*;

/// "Each player may put an artifact, creature, enchantment, or land card from their hand onto the
/// battlefield." Each player chooses in turn order from the active player, and then the cards are
/// put onto the battlefield at once ([CR 101.4]): "the active player ... makes any choices
/// required, then the next player in turn order ... Then the actions happen simultaneously." A card
/// chosen from a hand stays face down as it is chosen ([CR 101.4a]), so the player choosing second
/// does not know what the first chose: the pick is hidden. Each card enters under the control of
/// the player who put it there, its owner, since it came from that player's hand ([CR 110.2a]).
///
/// [CR 101.4]: https://yawgatog.com/resources/magic-rules/#R1014
/// [CR 101.4a]: https://yawgatog.com/resources/magic-rules/#R1014a
/// [CR 110.2a]: https://yawgatog.com/resources/magic-rules/#R1102a
pub static CARD: CardDef = Printed::ShowAndTell
    .abilities(&[Ability::spell().effect(&sequence(&[
        for_each_player(
            each_player(),
            &may_choose_hidden(
                that_player(),
                cards_in(
                    Zone::Hand,
                    &that_player(),
                    Filter::Types(
                        Types::ARTIFACT
                            .union(Types::CREATURE)
                            .union(Types::ENCHANTMENT)
                            .union(Types::LAND),
                    ),
                ),
                Prompt::new(Purpose::Look)
                    .most(1)
                    .places(Place::Battlefield, Place::None),
            ),
        ),
        put_onto_battlefield(&PutOnto {
            objects: those(),
            under: None,
            tapped: false,
            linked: false,
        }),
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SHOW: &str = Card::ShowAndTell;

    pub const SCENARIOS: &[Scenario] = &[
        // The active player chooses first, then the other player; both cards enter together.
        Scenario::new("Show and Tell puts both players' creatures onto the battlefield")
            .interaction(&[Card::Griselbrand])
            .hand(P0, &[SHOW, Card::Griselbrand])
            .battlefield(P0, &[Card::Island; 3])
            .hand(P1, &[Prop::Vanilla])
            .actions(&[
                ME.casts(SHOW),
                ME.picks_up_to(Card::Griselbrand),
                THEM.picks_up_to(Prop::Vanilla),
                ME.controls(Card::Griselbrand, 1),
                THEM.controls(Prop::Vanilla, 1),
                ME.has(Zone::Graveyard, SHOW, 1),
            ]),
        // A player may put nothing; the other player's card still enters.
        Scenario::new("Show and Tell when one player declines")
            .interaction(&[Card::Griselbrand])
            .hand(P0, &[SHOW, Card::Griselbrand])
            .battlefield(P0, &[Card::Island; 3])
            .hand(P1, &[Prop::Vanilla])
            .actions(&[
                ME.casts(SHOW),
                ME.picks_up_to(Card::Griselbrand),
                THEM.done_picking(),
                ME.controls(Card::Griselbrand, 1),
                THEM.controls(Prop::Vanilla, 0),
                THEM.has(Zone::Hand, Prop::Vanilla, 1),
            ]),
        // A land may be put. An instant stays in its owner's hand: a player whose hand holds no
        // card the filter accepts is offered only to put nothing, a question with one answer
        // the game answers for them.
        Scenario::new("Show and Tell puts a land, and no instant")
            .hand(P0, &[SHOW, Card::Plains])
            .battlefield(P0, &[Card::Island; 3])
            .hand(P1, &[Prop::Instant])
            .actions(&[
                ME.casts(SHOW),
                ME.picks_up_to(Card::Plains),
                ME.controls(Card::Plains, 1),
                THEM.has(Zone::Hand, Prop::Instant, 1),
            ]),
        // An artifact may be put, and an enchantment.
        Scenario::new("Show and Tell puts an artifact and an enchantment")
            .interaction(&[Card::BloodMoon])
            .hand(P0, &[SHOW, Prop::Artifact])
            .battlefield(P0, &[Card::Island; 3])
            .hand(P1, &[Card::BloodMoon])
            .actions(&[
                ME.casts(SHOW),
                ME.picks_up_to(Prop::Artifact),
                THEM.picks_up_to(Card::BloodMoon),
                ME.controls(Prop::Artifact, 1),
                THEM.controls(Card::BloodMoon, 1),
            ]),
        // An Aura put onto the battlefield is not cast and targets nothing: it is attached to
        // something it can enchant as it enters, chosen by the player putting it there
        // (CR 303.4f), here a creature card in a graveyard for Animate Dead.
        Scenario::new("Show and Tell puts Animate Dead onto the battlefield")
            .interaction(&[Card::AnimateDead])
            .hand(P0, &[SHOW, Card::AnimateDead])
            .battlefield(P0, &[Card::Island; 3])
            .graveyard(P1, &[Prop::Vanilla])
            .hand(P1, &[Prop::Instant])
            .actions(&[
                ME.casts(SHOW),
                ME.picks_up_to(Card::AnimateDead),
                // The one creature card in a graveyard is the one Animate Dead can enchant, and
                // its enters ability returns it.
                stack(1),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                ME.controls(Card::AnimateDead, 1),
            ]),
        // A creature card put onto the battlefield was not cast, so Containment Priest exiles
        // it instead; a land is not a creature and enters.
        Scenario::new("Show and Tell under Containment Priest")
            .interaction(&[Card::ContainmentPriest, Card::Griselbrand])
            .hand(P0, &[SHOW, Card::Griselbrand])
            .battlefield(P0, &[Card::Island; 3])
            .battlefield(P1, &[Card::ContainmentPriest])
            .hand(P1, &[Card::Island])
            .actions(&[
                ME.casts(SHOW),
                ME.picks_up_to(Card::Griselbrand),
                THEM.picks_up_to(Card::Island),
                ME.controls(Card::Griselbrand, 0),
                ME.has(Zone::Exile, Card::Griselbrand, 1),
                THEM.controls(Card::Island, 1),
            ]),
    ];
}
