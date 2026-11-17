// Generated from the card store by `just card header`: notes go below the end marker.
//! Oko, Thief of Crowns {1}{G}{U}
//! Legendary Planeswalker — Oko, loyalty 4
//!
//! +2: Create a Food token. (It's an artifact with "{2}, {T}, Sacrifice this token: You gain 3
//! life.")
//! +1: Target artifact or creature loses all abilities and becomes a green Elk creature with base
//! power and toughness 3/3.
//! −5: Exchange control of target artifact or creature you control and target creature an opponent
//! controls with power 3 or less.
// End of the generated card header.

use crate::prelude::*;

/// The Food token's name: its subtype plus the word "Token" ([CR 111.4]): "If the spell or ability
/// doesn't specify the name of the token, its name is the same as its subtype(s) plus the word
/// 'Token.'"
///
/// [CR 111.4]: https://yawgatog.com/resources/magic-rules/#R1114
pub use crate::tokens::FOOD_TOKEN;

/// "+2: Create a Food token." The predefined Food token ([CR 111.10b]): "A Food token is a colorless
/// Food artifact token with "{2}, {T}, Sacrifice this token: You gain 3 life.""
///
/// "+1: Target artifact or creature loses all abilities and becomes a green Elk creature with base
/// power and toughness 3/3." One effect with parts in four layers, which begin together with one
/// timestamp ([CR 613.6]): "If an effect should be applied in different layers and/or sublayers,
/// the parts of the effect each apply in their appropriate ones." It is a creature and an Elk and
/// no other card type or creature type in layer 4 ([CR 205.1a]): "the new card type(s) replaces any
/// existing card types" and "the new subtype(s) replaces any existing subtypes from the appropriate
/// set". It keeps its supertypes, so a legendary permanent stays legendary. It is green in layer 5,
/// has no abilities in layer 6 and is a 3/3 in layer 7b. It lasts for the rest of the game
/// ([CR 611.2a]): "If no duration is stated, it lasts until the end of the game."
///
/// "−5: Exchange control of target artifact or creature you control and target creature an opponent
/// controls with power 3 or less." [CR 701.12a]: "if the entire exchange can't be completed, no
/// part of the exchange occurs." So when either target is illegal as the ability resolves, neither
/// permanent changes controller.
///
/// [CR 111.10b]: https://yawgatog.com/resources/magic-rules/#R11110b
/// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
/// [CR 205.1a]: https://yawgatog.com/resources/magic-rules/#R2051a
/// [CR 611.2a]: https://yawgatog.com/resources/magic-rules/#R6112a
/// [CR 701.12a]: https://yawgatog.com/resources/magic-rules/#R70112a
pub static CARD: CardDef = Printed::OkoThiefOfCrowns
    .abilities(&[
        Ability::loyalty(2).effect(&create_tokens(you(), &crate::tokens::FOOD, 1)),
        Ability::loyalty(1)
            .target(&TargetSpec::permanent(Filter::Types(
                Types::ARTIFACT.union(Types::CREATURE),
            )))
            .effect(&modify(
                target(),
                &[
                    Modification::LoseAllAbilities,
                    Modification::SetColors(Colors::GREEN),
                    Modification::SetTypes(Types::CREATURE),
                    Modification::SetSubtype(Subtype::Elk),
                    Modification::SetBasePt(&Amount::Fixed(3), &Amount::Fixed(3)),
                ],
                Duration::Indefinite,
            )),
        Ability::loyalty(-5)
            .targeting(&[
                TargetSpec::permanent(Filter::All(&[
                    Filter::Types(Types::ARTIFACT.union(Types::CREATURE)),
                    Filter::Whose(&Players::You),
                ])),
                TargetSpec::permanent(Filter::All(&[
                    Filter::Types(Types::CREATURE),
                    Filter::Whose(&Players::EachOpponent),
                    Filter::PowerAtMost(3),
                ])),
            ])
            .effect(&exchange_control(nth_target(0), nth_target(1))),
    ])
    .card()
    .creating(&[&crate::tokens::FOOD]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    use super::FOOD_TOKEN;

    const OKO: &str = Card::OkoThiefOfCrowns;
    const ADELINE: &str = Card::AdelineResplendentCathar;
    const BEZA: &str = Card::BezaTheBoundingSpring;
    const SCIMITAR: &str = Prop::Equipment;
    const VANILLA: &str = Prop::Vanilla;

    /// Oko with `loyalty` loyalty counters.
    const fn oko(loyalty: u8) -> Permanent {
        Permanent::new(OKO).counters(CounterKind::Loyalty, loyalty)
    }

    pub const SCENARIOS: &[Scenario] = &[
        // The Food's ability costs {2}: two Islands pay it.
        Scenario::new("+2 creates a Food, which gains 3 life")
            .battlefield(P0, &[OKO, Card::Island, Card::Island])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(OKO, 0), // +2
                Resolve,
                ME.has_loyalty(OKO, 6),
                ME.has(Zone::Battlefield, FOOD_TOKEN, 1),
                ME.creature(FOOD_TOKEN, false),
                ME.has_subtype(FOOD_TOKEN, Subtype::Food, true),
                ME.activates(FOOD_TOKEN, 0),
                Resolve,
                ME.life(23),
                ME.has(Zone::Battlefield, FOOD_TOKEN, 0),
            ]),
        Scenario::new("+1 makes an opponent's creature a 3/3 Elk")
            .battlefield(P0, &[OKO])
            .battlefield(P1, &[Prop::Flier])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(OKO, 1), // +1
                Resolve,
                ME.has_loyalty(OKO, 5),
                THEM.pt(Prop::Flier, 3, 3),
                // It lasts past the turn.
                advance_to(2, TurnStep::Upkeep),
                THEM.pt(Prop::Flier, 3, 3),
            ]),
        // Adeline is a legendary */4 with vigilance whose power is the number of creatures her
        // controller controls, and who makes a Human token whenever her controller attacks. As
        // an Elk she has no abilities: her base 3/3 is all, and she taps to attack and makes no
        // token (the card's ruling: "It's just a green Elk").
        Scenario::new("+1's Elk has no abilities")
            .interaction(&[ADELINE])
            .battlefield(P0, &[OKO, ADELINE])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                // Adeline is the one target, and once the ability resolves P0 has nothing to do
                // before combat.
                ME.activates(OKO, 1), // +1
                ME.pt(ADELINE, 3, 3),
                ME.colored(Zone::Battlefield, ADELINE, Colors::GREEN),
                ME.has_subtype(ADELINE, Subtype::Elk, true),
                ME.has_subtype(ADELINE, Subtype::Human, false),
                ME.has_subtype(ADELINE, Subtype::Knight, false),
                ME.attacks_with(ADELINE),
                ME.tapped(ADELINE, 1),
                ME.controls(crate::tokens::HUMAN_TOKEN, 0),
            ]),
        // Leonin Scimitar on P0's 2/1 makes it a 3/2. The Equipment's +1/+1 applies in layer 7c,
        // after the Elk's base 3/3 of layer 7b, whichever began first (the card's ruling:
        // "Effects that modify a creature's power and/or toughness ... will apply to the creature
        // no matter when they started to take effect").
        Scenario::new("+1's Elk wearing an Equipment attached before")
            .battlefield(P0, &[OKO, SCIMITAR, VANILLA, Card::Island])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(SCIMITAR, 1), // Equip {1}
                Resolve,
                ME.pt(VANILLA, 3, 2),
                ME.activates(OKO, 1), // +1
                ME.targets(VANILLA),
                Resolve,
                ME.pt(VANILLA, 4, 4),
            ]),
        Scenario::new("+1's Elk wearing an Equipment attached after")
            .battlefield(P0, &[OKO, SCIMITAR, VANILLA, Card::Island])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(OKO, 1), // +1
                ME.targets(VANILLA),
                Resolve,
                ME.pt(VANILLA, 3, 3),
                ME.activates(SCIMITAR, 1), // Equip {1}
                Resolve,
                ME.pt(VANILLA, 4, 4),
            ]),
        // The attached Leonin Scimitar made an Elk is a creature and no Equipment, so it can equip
        // nothing and becomes unattached (CR 301.5c, CR 704.5n): the 2/1 it was on loses its
        // +1/+1.
        Scenario::new("+1 makes an attached Equipment a 3/3 Elk")
            .battlefield(P0, &[OKO, SCIMITAR, VANILLA, Card::Island])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(SCIMITAR, 1), // Equip {1}
                Resolve,
                ME.activates(OKO, 1), // +1
                ME.targets(SCIMITAR),
                Resolve,
                ME.creature(SCIMITAR, true),
                ME.pt(SCIMITAR, 3, 3),
                ME.pt(VANILLA, 2, 1),
            ]),
        // The card's ruling: an artifact made an Elk "loses any other card types it has (such as
        // artifact)". P1's Leonin Scimitar, made an Elk, is no artifact for a kicked Thieving
        // Skydiver to take: its trigger has no target and is removed (CR 603.3d).
        Scenario::new("+1's Elk is no longer an artifact")
            .battlefield(P0, &[OKO, Card::Island, Card::Island, Card::Island])
            .hand(P0, &[Prop::Thief])
            .battlefield(P1, &[SCIMITAR])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(OKO, 1), // +1
                Resolve,
                THEM.creature(SCIMITAR, true),
                Action::Cast {
                    player: P0,
                    card: Prop::Thief,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                // Three Islands kick it for 1 only, so X asks nothing.
                stack(0),
                THEM.controls(SCIMITAR, 1),
            ]),
        // −5 trades P0's Leonin Scimitar for P1's 2/1; Oko, at 0 loyalty, goes to the graveyard.
        Scenario::new("−5 exchanges an artifact for a small creature")
            .permanents(P0, &[oko(5)])
            .battlefield(P0, &[Prop::Equipment])
            .battlefield(P1, &[Prop::Vanilla])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(OKO, 2), // −5
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                THEM.controls(Prop::Equipment, 1),
                ME.has(Zone::Graveyard, OKO, 1),
            ]),
        // P1's Beza is a 4/5, which "power 3 or less" refuses, so P1's 2/1 is −5's only second
        // target. The 2/1 has not been under P0's control since P0's turn began, so it can't
        // attack this turn (CR 302.6).
        Scenario::new("−5 takes only a creature with power 3 or less")
            .interaction(&[BEZA])
            .permanents(P0, &[oko(6)])
            .battlefield(P0, &[SCIMITAR])
            .battlefield(P1, &[BEZA, VANILLA])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(OKO, 2), // −5
                Resolve,
                ME.controls(VANILLA, 1),
                THEM.controls(BEZA, 1),
                THEM.controls(SCIMITAR, 1),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacking(VANILLA, 0),
            ]),
        // The card's ruling: "If either of the target permanents is an illegal target when Oko's
        // last ability resolves, the exchange won't happen." P0 makes a Food with +2 on turn 1.
        // On turn 3, in response to −5, P0 sacrifices the Food it targets for 3 life, so P1
        // keeps the 2/1 (CR 701.12a).
        Scenario::new("−5 exchanges nothing when a target is gone")
            .permanents(P0, &[oko(3)])
            .battlefield(P0, &[Card::Island, Card::Island])
            .battlefield(P1, &[VANILLA])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.activates(OKO, 0), // +2
                Resolve,
                // P1's 2/1 stays home on turn 2.
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                ME.passes(),
                THEM.done_attacking(),
                advance_to(3, TurnStep::PrecombatMain),
                ME.activates(OKO, 2), // −5
                ME.activates(FOOD_TOKEN, 0),
                Resolve,
                ME.life(23),
                Resolve,
                THEM.controls(VANILLA, 1),
                ME.controls(VANILLA, 0),
                ME.has(Zone::Graveyard, OKO, 1),
            ]),
        // Under Humility, Oko's effect began after Humility's and sets base 3/3 later in layer 7b
        // (CR 613.7), so the target is a 3/3 while the others are 1/1.
        Scenario::new("+1's Elk under an earlier Humility is 3/3")
            .interaction(&[Card::Humility])
            .battlefield(P0, &[OKO, Card::Humility, VANILLA])
            .battlefield(P1, &[Prop::Flier])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.pt(VANILLA, 1, 1),
                ME.activates(OKO, 1), // +1
                ME.targets(VANILLA),
                Resolve,
                ME.pt(VANILLA, 3, 3),
                THEM.pt(Prop::Flier, 1, 1),
            ]),
        // Humility cast after Oko's effect began applies its 1/1 later in layer 7b.
        Scenario::new("+1's Elk under a later Humility is 1/1")
            .interaction(&[Card::Humility])
            .battlefield(
                P0,
                &[
                    OKO,
                    VANILLA,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .hand(P0, &[Card::Humility])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(OKO, 1), // +1
                Resolve,
                ME.pt(VANILLA, 3, 3),
                ME.casts(Card::Humility),
                Resolve,
                ME.pt(VANILLA, 1, 1),
            ]),
        // Counters stay on the Elk and modify the new base 3/3 (CR 613.4c).
        Scenario::new("+1's Elk keeps its +1/+1 counters")
            .permanents(
                P0,
                &[Permanent::new(VANILLA).counters(CounterKind::PlusOne, 2)],
            )
            .battlefield(P0, &[OKO])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.pt(VANILLA, 4, 3),
                ME.activates(OKO, 1), // +1
                Resolve,
                ME.pt(VANILLA, 5, 5),
                ME.counters(VANILLA, CounterKind::PlusOne, 2),
            ]),
        // The Elk's ability loss is applied in layer 6, before Tarmogoyf's would define its
        // size in layer 7a, and base 3/3 is set anyway.
        Scenario::new("+1 on Tarmogoyf makes a 3/3 Elk")
            .interaction(&[Card::Tarmogoyf])
            .battlefield(P0, &[OKO, Card::Tarmogoyf])
            .graveyard(P0, &[Prop::Flier, Prop::Sorcery, Prop::Instant])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(OKO, 1), // +1
                Resolve,
                ME.pt(Card::Tarmogoyf, 3, 3),
                ME.has_subtype(Card::Tarmogoyf, Subtype::Elk, true),
                ME.has_subtype(Card::Tarmogoyf, Subtype::Lhurgoyf, false),
            ]),
        // A land creature made an Elk is no land and no Dryad: it has the Elk type alone
        // (CR 205.1a, CR 205.3d).
        Scenario::new("+1's Elk replaces a land creature's subtypes")
            .battlefield(P0, &[OKO, Card::DryadArbor])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(OKO, 1), // +1
                Resolve,
                ME.pt(Card::DryadArbor, 3, 3),
                ME.has_subtype(Card::DryadArbor, Subtype::Elk, true),
                ME.has_subtype(Card::DryadArbor, Subtype::Dryad, false),
                ME.has_subtype(Card::DryadArbor, Subtype::Forest, false),
            ]),
        // A Food token made an Elk is a 3/3 creature with no Food ability and no Food type.
        Scenario::new("+1 makes a Food token a 3/3 Elk")
            .battlefield(P0, &[OKO])
            .battlefield(P1, &[VANILLA])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.activates(OKO, 0), // +2
                Resolve,
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.done_attacking(),
                advance_to(3, TurnStep::PrecombatMain),
                ME.activates(OKO, 1), // +1
                ME.targets(FOOD_TOKEN),
                Resolve,
                ME.creature(FOOD_TOKEN, true),
                ME.token_pt(FOOD_TOKEN, 3, 3),
            ]),
        // Opalescence's enchantment creature is a creature, so Oko can target it. Oko's effect
        // sets its types to creature alone (CR 205.1a): it is no longer an enchantment, so
        // Opalescence's mana-value size no longer applies and the Elk is 3/3, not 2/2.
        Scenario::new("+1 on an Opalescence creature makes a 3/3 Elk")
            .interaction(&[Card::Opalescence, Card::DressDown])
            .battlefield(P0, &[OKO, Card::Opalescence, Card::DressDown])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.pt(Card::DressDown, 2, 2),
                ME.activates(OKO, 1), // +1
                Resolve,
                ME.pt(Card::DressDown, 3, 3),
                ME.has_subtype(Card::DressDown, Subtype::Elk, true),
            ]),
        // The Bolt leaves 3 damage on a 4/5. Oko makes that damage lethal by setting its
        // toughness to 3 before the next state-based action check.
        Scenario::new("+1 makes damage already on the creature lethal")
            .interaction(&[Card::LightningBolt, BEZA])
            .battlefield(P0, &[OKO, Card::Mountain])
            .battlefield(P1, &[BEZA])
            .hand(P0, &[Card::LightningBolt])
            .actions(&[
                ME.casts(Card::LightningBolt),
                ME.targets(BEZA),
                Resolve,
                THEM.pt(BEZA, 4, 5),
                ME.activates(OKO, 1),
                Resolve,
                THEM.has(Zone::Graveyard, BEZA, 1),
            ]),
        // Even after the temporary Crew effect expires, Oko's later type-setting effect
        // leaves the formerly reconfigured Vehicle a green 3/3 creature.
        Scenario::new("+1 lasts after a temporary animation wears off")
            .interaction(&[Card::SwiftReconfiguration])
            .battlefield(P0, &[OKO, Card::Plains, VANILLA, Prop::Flier, Prop::Flier])
            .hand(P0, &[Card::SwiftReconfiguration])
            .actions(&[
                ME.casts(Card::SwiftReconfiguration),
                ME.targets(VANILLA),
                Resolve,
                ME.creature(VANILLA, false),
                Action::activate_granted(P0, VANILLA, 0),
                Resolve,
                ME.creature(VANILLA, true),
                ME.activates(OKO, 1),
                ME.targets(VANILLA),
                Resolve,
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::Upkeep),
                ME.pt(VANILLA, 3, 3),
                ME.colored(Zone::Battlefield, VANILLA, Colors::GREEN),
            ]),
        // The Elk effect remains after the planeswalker that created it leaves.
        Scenario::new("+1 keeps an Elk after Oko leaves")
            .interaction(&[Card::GetLost])
            .battlefield(P0, &[OKO, Card::Plains, Card::Plains])
            .battlefield(P1, &[Prop::Flier])
            .hand(P0, &[Card::GetLost])
            .actions(&[
                ME.activates(OKO, 1),
                Resolve,
                ME.casts(Card::GetLost),
                ME.targets(OKO),
                Resolve,
                ME.has(Zone::Graveyard, OKO, 1),
                THEM.pt(Prop::Flier, 3, 3),
                THEM.has_subtype(Prop::Flier, Subtype::Elk, true),
            ]),
        // Vigilance granted after the Elk effect starts is still present when it attacks.
        Scenario::new("+1 allows abilities granted later")
            .interaction(&[Card::Vigilance])
            .battlefield(P0, &[OKO, Card::Plains, VANILLA])
            .hand(P0, &[Card::Vigilance])
            .actions(&[
                ME.activates(OKO, 1),
                Resolve,
                ME.casts(Card::Vigilance),
                Resolve,
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(VANILLA),
                ME.tapped(VANILLA, 0),
            ]),
        // The Equipment keeps its controller and stays attached when Oko trades its wearer.
        Scenario::new("−5 exchanges a creature without exchanging its Equipment")
            .permanents(P0, &[oko(5)])
            .battlefield(P0, &[SCIMITAR, VANILLA, Card::Island])
            .battlefield(P1, &[Prop::Flier])
            .actions(&[
                ME.activates(SCIMITAR, 1),
                Resolve,
                ME.activates(OKO, 2),
                ME.targets(VANILLA),
                Resolve,
                ME.controls(SCIMITAR, 1),
                THEM.controls(VANILLA, 1),
                THEM.pt(VANILLA, 3, 2),
                ME.controls(Prop::Flier, 1),
            ]),
        // Trading the Equipment leaves its +1/+1 on the same creature until it is moved.
        Scenario::new("−5 exchanges Equipment without detaching it")
            .permanents(P0, &[oko(5)])
            .battlefield(P0, &[SCIMITAR, VANILLA, Card::Island])
            .battlefield(P1, &[Prop::Flier])
            .actions(&[
                ME.activates(SCIMITAR, 1),
                Resolve,
                ME.activates(OKO, 2),
                ME.targets(SCIMITAR),
                Resolve,
                THEM.controls(SCIMITAR, 1),
                ME.pt(VANILLA, 3, 2),
            ]),
        // Once the exchange is complete, neither the turn ending nor the new creature
        // growing above 3 power hands it back to its former controller.
        Scenario::new("−5 still controls the creature after cleanup and a power increase")
            .permanents(P0, &[oko(5)])
            .battlefield(P0, &[Prop::Artifact, SCIMITAR, Card::Island])
            .battlefield(P1, &[Prop::Flier])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.activates(OKO, 2),
                ME.targets(Prop::Artifact),
                Resolve,
                ME.controls(Prop::Flier, 1),
                advance_to(3, TurnStep::PrecombatMain),
                ME.activates(SCIMITAR, 1),
                Resolve,
                ME.pt(Prop::Flier, 4, 2),
                ME.controls(Prop::Flier, 1),
                THEM.controls(Prop::Artifact, 1),
            ]),
    ];
}
