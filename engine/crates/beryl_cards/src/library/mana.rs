//! Mana ([CR 106](https://yawgatog.com/resources/magic-rules/#R106)).

use beryl_engine::card::ManaColor;
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to, stack};

use crate::{names, props};

/// Mana a triggered ability adds is in its controller's pool like any other
/// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect instructs
/// a player to add mana, that mana goes into a player's mana pool." Unspent, it is gone once
/// the phase has ended (CR 500.5). The amount is the one settled as the ability was made:
/// three, the mana value of the sorcery countered.
pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("Mana a triggered ability adds is gone once its phase has ended")
        .at(2, P1, TurnStep::PrecombatMain)
        .permanents(P0, &[Permanent::new(names::ISLAND); 2])
        .hand(P0, &[names::MANA_DRAIN])
        .library(P0, &[names::PLAINS, names::PLAINS])
        .permanents(P1, &[Permanent::new(names::ISLAND); 3])
        .hand(P1, &[props::UNTARGETED_SPELL.card])
        .actions(&[
            Action::cast(P1, props::UNTARGETED_SPELL.card),
            Action::cast(P0, names::MANA_DRAIN),
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::PrecombatMain,
            },
            Action::pass(P0),
            Action::assert(Assertion::Mana {
                player: P0,
                color: ManaColor::Colorless,
                amount: 3,
            }),
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::BeginningOfCombat,
            },
            Action::assert(Assertion::Mana {
                player: P0,
                color: ManaColor::Colorless,
                amount: 0,
            }),
        ]),
    // The delayed ability leaves the mana pool empty in the opponent's postcombat main phase.
    Scenario::new("Mana Drain: the countered spell's mana value at the caster's next main phase")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[names::ISLAND, names::ISLAND])
        .hand(P0, &[names::MANA_DRAIN])
        .library(P0, &[names::PLAINS])
        .battlefield(P1, &[names::ISLAND; 3])
        .hand(P1, &[names::TIMETWISTER])
        .actions(&[
            THEM.casts(names::TIMETWISTER),
            ME.casts(names::MANA_DRAIN),
            THEM.has(Zone::Graveyard, names::TIMETWISTER, 1),
            advance_to(2, TurnStep::PostcombatMain),
            ME.mana(ManaColor::Colorless, 0),
            advance_to(3, TurnStep::PrecombatMain),
            ME.passes(),
            ME.mana(ManaColor::Colorless, 3),
        ]),
];

pub const EQUIVALENT_SOURCES: &[Scenario] =
    &[
        Scenario::new("Equivalent lands pay without a payment choice")
            .hand(P0, &[props::VANILLA.card])
            .battlefield(P0, &[names::ISLAND; 3])
            .actions(&[
                ME.casts(props::VANILLA.card),
                stack(1),
                ME.tapped(names::ISLAND, 2),
                Resolve,
                ME.controls(props::VANILLA.card, 1),
            ]),
    ];

pub const FLOATING_MANA: &[Scenario] =
    &[
        Scenario::new("A payment that floats mana is a distinct choice")
            .hand(P0, &[props::VANILLA.card])
            .battlefield(P0, &[names::ISLAND, names::ISLAND, names::MANA_CRYPT])
            .actions(&[
                ME.casts(props::VANILLA.card),
                ME.deciding(DecisionKind::PaymentPlan, 2),
                // One Island pays {U}, and Mana Crypt pays {1} with {C} left over.
                ME.answers(DecisionKind::PaymentPlan, Pick::Plan { i: 1 }),
                stack(1),
                ME.tapped(names::MANA_CRYPT, 1),
                ME.tapped(names::ISLAND, 1),
                ME.mana(ManaColor::Colorless, 1),
                Resolve,
                ME.controls(props::VANILLA.card, 1),
            ]),
    ];

