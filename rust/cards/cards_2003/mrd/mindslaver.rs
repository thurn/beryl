// Generated from the card store by `just card header`: notes go below the end marker.
//! Mindslaver {6}
//! Legendary Artifact
//!
//! {4}, {T}, Sacrifice Mindslaver: You control target player during that player's next turn. (You
//! see all cards that player could see and make all decisions for them.)
// End of the generated card header.

use crate::prelude::*;

/// An extra turn can be the controlled turn. The effect applies to "the next turn that
/// the affected player actually takes" ([CR 724.1]). The controller still makes decisions
/// for themself during that turn ([CR 724.8]).
///
/// [CR 724.1]: https://yawgatog.com/resources/magic-rules/#R7241
/// [CR 724.8]: https://yawgatog.com/resources/magic-rules/#R7248
pub static CARD: CardDef = Printed::Mindslaver
    .abilities(&[Ability::activated(Cost::of(
        ManaCost::generic(4),
        &[CostPart::TAP, CostPart::SACRIFICE],
    ))
    .target(&TargetSpec::player())
    .effect(&control_player(target_player()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const MINDSLAVER: &str = Card::Mindslaver;

    /// P0 after combat, with Mindslaver and the four Islands to activate it; P1 with Oust and a
    /// Plains to cast it, a creature of each player's to target, and libraries for three draws.
    const fn slaving(name: &'static str) -> Scenario {
        Scenario::new(name)
            .at(1, P0, TurnStep::PostcombatMain)
            .battlefield(
                P0,
                &[
                    MINDSLAVER,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Prop::Vanilla,
                ],
            )
            .battlefield(P1, &[Card::Plains, Prop::Flier])
            .hand(P1, &[Prop::Sorcery])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
    }

    /// [`slaving`], with Time Warp in P0's hand and five more Islands to cast it after
    /// activating Mindslaver, and an Oust for each of P1's next two turns.
    const fn warping(name: &'static str) -> Scenario {
        slaving(name)
            .interaction(&[Card::TimeWarp])
            .hand(P0, &[Card::TimeWarp])
            .hand(P1, &[Prop::Sorcery, Prop::Sorcery])
            .battlefield(
                P0,
                &[
                    MINDSLAVER,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Prop::Vanilla,
                ],
            )
    }

    pub const SCENARIOS: &[Scenario] = &[
        // CR 724.5 and 724.5a: in P1's next turn P0 casts P1's Oust, paid with P1's Plains, at
        // P1's own creature; P1 is its controller and gains the 3 life.
        slaving("the controller casts the opponent's spell at the opponent's creature").actions(&[
            ME.activates(MINDSLAVER, 0),
            ME.targets_player(THEM),
            Resolve,
            ME.has(Zone::Graveyard, MINDSLAVER, 1),
            advance_to(2, TurnStep::PrecombatMain),
            ME.casts(Prop::Sorcery),
            ME.targets(Prop::Flier),
            Resolve,
            THEM.has(Zone::Library, Prop::Flier, 1),
            THEM.tapped(Card::Plains, 1),
            THEM.life(23),
            ME.controls(Prop::Vanilla, 1),
        ]),
        // CR 724.5 and 724.8: P0 declares P1's attack into its own blocker, and as the
        // defending player declares that block for itself. Each creature dies.
        slaving("the controller attacks into their own blocker")
            .battlefield(
                P0,
                &[
                    MINDSLAVER,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Prop::Flier,
                ],
            )
            .battlefield(P1, &[Card::Plains, Prop::Vanilla])
            .actions(&[
                ME.activates(MINDSLAVER, 0),
                ME.targets_player(THEM),
                Resolve,
                advance_to(2, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Vanilla),
                ME.chooses(DecisionKind::Blocker, Prop::Flier),
                advance_to(2, TurnStep::EndOfCombat),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                ME.has(Zone::Graveyard, Prop::Flier, 1),
                ME.life(20),
            ]),
        // CR 724.1a: "Multiple player-controlling effects that affect the same player
        // overwrite each other. The last one to be created is the one that works." P1 answers
        // P0's Mindslaver with their own, on themself: in their turn they decide for themself
        // (CR 724.9).
        slaving("the last player-controlling effect works")
            .battlefield(
                P1,
                &[
                    MINDSLAVER,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Plains,
                    Prop::Flier,
                ],
            )
            .actions(&[
                ME.activates(MINDSLAVER, 0),
                ME.targets_player(THEM),
                Resolve,
                ME.passes(),
                THEM.activates(MINDSLAVER, 0),
                THEM.targets_player(THEM),
                // Their own ability alone on the stack, P1 is not asked to pass, and P0
                // passes silently with nothing to do.
                stack(0),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(Prop::Sorcery),
                THEM.targets(Prop::Vanilla),
                Resolve,
                ME.has(Zone::Library, Prop::Vanilla, 1),
            ]),
        // CR 724.1: "This effect applies to the next turn that the affected player actually
        // takes." P1's extra turn from Time Warp (CR 500.7) is turn 2 and is the controlled
        // one, so in turn 3, their regular turn, P1 decides for themself.
        warping("an extra turn of the controlled player's is the controlled one").actions(&[
            ME.activates(MINDSLAVER, 0),
            ME.targets_player(THEM),
            Resolve,
            ME.casts(Card::TimeWarp),
            ME.targets_player(THEM),
            Resolve,
            advance_to(2, TurnStep::PrecombatMain),
            ME.casts(Prop::Sorcery),
            ME.targets(Prop::Flier),
            Resolve,
            THEM.has(Zone::Library, Prop::Flier, 1),
            advance_to(3, TurnStep::PrecombatMain),
            // P0's creature is the only one left, so the target is chosen without asking.
            THEM.casts(Prop::Sorcery),
            Resolve,
            ME.has(Zone::Library, Prop::Vanilla, 1),
            THEM.life(23),
            ME.life(23),
        ]),
        // CR 724.1 and 500.7: P0 takes an extra turn first, turn 2, which P1 takes no part
        // in; the control waits for turn 3, P1's next turn.
        warping("the controller's extra turn comes before the controlled turn").actions(&[
            ME.activates(MINDSLAVER, 0),
            ME.targets_player(THEM),
            Resolve,
            ME.casts(Card::TimeWarp),
            ME.targets_player(ME),
            Resolve,
            advance_to(2, TurnStep::PrecombatMain),
            ME.has(Zone::Hand, Card::Island, 1),
            THEM.has(Zone::Hand, Card::Island, 0),
            advance_to(2, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.done_attacking(),
            advance_to(3, TurnStep::PrecombatMain),
            ME.casts(Prop::Sorcery),
            ME.targets(Prop::Flier),
            Resolve,
            THEM.has(Zone::Library, Prop::Flier, 1),
            THEM.life(23),
        ]),
        // CR 724.8: the controller keeps making their own decisions. In P1's controlled turn P0
        // casts P1's Oust at P1's creature, then, as themself, responds with their own Sink into
        // Stupor on the Oust, paying with their own Islands; the Oust returns to P1's hand and
        // P1's creature stays.
        slaving("the controller also acts for themself during the controlled turn")
            .hand(P0, &[Prop::Removal])
            .battlefield(
                P0,
                &[
                    MINDSLAVER,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Prop::Vanilla,
                ],
            )
            .actions(&[
                ME.activates(MINDSLAVER, 0),
                ME.targets_player(THEM),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                ME.casts(Prop::Sorcery),
                ME.targets(Prop::Flier),
                // Their own spell alone on the stack, P1 is not asked to pass: P0 holds priority
                // as themself.
                ME.casts(Prop::Removal),
                ME.targets(Prop::Sorcery),
                // Nobody can respond, so the Sink into Stupor resolves, and then the Oust is gone.
                THEM.controls(Prop::Flier, 1),
                THEM.has(Zone::Hand, Prop::Sorcery, 1),
                ME.has(Zone::Graveyard, Prop::Removal, 1),
                ME.tapped(Card::Island, 7),
            ]),
        // CR 724.5: the controller makes the controlled player's cleanup-step discard choice
        // ("a player makes all choices and decisions the controlled player is allowed to make"),
        // here which of P1's nine cards go.
        slaving("the controller chooses the controlled player's cleanup discard")
            .hand(P1, &[Card::Plains; 8])
            .actions(&[
                ME.activates(MINDSLAVER, 0),
                ME.targets_player(THEM),
                Resolve,
                advance_to(2, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::End),
                ME.passes(),
                ME.deciding(DecisionKind::ChooseObject, 2),
            ]),
    ];
}
