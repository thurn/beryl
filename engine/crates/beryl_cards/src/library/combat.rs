//! Combat declarations and damage ([CR 506] through [CR 511]).
//!
//! [CR 506]: https://yawgatog.com/resources/magic-rules/#R506
//! [CR 511]: https://yawgatog.com/resources/magic-rules/#R511

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{P0, P1, Permanent, Pick, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to};

use super::keywords::trample;
use crate::{names, props};

const GROUND: &str = props::VANILLA.card;
const FLIER: &str = props::FLIER.card;
const KNIGHT: &str = names::WHITE_KNIGHT;
const DREADMAW: &str = names::COLOSSAL_DREADMAW;

pub const DECLARATIONS: &[Scenario] = &[
    Scenario::new("two interchangeable attackers are offered one copy at a time")
        .battlefield(P0, &[GROUND, GROUND, FLIER])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.deciding(DecisionKind::Attacker, 3),
            ME.attacks_with(GROUND),
            ME.deciding(DecisionKind::Attacker, 3),
            ME.attacks_with(GROUND),
            ME.done_attacking(),
            advance_to(1, TurnStep::DeclareAttackers),
            ME.attacking(GROUND, 2),
            ME.tapped(GROUND, 2),
            ME.tapped(FLIER, 0),
            advance_to(1, TurnStep::PostcombatMain),
            ME.attacking(GROUND, 0),
            THEM.life(16),
            ME.life(20),
        ]),
    Scenario::new("a tapped creature and a creature that just entered cannot attack")
        .permanents(
            P0,
            &[
                Permanent::new(GROUND).sick(),
                Permanent::new(FLIER).tapped(),
            ],
        )
        .battlefield(P1, &[GROUND])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            advance_to(1, TurnStep::PostcombatMain),
            ME.tapped(GROUND, 0),
            THEM.life(20),
        ]),
];

pub const FIRST_STRIKE: &[Scenario] = &[
    Scenario::new("a first striker kills its blocker before the blocker deals damage")
        .battlefield(P0, &[KNIGHT])
        .battlefield(P1, &[GROUND])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(KNIGHT),
            THEM.chooses(DecisionKind::Blocker, GROUND),
            advance_to(1, TurnStep::FirstStrikeDamage),
            THEM.has(Zone::Graveyard, GROUND, 1),
            ME.controls(KNIGHT, 1),
            advance_to(1, TurnStep::EndOfCombat),
            ME.controls(KNIGHT, 1),
        ]),
    Scenario::new("a first striker deals damage once and the other creatures deal it second")
        .battlefield(P0, &[KNIGHT, GROUND])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(KNIGHT),
            ME.attacks_with(GROUND),
            advance_to(1, TurnStep::FirstStrikeDamage),
            THEM.life(18),
            ME.controls(GROUND, 1),
            advance_to(1, TurnStep::CombatDamage),
            THEM.life(16),
            ME.controls(GROUND, 1),
        ]),
    Scenario::new("a blocker without first strike deals damage only in the second step")
        .battlefield(P0, &[KNIGHT])
        .battlefield(P1, &[DREADMAW])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(KNIGHT),
            THEM.chooses(DecisionKind::Blocker, DREADMAW),
            advance_to(1, TurnStep::FirstStrikeDamage),
            ME.controls(KNIGHT, 1),
            trample::two_damage(P1),
            advance_to(1, TurnStep::CombatDamage),
            ME.has(Zone::Graveyard, KNIGHT, 1),
            trample::two_damage(P1),
        ]),
    Scenario::new(
        "a first-striking trampler kills multiple blockers in the first step and tramples over",
    )
    .interaction(&[names::COORDINATED_ASSAULT, DREADMAW])
    .battlefield(P0, &[DREADMAW, names::MOUNTAIN])
    .hand(P0, &[names::COORDINATED_ASSAULT])
    .battlefield(P1, &[names::GRIZZLY_BEARS, names::SAVANNAH_LIONS])
    .actions(&[
        advance_to(1, TurnStep::BeginningOfCombat),
        ME.passes(),
        ME.attacks_with(DREADMAW),
        ME.passes(),
        THEM.chooses(DecisionKind::Blocker, names::GRIZZLY_BEARS),
        THEM.chooses(DecisionKind::Blocker, names::SAVANNAH_LIONS),
        advance_to(1, TurnStep::DeclareBlockers),
        ME.casts(names::COORDINATED_ASSAULT),
        ME.answers(DecisionKind::TargetCount, Pick::Number { n: 1 }),
        ME.targets(DREADMAW),
        Resolve,
        ME.passes(),
        ME.chooses(DecisionKind::DamageDivision, names::GRIZZLY_BEARS),
        ME.chooses(DecisionKind::DamageDivision, names::SAVANNAH_LIONS),
        advance_to(1, TurnStep::FirstStrikeDamage),
        THEM.has(Zone::Graveyard, names::GRIZZLY_BEARS, 1),
        THEM.has(Zone::Graveyard, names::SAVANNAH_LIONS, 1),
        THEM.life(16),
        advance_to(1, TurnStep::EndOfCombat),
        ME.controls(DREADMAW, 1),
        THEM.life(16),
    ]),
];