/// Priced mana abilities (CR 605.3a): activating a mana ability whose cost includes mana
/// requires paying that mana from another source or the pool first, and sequences of priced
/// sources must spend the right colors to leave the target cost payable.
pub const PRICED_MANA: &[Scenario] = &[
    Scenario::new("A priced mana source alone cannot pay for a one-mana spell")
        .battlefield(P0, &[names::AZORIUS_SIGNET])
        .hand(P0, &[props::EQUIPMENT.card, names::ISLAND])
        .actions(&[
            // Only Pass and playing the Island are legal: the Signet cannot pay its own {1}.
            ME.deciding(DecisionKind::Priority, 2),
            ME.has(Zone::Hand, props::EQUIPMENT.card, 1),
            ME.controls(props::EQUIPMENT.card, 0),
        ]),
    Scenario::new("A Signet and an Island pay {W}{U}")
        .battlefield(P0, &[names::AZORIUS_SIGNET, names::ISLAND])
        .hand(P0, &[props::ARTIFACT.card, props::EQUIPMENT.card])
        .actions(&[
            // {W} from the Signet casts Portable Hole and leaves {U} floating, which pays {1}.
            ME.casts(props::ARTIFACT.card),
            ME.mana(ManaColor::Blue, 1),
            Resolve,
            ME.controls(props::ARTIFACT.card, 1),
            ME.casts(props::EQUIPMENT.card),
            Resolve,
            ME.controls(props::EQUIPMENT.card, 1),
            ME.tapped(names::AZORIUS_SIGNET, 1),
            ME.tapped(names::ISLAND, 1),
            ME.mana(ManaColor::Blue, 0),
        ]),
    Scenario::new("Two Signets and one land pay {1}{W}{W} only by spending {U} into the second")
        .battlefield(
            P0,
            &[names::AZORIUS_SIGNET, names::AZORIUS_SIGNET, names::ISLAND],
        )
        .hand(P0, &[props::FLIER.card])
        .actions(&[
            ME.casts(props::FLIER.card),
            Resolve,
            ME.controls(props::FLIER.card, 1),
            ME.tapped(names::AZORIUS_SIGNET, 2),
            ME.tapped(names::ISLAND, 1),
        ]),
    Scenario::new("Mana left floating by a priced payment stays in the pool")
        .battlefield(P0, &[names::AZORIUS_SIGNET, names::ISLAND])
        .hand(P0, &[props::ARTIFACT.card])
        .actions(&[
            ME.casts(props::ARTIFACT.card),
            ME.mana(ManaColor::Blue, 1),
            ME.tapped(names::AZORIUS_SIGNET, 1),
            ME.tapped(names::ISLAND, 1),
            Resolve,
            ME.controls(props::ARTIFACT.card, 1),
        ]),
];

/// Triggered mana abilities from a resolving spell (CR 605.1b, CR 106.12): High Tide adds
/// an additional {U} whenever any player taps an Island for mana until end of turn.
pub const HIGH_TIDE: &[Scenario] = &[
    Scenario::new("High Tide makes an Island produce {U}{U}")
        .battlefield(P0, &[names::ISLAND; 2])
        .hand(P0, &[names::HIGH_TIDE, props::VANILLA.card])
        .actions(&[
            ME.casts(names::HIGH_TIDE),
            Resolve,
            ME.casts(props::VANILLA.card),
            Resolve,
            ME.controls(props::VANILLA.card, 1),
            ME.tapped(names::ISLAND, 2),
            ME.mana(ManaColor::Blue, 0),
        ]),
    Scenario::new("Two High Tides make an Island produce three {U}")
        .battlefield(P0, &[names::ISLAND; 3])
        .hand(
            P0,
            &[
                names::HIGH_TIDE,
                names::HIGH_TIDE,
                props::VANILLA.card,
                props::EQUIPMENT.card,
            ],
        )
        .actions(&[
            ME.casts(names::HIGH_TIDE),
            Resolve,
            ME.casts(names::HIGH_TIDE),
            Resolve,
            // Empty the floating {U} from the second High Tide before testing one Island.
            advance_to(1, TurnStep::PostcombatMain),
            ME.mana(ManaColor::Blue, 0),
            ME.casts(props::VANILLA.card),
            ME.mana(ManaColor::Blue, 1),
            Resolve,
            ME.controls(props::VANILLA.card, 1),
            ME.casts(props::EQUIPMENT.card),
            ME.mana(ManaColor::Blue, 0),
            Resolve,
            ME.controls(props::EQUIPMENT.card, 1),
            ME.tapped(names::ISLAND, 3),
        ]),
    Scenario::new("Tundra tapped for {W} under High Tide adds an additional {U}")
        .battlefield(P0, &[names::ISLAND, props::NONBASIC_LAND.card])
        .hand(P0, &[names::HIGH_TIDE, props::ARTIFACT.card])
        .actions(&[
            ME.casts(names::HIGH_TIDE),
            Resolve,
            ME.casts(props::ARTIFACT.card),
            ME.mana(ManaColor::Blue, 1),
            ME.tapped(props::NONBASIC_LAND.card, 1),
            Resolve,
            ME.controls(props::ARTIFACT.card, 1),
        ]),
    Scenario::new("High Tide adds {U} when the opponent taps an Island for mana")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[names::ISLAND])
        .hand(P0, &[names::HIGH_TIDE])
        .battlefield(P1, &[names::ISLAND])
        .hand(P1, &[props::VANILLA.card])
        .actions(&[
            ME.casts(names::HIGH_TIDE),
            THEM.casts(props::VANILLA.card),
            THEM.controls(props::VANILLA.card, 1),
            THEM.tapped(names::ISLAND, 1),
        ]),
    Scenario::new("High Tide ends at cleanup")
        .battlefield(P0, &[names::ISLAND])
        .hand(P0, &[names::HIGH_TIDE, props::VANILLA.card])
        .library(P0, &[props::EQUIPMENT.card; 2])
        .library(P1, &[props::EQUIPMENT.card; 2])
        .actions(&[
            ME.casts(names::HIGH_TIDE),
            Resolve,
            advance_to(3, TurnStep::PrecombatMain),
            // Only Pass and casting the drawn Equipment are legal; Vanilla ({1}{U}) cannot be paid.
            ME.deciding(DecisionKind::Priority, 2),
            ME.controls(props::VANILLA.card, 0),
        ]),
    Scenario::new("A spell paid with one Island under High Tide leaves {U} floating")
        .battlefield(P0, &[names::ISLAND; 2])
        .hand(P0, &[names::HIGH_TIDE, props::EQUIPMENT.card])
        .actions(&[
            ME.casts(names::HIGH_TIDE),
            Resolve,
            ME.casts(props::EQUIPMENT.card),
            ME.mana(ManaColor::Blue, 1),
            Resolve,
            ME.controls(props::EQUIPMENT.card, 1),
        ]),
    Scenario::new("Under High Tide, paying {2}{B} taps one Island instead of two Plains")
        .battlefield(
            P0,
            &[
                names::ISLAND,
                names::ISLAND,
                names::PLAINS,
                names::PLAINS,
                names::SWAMP,
            ],
        )
        .hand(P0, &[names::HIGH_TIDE, props::FLASH_CREATURE.card])
        .actions(&[
            ME.casts(names::HIGH_TIDE),
            Resolve,
            ME.casts(props::FLASH_CREATURE.card),
            ME.tapped(names::ISLAND, 2),
            ME.tapped(names::SWAMP, 1),
            ME.tapped(names::PLAINS, 0),
            Resolve,
            ME.controls(props::FLASH_CREATURE.card, 1),
        ]),
];

