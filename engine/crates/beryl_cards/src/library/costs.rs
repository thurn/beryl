//! Costs ([CR 118](https://yawgatog.com/resources/magic-rules/#R118)) paid to cast a spell,
//! activate an ability, or carry out an instruction during resolution.

use beryl_engine::state::CounterKind;
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, stack};

use crate::{names, props};

const THIRST: &str = names::THIRST_FOR_DISCOVERY;

const fn count(zone: Zone, card: Option<&'static str>, count: u8) -> Action {
    Action::assert(Assertion::Count {
        player: P0,
        zone,
        card,
        count,
    })
}

/// A cost that is an action is the player's to pay with the card they choose
/// ([CR 118.12](https://yawgatog.com/resources/magic-rules/#R11812)): "The action [do
/// something] is a cost, paid when the spell or ability resolves." With two basic land cards
/// in hand, a yes is followed by which of them to discard, and the other effect, the discard
/// of two cards, does not happen.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A discard paid as a cost is of the card its player chooses")
            .permanents(P0, &[Permanent::new(names::ISLAND); 3])
            .hand(P0, &[THIRST])
            .library(
                P0,
                &[
                    names::PLAINS,
                    names::ISLAND,
                    props::ARTIFACT.card,
                    names::ISLAND,
                ],
            )
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, THIRST),
                Action::Resolve,
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::ChooseObject,
                    candidates: 2,
                }),
                Action::choose(P0, DecisionKind::ChooseObject, names::ISLAND),
                count(Zone::Graveyard, Some(names::ISLAND), 1),
                count(Zone::Hand, Some(names::PLAINS), 1),
                count(Zone::Hand, None, 2),
            ]),
    ];

/// The discard happens during casting. [CR 601.2h] "The player pays the total cost."
///
/// [CR 601.2h]: https://yawgatog.com/resources/magic-rules/#R6012h
pub const DISCARD_TO_CAST: &[Scenario] =
    &[
        Scenario::new("discarding a card pays for drawing two cards")
            .battlefield(P0, &[names::MOUNTAIN; 2])
            .hand(P0, &[names::THRILL_OF_POSSIBILITY, names::SWAMP])
            .library(P0, &[names::ISLAND; 3])
            .actions(&[
                ME.casts(names::THRILL_OF_POSSIBILITY),
                ME.has(Zone::Graveyard, names::SWAMP, 1),
                ME.has(Zone::Hand, names::SWAMP, 0),
                Resolve,
                ME.has(Zone::Hand, names::ISLAND, 2),
            ]),
    ];

/// The sacrifice happens during casting. [CR 601.2h] "The player pays the total cost."
///
/// [CR 601.2h]: https://yawgatog.com/resources/magic-rules/#R6012h
pub const SACRIFICE_TO_CAST: &[Scenario] =
    &[
        Scenario::new("sacrificing a creature pays for drawing two cards")
            .battlefield(P0, &[names::SWAMP, props::VANILLA.card])
            .hand(P0, &[names::VILLAGE_RITES])
            .library(P0, &[names::ISLAND; 3])
            .actions(&[
                ME.casts(names::VILLAGE_RITES),
                ME.has(Zone::Graveyard, props::VANILLA.card, 1),
                ME.controls(props::VANILLA.card, 0),
                Resolve,
                ME.has(Zone::Hand, names::ISLAND, 2),
            ]),
    ];

/// [CR 702.29a] “Cycling [cost]” means “[Cost], Discard this card: Draw a card.”
///
/// [CR 702.29a]: https://yawgatog.com/resources/magic-rules/#R70229a
pub const CYCLING: &[Scenario] = &[Scenario::new("Cycling Spara's Headquarters")
    .at(1, P0, TurnStep::Upkeep)
    .battlefield(P0, &[names::PLAINS; 3])
    .hand(P0, &[names::SPARAS_HEADQUARTERS])
    .actions(&[
        ME.activates(names::SPARAS_HEADQUARTERS, 4),
        stack(1),
        ME.has_cards(Zone::Hand, 0),
        ME.has(Zone::Graveyard, names::SPARAS_HEADQUARTERS, 1),
        ME.tapped(names::PLAINS, 3),
        Resolve,
        ME.has(Zone::Hand, names::ISLAND, 1),
    ])];

const CHORD: &str = names::CHORD_OF_CALLING;
const BEARS: &str = props::GREEN_CREATURE.card;
const WOLF: &str = props::GREEN_WHITE_CREATURE.card;
const THOPTER: &str = props::COLORLESS_CREATURE.card;