pub const DOUBLE_STRIKE: &[Scenario] = &[
    Scenario::new("Fencing Ace deals damage in both combat damage steps")
        .battlefield(P0, &[names::FENCING_ACE])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(names::FENCING_ACE),
            advance_to(1, TurnStep::FirstStrikeDamage),
            THEM.life(19),
            advance_to(1, TurnStep::CombatDamage),
            THEM.life(18),
        ]),
    Scenario::new(
        "a double striker whose blocker dies in the first step deals no damage to the player",
    )
    .battlefield(P0, &[names::FENCING_ACE])
    .battlefield(P1, &[GROUND])
    .actions(&[
        advance_to(1, TurnStep::BeginningOfCombat),
        ME.passes(),
        ME.attacks_with(names::FENCING_ACE),
        THEM.chooses(DecisionKind::Blocker, GROUND),
        advance_to(1, TurnStep::FirstStrikeDamage),
        THEM.has(Zone::Graveyard, GROUND, 1),
        ME.controls(names::FENCING_ACE, 1),
        THEM.life(20),
        advance_to(1, TurnStep::EndOfCombat),
        ME.controls(names::FENCING_ACE, 1),
        THEM.life(20),
    ]),
    Scenario::new("Palace Guard survives first strike from Fencing Ace and divides regular damage")
        .interaction(&[names::PALACE_GUARD, names::SAVANNAH_LIONS])
        .battlefield(P0, &[names::FENCING_ACE, names::SAVANNAH_LIONS])
        .battlefield(P1, &[names::PALACE_GUARD])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(names::FENCING_ACE),
            ME.attacks_with(names::SAVANNAH_LIONS),
            THEM.chooses(DecisionKind::Blocker, names::PALACE_GUARD),
            THEM.chooses(DecisionKind::BlockTarget, names::FENCING_ACE),
            THEM.chooses(DecisionKind::Blocker, names::PALACE_GUARD),
            advance_to(1, TurnStep::FirstStrikeDamage),
            THEM.controls(names::PALACE_GUARD, 1),
            ME.controls(names::FENCING_ACE, 1),
            ME.passes(),
            THEM.chooses(DecisionKind::DamageDivision, names::FENCING_ACE),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.has(Zone::Graveyard, names::PALACE_GUARD, 1),
            ME.has(Zone::Graveyard, names::FENCING_ACE, 1),
            ME.controls(names::SAVANNAH_LIONS, 1),
            THEM.life(20),
        ]),
];

pub const REMOVED_BLOCKER: &[Scenario] =
    &[
        Scenario::new("an attacker without trample stays blocked when its blocker leaves")
            .hand(P0, &[names::GET_LOST])
            .battlefield(P0, &[names::PLAINS, names::PLAINS, FLIER])
            .battlefield(P1, &[names::SERRA_ANGEL])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(FLIER),
                ME.passes(),
                THEM.chooses(DecisionKind::Blocker, names::SERRA_ANGEL),
                advance_to(1, TurnStep::DeclareBlockers),
                ME.casts(names::GET_LOST),
                ME.targets(names::SERRA_ANGEL),
                Resolve,
                advance_to(1, TurnStep::EndOfCombat),
                ME.controls(FLIER, 1),
                THEM.has(Zone::Graveyard, names::SERRA_ANGEL, 1),
                THEM.life(20),
            ]),
    ];

