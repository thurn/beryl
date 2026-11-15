// Generated from the card store by `just card header`: notes go below the end marker.
//! Mother of Runes {W}
//! Creature — Human Cleric, 1/1
//!
//! {T}: Target creature you control gains protection from the color of your choice until end of
//! turn.
// End of the generated card header.

use crate::prelude::*;

/// "{T}: Target creature you control gains protection from the color of your choice until end of
/// turn." Its controller chooses the color as the ability resolves ([CR 608.2d]): "If an effect
/// of a spell or ability offers any choices other than choices already made as part of casting
/// the spell, activating the ability, or otherwise putting the spell or ability on the stack,
/// the player announces these while applying the effect." Protection ([CR 702.16]) keeps the
/// creature from being targeted, enchanted, equipped and blocked by what has the color, and
/// prevents damage from sources of it.
///
/// [CR 608.2d]: https://yawgatog.com/resources/magic-rules/#R6082d
/// [CR 702.16]: https://yawgatog.com/resources/magic-rules/#R70216
pub static CARD: CardDef = Printed::MotherOfRunes
    .abilities(&[Ability::activated(Cost::TAP)
        .target(&TargetSpec::permanent(Filter::All(&[
            Filter::Types(Types::CREATURE),
            Filter::Whose(&Players::You),
        ])))
        .effect(&modify(
            target(),
            &[Modification::AddProtection(Protection::ChosenColor)],
            Duration::EndOfTurn,
        ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const MOTHER: &str = Card::MotherOfRunes;
    const BOLT: &str = Card::LightningBolt;
    const HOLY: &str = Card::HolyStrength;

    /// P0 holds Lightning Bolt and a Mountain to cast it, Mother of Runes and a white
    /// creature.
    const BOLTING: Scenario = Scenario::setup()
        .interaction(&[BOLT])
        .battlefield(P0, &[Card::Mountain, MOTHER, Prop::Flier])
        .hand(P0, &[BOLT, Card::Mountain]);

    /// P0 casts Lightning Bolt at their own white creature and, in response, activates
    /// Mother of Runes targeting it.
    const BOLT_IT: [Action; 4] = [
        ME.casts(BOLT),
        ME.targets(Prop::Flier),
        ME.activates(MOTHER, 0),
        ME.targets(Prop::Flier),
    ];

    /// In P0's turn 3, P0 attacks with their blue 2/1 into P1's white 3/1.
    const fn attacking(name: &'static str) -> Scenario {
        Scenario::new(name)
            .at(3, P0, TurnStep::PrecombatMain)
            .battlefield(P0, &[MOTHER, Prop::Vanilla])
            .battlefield(P1, &[Prop::Flier])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
    }

    pub const SCENARIOS: &[Scenario] = &[
        // [CR 702.16b]: "A permanent or player with protection can’t be targeted by spells
        // with the stated quality". Its one target illegal, the spell does nothing
        // ([CR 608.2b]).
        //
        // [CR 702.16b]: https://yawgatog.com/resources/magic-rules/#R70216b
        // [CR 608.2b]: https://yawgatog.com/resources/magic-rules/#R6082b
        BOLTING
            .titled("protection from red makes Lightning Bolt do nothing")
            .actions(&[
                BOLT_IT[0],
                BOLT_IT[1],
                BOLT_IT[2],
                BOLT_IT[3],
                ME.chooses_color(Colors::RED),
                ME.has(Zone::Graveyard, BOLT, 1),
                ME.has(Zone::Battlefield, Prop::Flier, 1),
                ME.tapped(MOTHER, 1),
            ]),
        BOLTING
            .titled("protection from blue doesn't stop Lightning Bolt")
            .actions(&[
                BOLT_IT[0],
                BOLT_IT[1],
                BOLT_IT[2],
                BOLT_IT[3],
                ME.chooses_color(Colors::BLUE),
                ME.has(Zone::Graveyard, Prop::Flier, 1),
            ]),
        // "You control": P1's creature is no target, so Mother and P0's creature are the
        // two.
        Scenario::new("Mother of Runes targets only a creature its controller controls")
            .battlefield(P0, &[MOTHER, Prop::Flier])
            .battlefield(P1, &[Prop::Vanilla])
            .hand(P0, &[Card::Island])
            .actions(&[
                ME.activates(MOTHER, 0),
                ME.deciding(DecisionKind::Target, 2),
                ME.targets(Prop::Flier),
                ME.chooses_color(Colors::GREEN),
                ME.tapped(MOTHER, 1),
            ]),
        // [CR 702.16c]: "Such Auras attached to the permanent or player with protection will
        // be put into their owners’ graveyards as a state-based action."
        //
        // [CR 702.16c]: https://yawgatog.com/resources/magic-rules/#R70216c
        Scenario::new("protection from white makes a white Aura fall off")
            .interaction(&[HOLY])
            .battlefield(P0, &[Card::Plains, MOTHER, Prop::Vanilla])
            .hand(P0, &[HOLY, Card::Island])
            .actions(&[
                ME.casts(HOLY),
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.pt(Prop::Vanilla, 3, 3),
                ME.activates(MOTHER, 0),
                ME.targets(Prop::Vanilla),
                ME.chooses_color(Colors::WHITE),
                ME.has(Zone::Graveyard, HOLY, 1),
                ME.pt(Prop::Vanilla, 2, 1),
            ]),
        // [CR 702.16f]: "Attacking creatures with protection can’t be blocked by creatures
        // that have the stated quality." P1's white creature can't block, so P1 takes 2.
        //
        // [CR 702.16f]: https://yawgatog.com/resources/magic-rules/#R70216f
        attacking("an attacker with protection from white can't be blocked by a white creature")
            .actions(&[
                advance_to(3, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Vanilla),
                ME.done_attacking(),
                ME.activates(MOTHER, 0),
                ME.targets(Prop::Vanilla),
                ME.chooses_color(Colors::WHITE),
                advance_to(3, TurnStep::EndOfCombat),
                THEM.life(18),
                ME.has(Zone::Battlefield, Prop::Vanilla, 1),
            ]),
        // [CR 702.16e]: "Any damage that would be dealt by sources that have the stated
        // quality to a permanent or player with protection is prevented." Blocked first, the
        // attacker stays blocked; the white blocker's 3 damage is prevented, and the attacker's
        // 2 kill it.
        //
        // [CR 702.16e]: https://yawgatog.com/resources/magic-rules/#R70216e
        attacking("protection from white prevents a white blocker's combat damage").actions(&[
            advance_to(3, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(Prop::Vanilla),
            ME.done_attacking(),
            ME.passes(),
            THEM.chooses(DecisionKind::Blocker, Prop::Flier),
            ME.activates(MOTHER, 0),
            ME.targets(Prop::Vanilla),
            ME.chooses_color(Colors::WHITE),
            advance_to(3, TurnStep::EndOfCombat),
            ME.has(Zone::Battlefield, Prop::Vanilla, 1),
            THEM.has(Zone::Graveyard, Prop::Flier, 1),
            THEM.life(20),
        ]),
        // [CR 602.5a]: "A creature's activated ability with the tap symbol ({T}) or the untap
        // symbol ({Q}) in its activation cost can't be activated unless the creature has been
        // under its controller's control since the start of their most recent turn." Only passing and playing the
        // Island are offered.
        //
        // [CR 602.5a]: https://yawgatog.com/resources/magic-rules/#R6025a
        Scenario::new("a summoning-sick Mother of Runes can't activate")
            .battlefield(P0, &[Card::Plains])
            .hand(P0, &[MOTHER, Card::Island])
            .actions(&[
                ME.casts(MOTHER),
                Resolve,
                ME.deciding(DecisionKind::Priority, 2),
            ]),
        // [CR 608.2b]: with its only target gone, the ability doesn't resolve, so no color is
        // chosen and the creature gains nothing.
        //
        // [CR 608.2b]: https://yawgatog.com/resources/magic-rules/#R6082b
        BOLTING
            .titled("Mother's ability fizzles when its target is gone")
            .actions(&[
                ME.activates(MOTHER, 0),
                ME.targets(Prop::Flier),
                ME.casts(BOLT),
                ME.targets(Prop::Flier),
                Resolve,
                ME.has(Zone::Graveyard, Prop::Flier, 1),
                stack(1),
                Resolve,
                ME.deciding(DecisionKind::Priority, 2),
            ]),
        // [CR 514.2]: "all "until end of turn" and "this turn" effects end", so on the opponent's turn the
        // creature can be targeted by a red spell again.
        //
        // [CR 514.2]: https://yawgatog.com/resources/magic-rules/#R5142
        Scenario::new("protection from Mother of Runes ends at end of turn")
            .battlefield(P0, &[MOTHER, Prop::Flier])
            .battlefield(P1, &[Card::Mountain])
            .hand(P1, &[BOLT])
            .library(P0, &[Card::Plains; 3])
            .library(P1, &[Card::Plains; 3])
            .interaction(&[BOLT])
            .actions(&[
                ME.activates(MOTHER, 0),
                ME.targets(Prop::Flier),
                Resolve,
                ME.chooses_color(Colors::RED),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                THEM.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(BOLT),
                THEM.targets(Prop::Flier),
                Resolve,
                ME.has(Zone::Graveyard, Prop::Flier, 1),
            ]),
        // [CR 702.16e]: damage from a source with the quality is prevented, and
        // [CR 616.1] lets the affected player order Furnace of Rath's doubling and the
        // prevention; either order leaves no damage dealt to the protected attacker.
        //
        // [CR 702.16e]: https://yawgatog.com/resources/magic-rules/#R70216e
        // [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
        attacking("Furnace of Rath doesn't get damage past protection")
            .battlefield(P0, &[MOTHER, Prop::Vanilla, Card::FurnaceOfRath])
            .actions(&[
                advance_to(3, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Vanilla),
                ME.done_attacking(),
                ME.passes(),
                THEM.chooses(DecisionKind::Blocker, Prop::Flier),
                ME.activates(MOTHER, 0),
                ME.targets(Prop::Vanilla),
                ME.chooses_color(Colors::WHITE),
                advance_to(3, TurnStep::EndOfCombat),
                ME.has(Zone::Battlefield, Prop::Vanilla, 1),
                THEM.has(Zone::Graveyard, Prop::Flier, 1),
            ]),
        // Protection only stops targeting, damage, enchanting/equipping and blocking
        // ([CR 702.16]): Damn overloaded doesn't target, and black isn't white, so it destroys
        // the creature that has protection from black.
        //
        // [CR 702.16]: https://yawgatog.com/resources/magic-rules/#R70216
        Scenario::new("protection doesn't save a creature from a spell that doesn't target")
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    MOTHER,
                    Prop::Vanilla,
                ],
            )
            .hand(P0, &[Card::Damn])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(MOTHER, 0),
                ME.targets(Prop::Vanilla),
                ME.chooses_color(Colors::BLACK),
                Action::Cast {
                    player: P0,
                    card: Card::Damn,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                ME.has(Zone::Graveyard, Prop::Vanilla, 1),
            ]),
        // [CR 702.16d]: "A permanent with protection can't be equipped by Equipment that have
        // the stated quality", and the Equipment becomes unattached as a state-based action
        // ([CR 704.5n]). Painter's Servant makes the colorless Equipment red.
        //
        // [CR 702.16d]: https://yawgatog.com/resources/magic-rules/#R70216d
        // [CR 704.5n]: https://yawgatog.com/resources/magic-rules/#R7045n
        Scenario::new("protection from red unattaches an Equipment Painter's Servant made red")
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::LeoninScimitar,
                    MOTHER,
                    Prop::Vanilla,
                ],
            )
            .hand(P0, &[Card::PaintersServant])
            .actions(&[
                ME.activates(Card::LeoninScimitar, 1),
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.pt(Prop::Vanilla, 3, 2),
                ME.casts(Card::PaintersServant),
                Resolve,
                ME.chooses_color(Colors::RED),
                ME.activates(MOTHER, 0),
                ME.targets(Prop::Vanilla),
                ME.chooses_color(Colors::RED),
                ME.pt(Prop::Vanilla, 2, 1),
                ME.has(Zone::Battlefield, Card::LeoninScimitar, 1),
            ]),
    ];
}