/// Permanent triggered mana abilities (CR 605.1b, CR 106.12): Mana Flare adds one mana of any
/// type a land produced whenever any player taps a land for mana.
pub const MANA_FLARE: &[Scenario] = &[
    Scenario::new("Mana Flare makes an Island produce {U}{U} for its controller")
        .battlefield(P0, &[names::MANA_FLARE, names::ISLAND])
        .hand(P0, &[props::VANILLA.card])
        .actions(&[
            ME.casts(props::VANILLA.card),
            Resolve,
            ME.controls(props::VANILLA.card, 1),
            ME.tapped(names::ISLAND, 1),
            ME.mana(ManaColor::Blue, 0),
        ]),
    Scenario::new("Mana Flare makes an Island produce {U}{U} for the opponent")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[names::MANA_FLARE])
        .battlefield(P1, &[names::ISLAND])
        .hand(P1, &[props::VANILLA.card])
        .actions(&[
            THEM.casts(props::VANILLA.card),
            THEM.controls(props::VANILLA.card, 1),
            THEM.tapped(names::ISLAND, 1),
        ]),
    Scenario::new("Under Mana Flare, a land tapped for two types adds one of the chosen type")
        .interaction(&[names::SKYCLOUD_EXPANSE])
        .battlefield(
            P0,
            &[names::MANA_FLARE, names::SKYCLOUD_EXPANSE, names::ISLAND],
        )
        .hand(P0, &[props::FLIER.card])
        .actions(&[
            // Island makes {U}{U} under Mana Flare; {U} activates Skycloud Expanse for {W}{U}
            // plus Mana Flare's chosen {W}, paying {1}{W}{W} and leaving {U} floating.
            ME.casts(props::FLIER.card),
            Resolve,
            ME.controls(props::FLIER.card, 1),
            ME.tapped(names::SKYCLOUD_EXPANSE, 1),
            ME.tapped(names::ISLAND, 1),
            ME.mana(ManaColor::White, 0),
            ME.mana(ManaColor::Blue, 1),
        ]),
];