pub const REMOVED_PLANESWALKER: &[Scenario] =
    &[
        Scenario::new("an attacker whose planeswalker left combat deals no damage")
            .battlefield(P0, &[GROUND, names::PLAINS, names::PLAINS])
            .battlefield(P1, &[props::PLANESWALKER.card])
            .hand(P0, &[names::GET_LOST])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(GROUND),
                ME.chooses(DecisionKind::AttackTarget, props::PLANESWALKER.card),
                advance_to(1, TurnStep::DeclareAttackers),
                ME.casts(names::GET_LOST),
                ME.targets(props::PLANESWALKER.card),
                Resolve,
                ME.attacking(GROUND, 1),
                advance_to(1, TurnStep::EndOfCombat),
                ME.life(20),
                THEM.life(20),
            ]),
    ];

pub const CANNOT_BLOCK: &[Scenario] = &[
    Scenario::new("the defender is offered only creatures that can block")
        .battlefield(P0, &[GROUND])
        .battlefield(P1, &[names::BLOODGHAST, FLIER])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(GROUND),
            THEM.deciding(DecisionKind::Blocker, 2),
            THEM.chooses(DecisionKind::Blocker, FLIER),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.life(20),
            THEM.has(Zone::Graveyard, FLIER, 1),
        ]),
    Scenario::new("the defender is not asked to block when no creature can block")
        .battlefield(P0, &[GROUND])
        .battlefield(P1, &[names::BLOODGHAST])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(GROUND),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.life(18),
        ]),
];

pub const MUST_BLOCK: &[Scenario] = &[
    Scenario::new("all untapped creatures able to block a Lured creature must do so")
        .hand(P0, &[names::LURE])
        .battlefield(
            P0,
            &[names::FOREST, names::FOREST, names::FOREST, FLIER, GROUND],
        )
        .battlefield(P1, &[FLIER, props::COLORLESS_CREATURE.card, GROUND])
        .actions(&[
            ME.casts(names::LURE),
            ME.targets(FLIER),
            Resolve,
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(FLIER),
            ME.attacks_with(GROUND),
            // Done is withheld while both FLIER and the colorless flier must block P0's FLIER.
            THEM.deciding(DecisionKind::Blocker, 3),
            THEM.chooses(DecisionKind::Blocker, FLIER),
            THEM.deciding(DecisionKind::Blocker, 2),
            THEM.chooses(DecisionKind::Blocker, props::COLORLESS_CREATURE.card),
            // Both required blocks are met; Done and optional GROUND blocker are now offered.
            THEM.deciding(DecisionKind::Blocker, 2),
            THEM.answers(DecisionKind::Blocker, Pick::Done),
            ME.chooses(DecisionKind::DamageDivision, FLIER),
            ME.chooses(DecisionKind::DamageDivision, props::COLORLESS_CREATURE.card),
            advance_to(1, TurnStep::EndOfCombat),
            ME.has(Zone::Graveyard, FLIER, 1),
            ME.has(Zone::Graveyard, names::LURE, 1),
            THEM.has(Zone::Graveyard, FLIER, 1),
            THEM.has(Zone::Graveyard, props::COLORLESS_CREATURE.card, 1),
            THEM.life(18),
        ]),
    Scenario::new("a tapped creature and a ground creature need not block a Lured flier")
        .hand(P0, &[names::LURE])
        .battlefield(P0, &[names::FOREST, names::FOREST, names::FOREST, FLIER])
        .permanents(
            P1,
            &[Permanent::new(FLIER).tapped(), Permanent::new(GROUND)],
        )
        .actions(&[
            ME.casts(names::LURE),
            ME.targets(FLIER),
            Resolve,
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(FLIER),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.life(17),
            ME.controls(FLIER, 1),
        ]),
    Scenario::new("with two Lured attackers each blocker chooses which to block")
        .hand(P0, &[names::LURE, names::LURE])
        .battlefield(
            P0,
            &[
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                FLIER,
                props::COLORLESS_CREATURE.card,
                GROUND,
            ],
        )
        .battlefield(P1, &[FLIER, GROUND])
        .actions(&[
            ME.casts(names::LURE),
            ME.targets(FLIER),
            Resolve,
            ME.casts(names::LURE),
            ME.targets(props::COLORLESS_CREATURE.card),
            Resolve,
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(FLIER),
            ME.attacks_with(props::COLORLESS_CREATURE.card),
            ME.attacks_with(GROUND),
            // Done is withheld while P1's FLIER must block one of the two Lured fliers.
            THEM.deciding(DecisionKind::Blocker, 2),
            THEM.chooses(DecisionKind::Blocker, FLIER),
            THEM.deciding(DecisionKind::BlockTarget, 2),
            THEM.chooses(DecisionKind::BlockTarget, FLIER),
            // Now Done and GROUND (optional block on GROUND) are offered.
            THEM.deciding(DecisionKind::Blocker, 2),
            THEM.answers(DecisionKind::Blocker, Pick::Done),
            advance_to(1, TurnStep::EndOfCombat),
            ME.has(Zone::Graveyard, FLIER, 1),
            THEM.has(Zone::Graveyard, FLIER, 1),
            ME.controls(props::COLORLESS_CREATURE.card, 1),
            ME.controls(GROUND, 1),
            THEM.life(18),
        ]),
];

