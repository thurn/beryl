//! Snapcaster Mage. {1}{U} Creature — Human Wizard, 2/1. "Flash. When this creature enters,
//! target instant or sorcery card in your graveyard gains flashback until end of turn. The
//! flashback cost is equal to its mana cost."

use beryl_engine::card::{
    Ability, CardDef, Colors, Face, Flow, Keywords, ManaCost, Players, Subtype, Subtypes,
    TargetKind, TargetSpec, Trigger, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::filter::Filter;
use beryl_engine::state::TargetPlace;
use beryl_engine::{Layout, bodies};

use crate::names;

/// Flashback for the card's mana cost, an alternative cost
/// ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)): "'Flashback [cost]'
/// means 'You may cast this card from your graveyard if the resulting spell is an instant or
/// sorcery spell by paying [cost] rather than paying its mana cost' and 'If the flashback
/// cost was paid, exile this card instead of putting it anywhere else any time it would leave
/// the stack.'" The card is in its owner's graveyard, and its owner is the trigger's
/// controller.
fn grant<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    if let Some(card) = ctx.target_object(TargetPlace::FIRST) {
        ctx.grant_flashback(card, ctx.controller());
    }
    Ok(())
}

/// With no instant or sorcery card in its controller's graveyard the trigger has no target,
/// and is removed from the stack
/// ([CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d)).
pub static CARD: CardDef = CardDef {
    name: names::SNAPCASTER_MAGE,
    faces: &[Face {
        cost: ManaCost::new(1, [0, 1, 0, 0, 0, 0]),
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Human, Subtype::Wizard]),
        colors: Colors::BLUE,
        power: Some(2),
        toughness: Some(1),
        keywords: Keywords::FLASH,
        abilities: &[Ability {
            targets: &[TargetSpec::one(
                TargetKind::GraveyardCard,
                Filter::All(&[
                    Filter::Types(Types::INSTANT.union(Types::SORCERY)),
                    Filter::Whose(&Players::You),
                ]),
            )],
            ..Ability::triggered(&Trigger::ENTERS, bodies!(grant))
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::{DecisionKind, TurnStep, Zone};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::{names, props};

    const SNAPCASTER: &str = names::SNAPCASTER_MAGE;

    const SPELLBINDER: &str = names::ELITE_SPELLBINDER;

    const fn priority_candidates(candidates: u8) -> Action {
        Action::assert(Assertion::Decision {
            player: P0,
            kind: DecisionKind::Priority,
            candidates,
        })
    }

    const fn count(
        player: beryl_engine::Seat,
        zone: Zone,
        card: &'static str,
        count: u8,
    ) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card: Some(card),
            count,
        })
    }

    pub const SCENARIOS: &[Scenario] = &[
        // Flashed in against P1's Timetwister, it gives the Counterspell in P0's graveyard
        // flashback; Counterspell counters Timetwister and is exiled rather than going back to
        // the graveyard. The Plains in P1's hand keeps P1's main phase a decision afterward.
        Scenario::new("Snapcaster Mage: flashback a counterspell, then exile it")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P0,
                &[
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[names::COUNTERSPELL])
            .battlefield(
                P1,
                &[
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P1, &[names::TIMETWISTER, names::PLAINS])
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::cast(P1, names::TIMETWISTER),
                Action::cast(P0, SNAPCASTER),
                // Snapcaster, then its trigger, which no one can respond to.
                Action::Resolve,
                Action::pass(P1),
                // Counterspell resolves at once: neither player can respond.
                Action::cast(P0, names::COUNTERSPELL),
                count(P1, Zone::Graveyard, names::TIMETWISTER, 1),
                count(P0, Zone::Exile, names::COUNTERSPELL, 1),
                count(P0, Zone::Graveyard, names::COUNTERSPELL, 0),
            ]),
        // P1 answers the flashback Counterspell with Sink into Stupor, which would return it to
        // P0's hand: cast with flashback, it is exiled instead (CR 702.34a), and the creature
        // resolves.
        Scenario::new("Snapcaster Mage: a flashback spell returned to hand is exiled")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P0,
                &[
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[names::COUNTERSPELL])
            .battlefield(
                P1,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P1, &[props::CREATURE, props::REMOVAL, names::PLAINS])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P1, props::CREATURE),
                Action::cast(P0, SNAPCASTER),
                // Snapcaster resolves; P1, who could cast Sink into Stupor, passes as its trigger
                // resolves and again with the creature alone on the stack.
                Action::Resolve,
                Action::pass(P1),
                Action::pass(P1),
                Action::cast(P0, names::COUNTERSPELL),
                Action::cast(P1, props::REMOVAL),
                Action::choose(P1, DecisionKind::Target, names::COUNTERSPELL),
                Action::Resolve,
                count(P0, Zone::Exile, names::COUNTERSPELL, 1),
                count(P0, Zone::Hand, names::COUNTERSPELL, 0),
                Action::Resolve,
                Action::assert(Assertion::Controls {
                    player: P1,
                    card: props::CREATURE,
                    count: 1,
                }),
            ]),
        // Damn given flashback is cast for its mana cost only: the flashback cost is the
        // alternative cost, so overload can't be added (CR 118.9a), though P0 could pay it. The
        // two candidates are passing and the {B}{B} cast.
        Scenario::new("Snapcaster Mage: flashback is the one alternative cost")
            .interaction(&[names::DAMN])
            .battlefield(
                P0,
                &[
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::SWAMP),
                    Permanent::new(names::SWAMP),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[names::DAMN])
            .battlefield(P1, &[Permanent::new(SPELLBINDER)])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, SNAPCASTER),
                Action::Resolve,
                Action::Resolve,
                priority_candidates(2),
                Action::cast(P0, names::DAMN),
                Action::choose(P0, DecisionKind::Target, SPELLBINDER),
                Action::Resolve,
                count(P1, Zone::Graveyard, SPELLBINDER, 1),
                count(P0, Zone::Exile, names::DAMN, 1),
            ]),
        // Oust gains flashback on turn 1 and P0 leaves it; by P0's next turn it has none.
        Scenario::new("Snapcaster Mage: flashback ends with the turn")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[names::OUST])
            .library(P0, &[props::INSTANT])
            .library(P1, &[props::INSTANT])
            .actions(&[
                Action::cast(P0, SNAPCASTER),
                Action::Resolve,
                Action::Resolve,
                priority_candidates(2),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                priority_candidates(1),
                count(P0, Zone::Graveyard, names::OUST, 1),
            ]),
        // Nothing in P0's graveyard is an instant or sorcery: the trigger has no target and does
        // nothing.
        Scenario::new("Snapcaster Mage with no instant or sorcery to target")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[names::PLAINS, props::CREATURE])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, SNAPCASTER),
                Action::Resolve,
                Action::assert(Assertion::Stack { count: 0 }),
                priority_candidates(1),
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: SNAPCASTER,
                    count: 1,
                }),
            ]),
    ];
}