/// Aura triggered mana abilities (CR 605.1b, CR 106.12): Wild Growth adds an additional {G}
/// to the enchanted land's controller whenever that land is tapped for mana.
pub const WILD_GROWTH: &[Scenario] = &[
    Scenario::new("Wild Growth's enchanted land makes its own mana and {G}")
        .battlefield(P0, &[names::FOREST, names::ISLAND])
        .hand(P0, &[names::WILD_GROWTH, props::VANILLA.card])
        .actions(&[
            ME.casts(names::WILD_GROWTH),
            ME.targets(names::ISLAND),
            Resolve,
            // Forest is tapped for Wild Growth; the enchanted Island alone pays {1}{U}.
            ME.casts(props::VANILLA.card),
            Resolve,
            ME.controls(props::VANILLA.card, 1),
            ME.tapped(names::ISLAND, 1),
            ME.mana(ManaColor::Blue, 0),
            ME.mana(ManaColor::Green, 0),
        ]),
    Scenario::new("Wild Growth on an opponent's land gives the {G} to that land's controller")
        .battlefield(P0, &[names::FOREST])
        .hand(P0, &[names::WILD_GROWTH])
        .library(P0, &[names::PLAINS; 2])
        .battlefield(P1, &[names::ISLAND])
        .hand(P1, &[props::VANILLA.card])
        .library(P1, &[names::PLAINS; 2])
        .actions(&[
            ME.casts(names::WILD_GROWTH),
            ME.targets(names::ISLAND),
            Resolve,
            advance_to(2, TurnStep::PrecombatMain),
            THEM.casts(props::VANILLA.card),
            THEM.controls(props::VANILLA.card, 1),
            THEM.tapped(names::ISLAND, 1),
        ]),
];

/// Mana-multiplying replacement effects (CR 106.12b): Mana Reflection multiplies the tapped
/// ability's own product without multiplying triggered mana abilities (`ManaChange::Adds`),
/// and multiple copies multiply together.
pub const MANA_REFLECTION: &[Scenario] = &[
    Scenario::new("Mana Reflection doubles an Island")
        .battlefield(P0, &[names::MANA_REFLECTION, names::ISLAND])
        .hand(P0, &[props::VANILLA.card])
        .actions(&[
            ME.casts(props::VANILLA.card),
            Resolve,
            ME.controls(props::VANILLA.card, 1),
            ME.tapped(names::ISLAND, 1),
            ME.mana(ManaColor::Blue, 0),
        ]),
    Scenario::new("Mana Reflection doubles a Mana Crypt")
        .interaction(&[names::MANA_CRYPT])
        .battlefield(P0, &[names::MANA_REFLECTION, names::MANA_CRYPT])
        .hand(P0, &[props::EQUIPMENT.card; 4])
        .actions(&[
            ME.casts(props::EQUIPMENT.card),
            Resolve,
            ME.casts(props::EQUIPMENT.card),
            Resolve,
            ME.casts(props::EQUIPMENT.card),
            Resolve,
            ME.casts(props::EQUIPMENT.card),
            Resolve,
            ME.controls(props::EQUIPMENT.card, 4),
            ME.tapped(names::MANA_CRYPT, 1),
            ME.mana(ManaColor::Colorless, 0),
        ]),
    Scenario::new("Two Mana Reflections quadruple an Island")
        .battlefield(
            P0,
            &[
                names::MANA_REFLECTION,
                names::MANA_REFLECTION,
                names::ISLAND,
            ],
        )
        .hand(P0, &[props::PLANESWALKER.card])
        .actions(&[
            ME.casts(props::PLANESWALKER.card),
            Resolve,
            ME.controls(props::PLANESWALKER.card, 1),
            ME.tapped(names::ISLAND, 1),
            ME.mana(ManaColor::Blue, 0),
        ]),
    Scenario::new("Mana Reflection with High Tide makes three blue from an Island, not four")
        .interaction(&[names::HIGH_TIDE])
        .battlefield(P0, &[names::MANA_REFLECTION, names::ISLAND, names::ISLAND])
        .hand(
            P0,
            &[names::HIGH_TIDE, props::EQUIPMENT.card, props::VANILLA.card],
        )
        .actions(&[
            // First Island makes {U}{U}: {U} casts High Tide and {U} casts the Equipment.
            ME.casts(names::HIGH_TIDE),
            Resolve,
            ME.casts(props::EQUIPMENT.card),
            Resolve,
            // Second Island makes {U}{U} (doubled) + {U} (High Tide) = {U}{U}{U}, leaving 1 blue
            // after {1}{U} (not 2, as four blue would).
            ME.casts(props::VANILLA.card),
            Resolve,
            ME.controls(props::VANILLA.card, 1),
            ME.mana(ManaColor::Blue, 1),
        ]),
];