pub const MENACE: &[Scenario] = &[
    Scenario::new("a single creature cannot block an attacker with menace")
        .battlefield(P0, &[names::ALLEY_STRANGLER])
        .battlefield(P1, &[GROUND])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(names::ALLEY_STRANGLER),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.life(18),
            THEM.controls(GROUND, 1),
        ]),
    Scenario::new("blocking a menace attacker withholds Done until a second blocker is declared")
        .battlefield(P0, &[names::ALLEY_STRANGLER])
        .battlefield(P1, &[GROUND, FLIER, props::GREEN_CREATURE.card])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(names::ALLEY_STRANGLER),
            // Done + 3 blockers = 4 options.
            THEM.deciding(DecisionKind::Blocker, 4),
            THEM.chooses(DecisionKind::Blocker, GROUND),
            // After 1 blocker on menace, Done is withheld: only FLIER and the green creature are offered.
            THEM.deciding(DecisionKind::Blocker, 2),
            THEM.chooses(DecisionKind::Blocker, FLIER),
            // With 2 blockers on menace, Done and the green creature are offered.
            THEM.deciding(DecisionKind::Blocker, 2),
            THEM.answers(DecisionKind::Blocker, Pick::Done),
            ME.chooses(DecisionKind::DamageDivision, GROUND),
            ME.answers(DecisionKind::DamageDivision, Pick::Done),
            advance_to(1, TurnStep::EndOfCombat),
            ME.has(Zone::Graveyard, names::ALLEY_STRANGLER, 1),
            THEM.has(Zone::Graveyard, GROUND, 1),
            THEM.controls(FLIER, 1),
            THEM.controls(props::GREEN_CREATURE.card, 1),
            THEM.life(20),
        ]),
];

pub const MUST_ATTACK: &[Scenario] = &[
    Scenario::new("a creature that must attack withholds Done until it is declared")
        .battlefield(P0, &[names::BLOODROCK_CYCLOPS, GROUND])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            // Done is withheld: only Bloodrock Cyclops and GROUND are offered.
            ME.deciding(DecisionKind::Attacker, 2),
            ME.attacks_with(names::BLOODROCK_CYCLOPS),
            // Now Done and GROUND are offered.
            ME.deciding(DecisionKind::Attacker, 2),
            ME.done_attacking(),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.life(17),
        ]),
    Scenario::new("a tapped creature that must attack need not attack")
        .permanents(P0, &[Permanent::new(names::BLOODROCK_CYCLOPS).tapped()])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            advance_to(1, TurnStep::PostcombatMain),
            THEM.life(20),
        ]),
];

