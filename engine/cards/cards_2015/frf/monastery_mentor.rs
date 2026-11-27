// Generated from the card store by `just card header`: notes go below the end marker.
//! Monastery Mentor {2}{W}
//! Creature — Human Monk, 2/2
//!
//! Prowess (Whenever you cast a noncreature spell, this creature gets +1/+1 until end of turn.)
//! Whenever you cast a noncreature spell, create a 1/1 white Monk creature token with prowess.
// End of the generated card header.

use crate::prelude::*;

pub static MONK: CardDef = Token::MonkToken.abilities(&[Ability::prowess()]).card();

pub static CARD: CardDef =
    Printed::MonasteryMentor
        .abilities(&[
            Ability::prowess(),
            Ability::triggered(&Trigger::casts(Players::You, noncreature()))
                .effect(&create_tokens(you(), &MONK, 1)),
        ])
        .card()
        .creating(&[&MONK]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const MENTOR: &str = Card::MonasteryMentor;
    const BOLT: &str = Card::LightningBolt;
    const DIMINISH: &str = Card::Diminish;
    const SPELL: &str = Prop::UntargetedSpell;
    const MONK: &str = "Monk Token";

    pub const SCENARIOS: &[Scenario] = &[
        // Prowess and the token trigger both see a noncreature spell, and neither sees a
        // creature spell. The new Monk is not on the battlefield when the spell is cast, so it
        // gets no bonus.
        Scenario::new("A noncreature spell pumps Monastery Mentor and makes a Monk")
            .battlefield(P0, &[MENTOR, Card::Island, Card::Island, Card::Island])
            .library(P0, &[Card::Island; 30])
            .library(P1, &[Card::Island; 30])
            .hand(P0, &[SPELL])
            .actions(&[
                ME.casts(SPELL),
                ME.chooses(DecisionKind::TriggerOrder, MENTOR),
                Resolve,
                Resolve,
                Resolve,
                ME.pt(MENTOR, 3, 3),
                ME.token_pt(MONK, 1, 1),
                ME.passes(),
                ME.deciding(DecisionKind::Attacker, 2),
                ME.done_attacking(),
                advance_to(2, TurnStep::Upkeep),
                ME.pt(MENTOR, 2, 2),
            ]),
        Scenario::new("A creature spell does not trigger Monastery Mentor")
            .battlefield(P0, &[MENTOR, Card::Island, Card::Island])
            .hand(P0, &[Prop::Vanilla])
            .actions(&[
                ME.casts(Prop::Vanilla),
                Resolve,
                ME.pt(MENTOR, 2, 2),
                ME.controls(MONK, 0),
            ]),
        // The Monk made by the first spell has prowess, so the second spell gives it a bonus
        // too, and every bonus ends with the turn.
        Scenario::new("A Monk token has prowess and the bonuses end at end of turn")
            .interaction(&[BOLT])
            .battlefield(
                P0,
                &[
                    MENTOR,
                    Card::Mountain,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .library(P0, &[Card::Island; 30])
            .library(P1, &[Card::Island; 30])
            .hand(P0, &[BOLT, SPELL])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.chooses(DecisionKind::TriggerOrder, MENTOR),
                Resolve,
                Resolve,
                Resolve,
                ME.casts(SPELL),
                ME.chooses(DecisionKind::TriggerOrder, MENTOR),
                ME.chooses(DecisionKind::TriggerOrder, MENTOR),
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                ME.pt(MENTOR, 4, 4),
                ME.token_pt(MONK, 2, 2),
                ME.passes(),
                ME.deciding(DecisionKind::Attacker, 2),
                ME.done_attacking(),
                advance_to(2, TurnStep::Upkeep),
                ME.pt(MENTOR, 2, 2),
                ME.token_pt(MONK, 1, 1),
            ]),
        // Prowess's bonus is a layer 7c change ([CR 613.4c]) and Diminish's 1/1 is a layer 7b
        // one ([CR 613.4b]), so the bonus from each of the three spells, Diminish's own among
        // them, is added to the 1/1: 4/4, whichever order the effects began in.
        //
        // [CR 613.4b]: https://yawgatog.com/resources/magic-rules/#R6134b
        // [CR 613.4c]: https://yawgatog.com/resources/magic-rules/#R6134c
        Scenario::new("Three prowess bonuses and Diminish leave Monastery Mentor 4/4")
            .interaction(&[BOLT, DIMINISH])
            .battlefield(P0, &[MENTOR, Card::Mountain, Card::Mountain, Card::Island])
            .hand(P0, &[BOLT, BOLT, DIMINISH])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.chooses(DecisionKind::TriggerOrder, MENTOR),
                Resolve,
                Resolve,
                Resolve,
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.chooses(DecisionKind::TriggerOrder, MENTOR),
                ME.chooses(DecisionKind::TriggerOrder, MENTOR),
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                ME.casts(DIMINISH),
                ME.targets(MENTOR),
                ME.chooses(DecisionKind::TriggerOrder, MENTOR),
                ME.chooses(DecisionKind::TriggerOrder, MENTOR),
                ME.chooses(DecisionKind::TriggerOrder, MONK),
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                ME.pt(MENTOR, 4, 4),
            ]),
    ];
}
