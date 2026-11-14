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
    const SNAPCASTER: &str = Prop::SmallCreature;

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
                ME.activates(FOOD_TOKEN, 0),
                Resolve,
                ME.life(23),
                ME.has(Zone::Battlefield, FOOD_TOKEN, 0),
            ]),
        // `Prop::Creature` is a 3/1 flier.
        Scenario::new("+1 makes an opponent's creature a 3/3 Elk")
            .battlefield(P0, &[OKO])
            .battlefield(P1, &[Prop::Creature])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(OKO, 1), // +1
                Resolve,
                ME.has_loyalty(OKO, 5),
                THEM.pt(Prop::Creature, 3, 3),
                // It lasts past the turn.
                advance_to(2, TurnStep::Upkeep),
                THEM.pt(Prop::Creature, 3, 3),
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
                ME.attacks_with(ADELINE),
                ME.tapped(ADELINE, 1),
                ME.controls(crate::tokens::HUMAN_TOKEN, 0),
            ]),
        // Leonin Scimitar on P0's 2/1 makes it a 3/2. The Equipment's +1/+1 applies in layer 7c,
        // after the Elk's base 3/3 of layer 7b, whichever began first (the card's ruling:
        // "Effects that modify a creature's power and/or toughness ... will apply to the creature
        // no matter when they started to take effect").
        Scenario::new("+1's Elk wearing an Equipment attached before")
            .battlefield(P0, &[OKO, SCIMITAR, SNAPCASTER, Card::Island])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(SCIMITAR, 1), // Equip {1}
                Resolve,
                ME.pt(SNAPCASTER, 3, 2),
                ME.activates(OKO, 1), // +1
                ME.targets(SNAPCASTER),
                Resolve,
                ME.pt(SNAPCASTER, 4, 4),
            ]),
        Scenario::new("+1's Elk wearing an Equipment attached after")
            .battlefield(P0, &[OKO, SCIMITAR, SNAPCASTER, Card::Island])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(OKO, 1), // +1
                ME.targets(SNAPCASTER),
                Resolve,
                ME.pt(SNAPCASTER, 3, 3),
                ME.activates(SCIMITAR, 1), // Equip {1}
                Resolve,
                ME.pt(SNAPCASTER, 4, 4),
            ]),
        // The attached Leonin Scimitar made an Elk is a creature and no Equipment, so it can equip
        // nothing and becomes unattached (CR 301.5c, CR 704.5n): the 2/1 it was on loses its
        // +1/+1.
        Scenario::new("+1 makes an attached Equipment a 3/3 Elk")
            .battlefield(P0, &[OKO, SCIMITAR, SNAPCASTER, Card::Island])
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
                ME.pt(SNAPCASTER, 2, 1),
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
            .battlefield(P1, &[Prop::SmallCreature])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(OKO, 2), // −5
                Resolve,
                ME.controls(Prop::SmallCreature, 1),
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
            .battlefield(P1, &[BEZA, SNAPCASTER])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(OKO, 2), // −5
                Resolve,
                ME.controls(SNAPCASTER, 1),
                THEM.controls(BEZA, 1),
                THEM.controls(SCIMITAR, 1),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacking(SNAPCASTER, 0),
            ]),
        // The card's ruling: "If either of the target permanents is an illegal target when Oko's
        // last ability resolves, the exchange won't happen." P0 makes a Food with +2 on turn 1.
        // On turn 3, in response to −5, P0 sacrifices the Food it targets for 3 life, so P1
        // keeps the 2/1 (CR 701.12a).
        Scenario::new("−5 exchanges nothing when a target is gone")
            .permanents(P0, &[oko(3)])
            .battlefield(P0, &[Card::Island, Card::Island])
            .battlefield(P1, &[SNAPCASTER])
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
                THEM.controls(SNAPCASTER, 1),
                ME.controls(SNAPCASTER, 0),
                ME.has(Zone::Graveyard, OKO, 1),
            ]),
    ];
}