pub const ATTACK_COST: &[Scenario] = &[
    Scenario::new("under Propaganda two attackers cost {4} and four lands are tapped")
        .battlefield(
            P0,
            &[
                GROUND,
                GROUND,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
            ],
        )
        .battlefield(P1, &[names::PROPAGANDA])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(GROUND),
            // Two attackers use every creature: the declaration finishes without asking.
            ME.attacks_with(GROUND),
            advance_to(1, TurnStep::DeclareAttackers),
            ME.attacking(GROUND, 2),
            ME.tapped(names::ISLAND, 4),
            advance_to(1, TurnStep::PostcombatMain),
            THEM.life(16),
        ]),
    Scenario::new("under Propaganda three lands pay for one attacker and a second is not offered")
        .battlefield(
            P0,
            &[GROUND, GROUND, names::ISLAND, names::ISLAND, names::ISLAND],
        )
        .battlefield(P1, &[names::PROPAGANDA])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            // Done and one of the two interchangeable creatures.
            ME.deciding(DecisionKind::Attacker, 2),
            ME.attacks_with(GROUND),
            // {4} for two attackers is more than three lands make: only Done is left, so the
            // declaration finishes without asking.
            advance_to(1, TurnStep::DeclareAttackers),
            ME.attacking(GROUND, 1),
            ME.tapped(names::ISLAND, 2),
            advance_to(1, TurnStep::PostcombatMain),
            THEM.life(18),
        ]),
    Scenario::new("under Propaganda a creature that taps for mana cannot pay for its own attack")
        .interaction(&[names::GOLD_MYR])
        .battlefield(P0, &[names::GOLD_MYR, GROUND, names::ISLAND])
        .battlefield(P1, &[names::PROPAGANDA])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            // Gold Myr would tap as it attacks, leaving one Island for {2}: Done and the
            // other creature are offered.
            ME.deciding(DecisionKind::Attacker, 2),
            // Gold Myr can't attack with it: {4} for two is more than an Island makes.
            ME.attacks_with(GROUND),
            advance_to(1, TurnStep::DeclareAttackers),
            ME.attacking(names::GOLD_MYR, 0),
            ME.tapped(names::GOLD_MYR, 1),
            ME.tapped(names::ISLAND, 1),
            advance_to(1, TurnStep::PostcombatMain),
            THEM.life(18),
        ]),
    Scenario::new("under Propaganda attacking Jace, the Mind Sculptor costs nothing")
        .battlefield(P0, &[GROUND, names::ISLAND, names::ISLAND])
        .battlefield(P1, &[names::PROPAGANDA, props::PLANESWALKER.card])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(GROUND),
            // Two Islands pay {2}, so the player and Jace are both offered.
            ME.deciding(DecisionKind::AttackTarget, 2),
            ME.chooses(DecisionKind::AttackTarget, props::PLANESWALKER.card),
            advance_to(1, TurnStep::DeclareAttackers),
            ME.tapped(names::ISLAND, 0),
            advance_to(1, TurnStep::PostcombatMain),
            THEM.has_loyalty(props::PLANESWALKER.card, 1),
            THEM.life(20),
        ]),
    Scenario::new("under two Propagandas one attacker costs {4}")
        .battlefield(
            P0,
            &[
                GROUND,
                GROUND,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
            ],
        )
        .battlefield(P1, &[names::PROPAGANDA, names::PROPAGANDA])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.deciding(DecisionKind::Attacker, 2),
            // {8} for two is more than four lands make: only Done is left.
            ME.attacks_with(GROUND),
            advance_to(1, TurnStep::DeclareAttackers),
            ME.attacking(GROUND, 1),
            ME.tapped(names::ISLAND, 4),
            advance_to(1, TurnStep::PostcombatMain),
            THEM.life(18),
        ]),
    Scenario::new("under Propaganda each combat's attack is paid for")
        .interaction(&[names::RELENTLESS_ASSAULT, names::SERRA_ANGEL])
        .hand(P0, &[names::RELENTLESS_ASSAULT])
        .battlefield(
            P0,
            &[
                names::SERRA_ANGEL,
                names::MOUNTAIN,
                names::MOUNTAIN,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
            ],
        )
        .battlefield(P1, &[names::PROPAGANDA])
        .actions(&[
            // Relentless Assault's additional combat follows this main phase.
            ME.casts(names::RELENTLESS_ASSAULT),
            Resolve,
            ME.tapped(names::ISLAND, 2),
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(names::SERRA_ANGEL),
            advance_to(1, TurnStep::PostcombatMain),
            THEM.life(16),
            ME.tapped(names::ISLAND, 4),
            // The Angel has vigilance, so it can attack again in the second combat, and pays
            // again.
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(names::SERRA_ANGEL),
            advance_to(1, TurnStep::PostcombatMain),
            THEM.life(12),
            ME.tapped(names::ISLAND, 6),
        ]),
];
