// Generated from the card store by `just card header`: notes go below the end marker.
//! Show and Tell {2}{U}
//! Sorcery
//!
//! Each player may put an artifact, creature, enchantment, or land card from their hand onto the
//! battlefield.
// End of the generated card header.

use crate::prelude::*;

/// Players choose in turn order starting with the active player. Each chosen card stays
/// hidden until all players have chosen ([CR 101.4a]): "If an effect has each player choose
/// a card in a hidden zone, such as their hand or library, those cards may remain face
/// down as they're chosen." The cards enter simultaneously ([CR 101.4]): "Then the actions
/// happen simultaneously."
///
/// [CR 101.4]: https://yawgatog.com/resources/magic-rules/#R1014
/// [CR 101.4a]: https://yawgatog.com/resources/magic-rules/#R1014a
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
        // The opponent can respond while the spell is on the stack. Neither player picks a
        // card until the opponent passes and the spell starts to resolve.
        Scenario::new("Show and Tell asks for cards after the opponent can respond")
            .hand(P0, &[SHOW, Prop::Flier])
            .battlefield(P0, &[Card::Island; 3])
            .hand(P1, &[Prop::Vanilla, Prop::Instant])
            .battlefield(P1, &[Card::Island; 2])
            .actions(&[
                ME.casts(SHOW),
                stack(1),
                THEM.passes(),
                ME.picks_up_to(Prop::Flier),
                THEM.picks_up_to(Prop::Vanilla),
                ME.controls(Prop::Flier, 1),
                THEM.controls(Prop::Vanilla, 1),
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
        // [CR 303.4f]: an Aura put onto the battlefield enchants something already there; the
        // ruling for Show and Tell: "If you put an Aura onto the battlefield this way, it can't
        // enchant a permanent that's entering at the same time." P1's creature enters
        // simultaneously with Holy Strength, so P0, with no other creature, has nothing for it
        // to enchant, and the Aura stays in P0's hand ([CR 303.4g]).
        //
        // [CR 303.4f]: https://yawgatog.com/resources/magic-rules/#R3034f
        // [CR 303.4g]: https://yawgatog.com/resources/magic-rules/#R3034g
        Scenario::new("Show and Tell: an Aura can't enchant a creature entering with it")
            .interaction(&[Card::HolyStrength])
            .hand(P0, &[SHOW, Card::HolyStrength])
            .battlefield(P0, &[Card::Island; 3])
            .hand(P1, &[Prop::Vanilla])
            .actions(&[
                ME.casts(SHOW),
                ME.picks_up_to(Card::HolyStrength),
                THEM.picks_up_to(Prop::Vanilla),
                THEM.controls(Prop::Vanilla, 1),
                ME.controls(Card::HolyStrength, 0),
                ME.has(Zone::Hand, Card::HolyStrength, 1),
            ]),
        // An Aura P1 puts onto the battlefield may enchant P0's creature (CR 303.4f): it is put
        // onto the battlefield under P1's control, enchanting a creature P0 controls.
        Scenario::new("Show and Tell: an Aura may enchant the opponent's creature")
            .interaction(&[Card::HolyStrength])
            .hand(P0, &[SHOW])
            .battlefield(
                P0,
                &[Card::Island, Card::Island, Card::Island, Prop::Vanilla],
            )
            .hand(P1, &[Card::HolyStrength])
            .actions(&[
                // P0's hand is empty, so only P1 is asked.
                ME.casts(SHOW),
                THEM.picks_up_to(Card::HolyStrength),
                THEM.controls(Card::HolyStrength, 1),
                ME.pt(Prop::Vanilla, 3, 3),
            ]),
        // The October 4, 2004 ruling puts entry choices after both hand picks, with the
        // active player choosing first. Both Auras choose while the flier is still a creature.
        Scenario::new("Show and Tell chooses both Auras before either attachment")
            .interaction(&[Card::SwiftReconfiguration, Card::HolyStrength])
            .hand(P0, &[SHOW, Card::SwiftReconfiguration])
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Prop::Vanilla,
                    Prop::FlashCreature,
                ],
            )
            .hand(P1, &[Card::HolyStrength])
            .battlefield(P1, &[Prop::Flier])
            .actions(&[
                ME.casts(SHOW),
                ME.picks_up_to(Card::SwiftReconfiguration),
                THEM.picks_up_to(Card::HolyStrength),
                ME.deciding(DecisionKind::ChooseObject, 3),
                ME.picks(Prop::Flier),
                THEM.deciding(DecisionKind::ChooseObject, 3),
                THEM.picks(Prop::Vanilla),
                ME.controls(Card::SwiftReconfiguration, 1),
                THEM.controls(Card::HolyStrength, 1),
                THEM.creature(Prop::Flier, false),
                THEM.has_subtype(Prop::Flier, Subtype::Vehicle, true),
                ME.pt(Prop::Vanilla, 3, 3),
            ]),
        // A permanent that enters as a copy chooses among permanents already on the battlefield,
        // so it can't copy one entering at the same time (CR 614.12). P0's
        // Phantasmal Image has nothing to copy, so it enters as a 0/0 and dies; P1's creature
        // stays.
        Scenario::new("Show and Tell: a clone can't copy a creature entering with it")
            .interaction(&[Card::PhantasmalImage])
            .hand(P0, &[SHOW, Card::PhantasmalImage])
            .battlefield(P0, &[Card::Island; 3])
            .hand(P1, &[Prop::Vanilla])
            .actions(&[
                ME.casts(SHOW),
                ME.picks_up_to(Card::PhantasmalImage),
                THEM.picks_up_to(Prop::Vanilla),
                THEM.controls(Prop::Vanilla, 1),
                ME.controls(Card::PhantasmalImage, 0),
                ME.has(Zone::Graveyard, Card::PhantasmalImage, 1),
            ]),
    ];
}