/// Convoke ([CR 702.51a]): "For each colored mana in this spell's total cost, you may tap an
/// untapped creature of that color you control rather than pay that mana. For each generic mana
/// in this spell's total cost, you may tap an untapped creature you control rather than pay that
/// mana." Chord of Calling costs {X}{G}{G}{G}, and its X is chosen before any creature taps
/// ([CR 702.51b]): "The convoke ability isn't an additional or alternative cost and applies only
/// after the total cost of the spell with convoke is determined."
///
/// [CR 702.51a]: https://yawgatog.com/resources/magic-rules/#R70251a
/// [CR 702.51b]: https://yawgatog.com/resources/magic-rules/#R70251b
pub const CONVOKE: &[Scenario] = &[
    // The Bears cast this turn taps for convoke all the same: it is tapped by the spell's
    // cost, not by an ability of its own (see CR 302.6).
    Scenario::new("a creature that entered this turn convokes")
        .battlefield(P0, &[names::FOREST; 4])
        .hand(P0, &[BEARS, CHORD])
        .library(P0, &[THOPTER, names::FOREST])
        .actions(&[
            ME.casts(BEARS),
            Resolve,
            // Two Forests and the Bears pay {G}{G}{G}, so X is 0 and the Bears is the one
            // creature to tap: both are asked of no one.
            ME.casts(CHORD),
            ME.tapped(BEARS, 1),
            ME.tapped(names::FOREST, 4),
            Resolve,
            ME.picks_up_to(THOPTER),
            ME.controls(THOPTER, 1),
        ]),
    // X is 1: the Bears pays a green symbol, the colorless Ornithopter the generic mana, and
    // the two Forests the other two green symbols. The Watchwolf in the library has mana value
    // 2, so the Ornithopter there is the one creature to find.
    Scenario::new("a green creature pays a green symbol and a colorless one generic mana")
        .battlefield(P0, &[names::FOREST, names::FOREST, BEARS, THOPTER])
        .hand(P0, &[CHORD])
        .library(P0, &[WOLF, THOPTER])
        .actions(&[
            ME.casts(CHORD),
            ME.x(1),
            ME.deciding(DecisionKind::ManaPayment, 2),
            ME.chooses(DecisionKind::ManaPayment, BEARS),
            // The Ornithopter is then the one way left to pay the generic mana.
            ME.tapped(names::FOREST, 2),
            ME.tapped(BEARS, 1),
            ME.tapped(THOPTER, 1),
            Resolve,
            ME.deciding(DecisionKind::ChooseUpTo, 2),
            ME.picks_up_to(THOPTER),
            ME.controls(THOPTER, 2),
            ME.has(Zone::Library, WOLF, 1),
        ]),
    // With no mana at all, three green creatures pay the three green symbols, and the fourth
    // stays untapped: there is nothing left for it to pay.
    Scenario::new("creatures cannot pay more than the total cost")
        .battlefield(P0, &[BEARS, BEARS, WOLF, BEARS])
        .hand(P0, &[CHORD])
        .library(P0, &[THOPTER, names::FOREST])
        .actions(&[
            ME.casts(CHORD),
            ME.x(0),
            ME.chooses(DecisionKind::ManaPayment, WOLF),
            // The last two green symbols: two of the Bears, the one kind of creature left.
            ME.tapped(WOLF, 1),
            ME.tapped(BEARS, 2),
            stack(1),
            Resolve,
            ME.picks_up_to(THOPTER),
            ME.controls(THOPTER, 1),
        ]),
    // Six creatures and no land pay {3}{G}{G}{G}: X may be 3 and no more, and Chord finds the
    // creature of mana value 3.
    Scenario::new(
        "six creatures cast Chord of Calling for 3, which finds a creature of mana value 3",
    )
    .battlefield(P0, &[BEARS, BEARS, BEARS, THOPTER, THOPTER, THOPTER])
    .hand(P0, &[CHORD])
    .library(P0, &[props::FLASH_CREATURE.card, names::FOREST])
    .actions(&[
        ME.casts(CHORD),
        ME.deciding(DecisionKind::X, 4),
        ME.x(3),
        ME.chooses(DecisionKind::ManaPayment, BEARS),
        ME.chooses(DecisionKind::ManaPayment, BEARS),
        ME.chooses(DecisionKind::ManaPayment, BEARS),
        // Then only the Ornithopters are left, each the one answer.
        ME.tapped(BEARS, 3),
        ME.tapped(THOPTER, 3),
        Resolve,
        ME.picks_up_to(props::FLASH_CREATURE.card),
        ME.controls(props::FLASH_CREATURE.card, 1),
    ]),
];