/// Reading what a source could produce (CR 106.7): Reflecting Pool makes any type a land its
/// controller controls could produce, ignoring costs and tapped state, with a least fixed point
/// so two Reflecting Pools alone make nothing and may still be tapped.
pub const REFLECTING_POOL: &[Scenario] = &[
    Scenario::new("Reflecting Pool with a Tundra makes white or blue")
        .permanents(
            P0,
            &[
                Permanent::new(names::TUNDRA).tapped(),
                Permanent::new(names::REFLECTING_POOL),
                Permanent::new(names::REFLECTING_POOL),
                Permanent::new(names::REFLECTING_POOL),
            ],
        )
        .hand(P0, &[props::ARTIFACT.card, props::VANILLA.card])
        .actions(&[
            ME.casts(props::ARTIFACT.card),
            Resolve,
            ME.casts(props::VANILLA.card),
            Resolve,
            ME.controls(props::ARTIFACT.card, 1),
            ME.controls(props::VANILLA.card, 1),
            ME.tapped(names::REFLECTING_POOL, 3),
        ]),
    Scenario::new("Two Reflecting Pools alone make nothing and may still be tapped")
        .battlefield(P0, &[names::REFLECTING_POOL, names::REFLECTING_POOL])
        .actions(&[
            ME.activates(names::REFLECTING_POOL, 0),
            ME.tapped(names::REFLECTING_POOL, 1),
            ME.mana(ManaColor::White, 0),
            ME.mana(ManaColor::Blue, 0),
            ME.mana(ManaColor::Colorless, 0),
        ]),
    Scenario::new("Reflecting Pool reads a land's ability whose cost cannot be paid")
        .interaction(&[names::SKYCLOUD_EXPANSE])
        .battlefield(P0, &[names::REFLECTING_POOL, names::SKYCLOUD_EXPANSE])
        .hand(P0, &[props::ARTIFACT.card])
        .actions(&[
            ME.casts(props::ARTIFACT.card),
            Resolve,
            ME.controls(props::ARTIFACT.card, 1),
            ME.tapped(names::REFLECTING_POOL, 1),
            ME.tapped(names::SKYCLOUD_EXPANSE, 0),
        ]),
];

/// Granted mana abilities (CR 613.1f, CR 302.6, CR 113.2c): Gemhide Sliver grants all Slivers
/// "{T}: Add one mana of any color."
pub const GRANTED_MANA: &[Scenario] = &[
    Scenario::new("A Sliver that entered this turn cannot tap for granted mana")
        .battlefield(P0, &[names::FOREST, names::ISLAND])
        .hand(P0, &[names::GEMHIDE_SLIVER, props::EQUIPMENT.card])
        .actions(&[
            ME.casts(names::GEMHIDE_SLIVER),
            Resolve,
            ME.controls(names::GEMHIDE_SLIVER, 1),
            // Both lands are tapped and Gemhide Sliver entered this turn (CR 302.6), so only Pass
            // is legal.
            ME.deciding(DecisionKind::Priority, 1),
        ]),
    Scenario::new("An opponent's Sliver has the granted mana ability and its controller uses it")
        .interaction(&[names::METALLIC_SLIVER])
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[names::GEMHIDE_SLIVER])
        .battlefield(P1, &[names::METALLIC_SLIVER])
        .hand(P1, &[props::EQUIPMENT.card])
        .actions(&[
            THEM.casts(props::EQUIPMENT.card),
            THEM.tapped(names::METALLIC_SLIVER, 1),
            THEM.controls(props::EQUIPMENT.card, 1),
        ]),
    Scenario::new("Two Gemhide Slivers offer one mana option for a Sliver")
        .battlefield(P0, &[names::GEMHIDE_SLIVER, names::GEMHIDE_SLIVER])
        .hand(P0, &[props::EQUIPMENT.card])
        .actions(&[
            // Both Gemhide Slivers grant the same ability; casting {1} taps one without a payment
            // choice between duplicate granted instances.
            ME.casts(props::EQUIPMENT.card),
            Resolve,
            ME.tapped(names::GEMHIDE_SLIVER, 1),
            ME.controls(props::EQUIPMENT.card, 1),
        ]),
    Scenario::new("Granted mana from Gemhide Sliver pays for a spell")
        .battlefield(P0, &[names::GEMHIDE_SLIVER, names::FOREST])
        .hand(P0, &[props::GREEN_WHITE_CREATURE.card])
        .actions(&[
            // Forest pays {G} and Gemhide Sliver taps for {W} to cast a {G}{W} creature.
            ME.casts(props::GREEN_WHITE_CREATURE.card),
            Resolve,
            ME.tapped(names::GEMHIDE_SLIVER, 1),
            ME.tapped(names::FOREST, 1),
            ME.controls(props::GREEN_WHITE_CREATURE.card, 1),
        ]),
];
