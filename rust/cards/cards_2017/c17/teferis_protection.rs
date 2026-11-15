// Generated from the card store by `just card header`: notes go below the end marker.
//! Teferi's Protection {2}{W}
//! Instant
//!
//! Until your next turn, your life total can't change and you gain protection from everything. All
//! permanents you control phase out. (While they're phased out, they're treated as though they
//! don't exist. They phase in before you untap during your untap step.)
//! Exile Teferi's Protection.
// End of the generated card header.

use crate::prelude::*;

/// The instructions in order ([CR 608.2c]): "The controller of the spell or ability follows its
/// instructions in the order written."
///
/// "Your life total can't change" and "you gain protection from everything" are rule changes on
/// the controller ([CR 613.11]: "Some continuous effects affect game rules rather than
/// objects."), lasting until their next turn begins. While the first is in force they neither
/// gain nor lose life, and can't pay life ([CR 119.8]): "a cost that involves having that player
/// pay life can't be paid." The second is [CR 702.16j]: "A permanent or player with protection
/// from everything ... can't be targeted by spells or abilities and can't be enchanted by Auras.
/// ... All damage that would be dealt to such a permanent or player is prevented."
///
/// "All permanents you control phase out" ([CR 702.26b]): each is treated as though it does not
/// exist until its controller's next untap step ([CR 502.1]), and what is attached to it phases
/// out with it ([CR 702.26g]).
///
/// Exiled last, Teferi's Protection is not put into a graveyard as the final part of its
/// resolution: it is no longer on the stack.
///
/// [CR 608.2c]: https://yawgatog.com/resources/magic-rules/#R6082c
/// [CR 613.11]: https://yawgatog.com/resources/magic-rules/#R61311
/// [CR 119.8]: https://yawgatog.com/resources/magic-rules/#R1198
/// [CR 702.16j]: https://yawgatog.com/resources/magic-rules/#R70216j
/// [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
/// [CR 502.1]: https://yawgatog.com/resources/magic-rules/#R5021
/// [CR 702.26g]: https://yawgatog.com/resources/magic-rules/#R70226g
pub static CARD: CardDef = Printed::TeferisProtection
    .abilities(&[Ability::spell().effect(&sequence(&[
        change_rule(
            &RuleChange {
                players: Players::You,
                what: Governed::Player(
                    PlayerRules::LIFE_CANT_CHANGE.union(PlayerRules::PROTECTION_FROM_EVERYTHING),
                ),
                when: When::AsUsual,
            },
            Duration::UntilYourNextTurn,
        ),
        phase_out(Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::Whose(&Players::You),
        }),
        exile(this()),
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const PROTECTION: &str = Card::TeferisProtection;
    const BOLT: &str = Card::LightningBolt;
    const SCIMITAR: &str = Prop::Equipment;

    // `Prop::Flier` is a white 3/1 flier, `Prop::Vanilla` a blue 2/1 without flying.

    /// In P1's turn 2, P0 holds Teferi's Protection with three Plains to cast it and controls a
    /// blue 2/1; P1 holds Lightning Bolt with a Mountain and controls a white 3/1 flier.
    const fn in_their_turn(name: &'static str) -> Scenario {
        Scenario::new(name)
            .interaction(&[BOLT])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P0,
                &[Card::Plains, Card::Plains, Card::Plains, Prop::Vanilla],
            )
            .hand(P0, &[PROTECTION])
            .battlefield(P1, &[Card::Mountain, Prop::Flier])
            .hand(P1, &[BOLT])
            .library(P0, &[Card::Plains; 3])
            .library(P1, &[Card::Plains; 3])
    }

    pub const SCENARIOS: &[Scenario] = &[
        // [CR 702.26g]: "When a permanent phases out, any Auras, Equipment, or Fortifications
        // attached to that permanent phase out at the same time." Phased out, the 2/1, its
        // Equipment and the lands are on no battlefield; Teferi's Protection is exiled. At P0's
        // next untap step all of them phase in, the Equipment still attached (the ruling: "Each
        // Aura and Equipment that phases out attached to a permanent that's phasing out phases in
        // with that permanent and still attached to it.").
        //
        // [CR 702.26g]: https://yawgatog.com/resources/magic-rules/#R70226g
        Scenario::new("an equipped creature phases out and in with its Equipment")
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    SCIMITAR,
                    Prop::Vanilla,
                ],
            )
            .hand(P0, &[PROTECTION])
            .library(P0, &[Card::Plains; 3])
            .library(P1, &[Card::Plains; 3])
            .actions(&[
                ME.activates(SCIMITAR, 1), // Equip {1}
                Resolve,
                ME.pt(Prop::Vanilla, 3, 2),
                ME.casts(PROTECTION),
                Resolve,
                ME.has(Zone::Exile, PROTECTION, 1),
                ME.has(Zone::Battlefield, Prop::Vanilla, 0),
                ME.has(Zone::Battlefield, SCIMITAR, 0),
                ME.has(Zone::Battlefield, Card::Plains, 0),
                advance_to(3, TurnStep::Upkeep),
                ME.has(Zone::Battlefield, Prop::Vanilla, 1),
                ME.has(Zone::Battlefield, SCIMITAR, 1),
                ME.pt(Prop::Vanilla, 3, 2),
                ME.tapped(Card::Plains, 0),
            ]),
        // The ruling: "A permanent phasing out causes a spell or ability on the stack to have an
        // illegal target if it targets that permanent." Lightning Bolt's one target is illegal,
        // so it does nothing ([CR 608.2b]), and the 2/1 phases in at P0's untap step.
        //
        // [CR 608.2b]: https://yawgatog.com/resources/magic-rules/#R6082b
        in_their_turn("a creature phased out in response to Lightning Bolt survives").actions(&[
            THEM.casts(BOLT),
            THEM.targets(Prop::Vanilla),
            ME.casts(PROTECTION),
            Resolve,
            ME.has(Zone::Battlefield, Prop::Vanilla, 0),
            advance_to(2, TurnStep::BeginningOfCombat),
            THEM.has(Zone::Graveyard, BOLT, 1),
            THEM.passes(),
            THEM.done_attacking(),
            advance_to(3, TurnStep::Upkeep),
            ME.has(Zone::Battlefield, Prop::Vanilla, 1),
            ME.has(Zone::Graveyard, Prop::Vanilla, 0),
        ]),
        // [CR 702.16j]: a player with protection from everything "can't be targeted by spells or
        // abilities". Lightning Bolt may target only P1 and P1's own creature: P0 is protected,
        // and P0's creature is phased out.
        //
        // [CR 702.16j]: https://yawgatog.com/resources/magic-rules/#R70216j
        in_their_turn("Lightning Bolt can't target a player with protection from everything")
            .actions(&[
                THEM.passes(),
                ME.casts(PROTECTION),
                Resolve,
                THEM.casts(BOLT),
                THEM.deciding(DecisionKind::Target, 2),
                THEM.targets(Prop::Flier),
                Resolve,
                THEM.has(Zone::Graveyard, Prop::Flier, 1),
            ]),
        // The ruling: "Creatures can still attack you while you have protection from everything,
        // although combat damage that they would deal to you will be prevented." P1's 3/1
        // attacks an empty board; P0's life total stays at 20.
        in_their_turn("an attack into an empty board deals no damage").actions(&[
            THEM.passes(),
            ME.casts(PROTECTION),
            Resolve,
            advance_to(2, TurnStep::BeginningOfCombat),
            THEM.passes(),
            THEM.attacks_with(Prop::Flier),
            advance_to(2, TurnStep::EndOfCombat),
            ME.life(20),
            THEM.life(20),
        ]),
        // The ruling: "Any creatures that phase in under your control as your next untap step
        // begins will be able to attack and pay a cost of {T} during that turn." The 2/1 that
        // phased in attacks for 2, and P0's life total can change again.
        in_their_turn("a creature that phased in attacks that turn").actions(&[
            THEM.passes(),
            ME.casts(PROTECTION),
            Resolve,
            advance_to(2, TurnStep::BeginningOfCombat),
            THEM.passes(),
            THEM.done_attacking(),
            advance_to(3, TurnStep::BeginningOfCombat),
            ME.passes(),
            THEM.passes(),
            ME.attacks_with(Prop::Vanilla),
            THEM.passes(),
            THEM.answers(DecisionKind::Blocker, Pick::Done),
            advance_to(3, TurnStep::EndOfCombat),
            THEM.life(18),
        ]),
    ];
}