const LOXODON: &str = names::VENERATED_LOXODON;

/// Creatures that convoked a spell ([CR 702.51a]): Venerated Loxodon remembers the creatures
/// tapped to pay for its convoke cost across entering the battlefield, puts a +1/+1 counter on
/// each still on the battlefield when its enters trigger resolves, and ignores creatures that
/// were tapped for mana rather than for convoke.
///
/// [CR 702.51a]: https://yawgatog.com/resources/magic-rules/#R70251a
pub const CONVOKED: &[Scenario] = &[
    // Two Plains and three creatures pay {4}{W}: all three creatures convoke, and each gets a
    // +1/+1 counter when Venerated Loxodon's enters trigger resolves.
    Scenario::new("each creature that convoked a spell gets a counter")
        .battlefield(P0, &[names::PLAINS, names::PLAINS, BEARS, WOLF, THOPTER])
        .hand(P0, &[LOXODON])
        .actions(&[
            ME.casts(LOXODON),
            ME.chooses(DecisionKind::ManaPayment, WOLF),
            ME.chooses(DecisionKind::ManaPayment, BEARS),
            ME.tapped(WOLF, 1),
            ME.tapped(BEARS, 1),
            ME.tapped(THOPTER, 1),
            ME.tapped(names::PLAINS, 2),
            Resolve,
            Resolve,
            ME.counters(WOLF, CounterKind::PlusOne, 1),
            ME.counters(BEARS, CounterKind::PlusOne, 1),
            ME.counters(THOPTER, CounterKind::PlusOne, 1),
            ME.counters(LOXODON, CounterKind::PlusOne, 0),
        ]),
    // One of the three convoking creatures leaves the battlefield before the enters trigger
    // resolves; only the two still on the battlefield get counters.
    Scenario::new("a convoking creature that left before the trigger resolves gets no counter")
        .battlefield(P0, &[names::PLAINS, names::PLAINS, BEARS, WOLF, THOPTER])
        .battlefield(P1, &[names::ISLAND; 3])
        .hand(P0, &[LOXODON])
        .hand(P1, &[props::REMOVAL.card])
        .actions(&[
            ME.casts(LOXODON),
            ME.chooses(DecisionKind::ManaPayment, WOLF),
            ME.chooses(DecisionKind::ManaPayment, BEARS),
            Resolve,
            ME.passes(),
            THEM.casts(props::REMOVAL.card),
            THEM.targets(THOPTER),
            ME.controls(THOPTER, 0),
            ME.has(Zone::Hand, THOPTER, 1),
            Resolve,
            ME.counters(WOLF, CounterKind::PlusOne, 1),
            ME.counters(BEARS, CounterKind::PlusOne, 1),
            ME.counters(LOXODON, CounterKind::PlusOne, 0),
        ]),
    // Cast with mana alone: no creature convoked it, so none gets a counter.
    Scenario::new("casting with mana alone puts no counters on creatures")
        .battlefield(
            P0,
            &[
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                BEARS,
            ],
        )
        .hand(P0, &[LOXODON])
        .actions(&[
            ME.casts(LOXODON),
            ME.answers(DecisionKind::ManaPayment, Pick::Done),
            ME.tapped(BEARS, 0),
            ME.tapped(names::PLAINS, 5),
            Resolve,
            Resolve,
            ME.counters(BEARS, CounterKind::PlusOne, 0),
            ME.counters(LOXODON, CounterKind::PlusOne, 0),
        ]),
    // Gold Myr is colorless, so it must tap for {W} mana rather than convoke {4}{W}: it gets
    // no counter even though it was tapped while casting Venerated Loxodon.
    Scenario::new("a creature tapped for mana rather than convoke gets no counter")
        .battlefield(
            P0,
            &[
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                names::GOLD_MYR,
            ],
        )
        .hand(P0, &[LOXODON])
        .actions(&[
            ME.casts(LOXODON),
            ME.tapped(names::GOLD_MYR, 1),
            ME.tapped(names::ISLAND, 4),
            Resolve,
            Resolve,
            ME.counters(names::GOLD_MYR, CounterKind::PlusOne, 0),
            ME.counters(LOXODON, CounterKind::PlusOne, 0),
        ]),
];
