// Generated from the card store by `just card header`: notes go below the end marker.
//! Blood Moon {2}{R}
//! Enchantment
//!
//! Nonbasic lands are Mountains.
// End of the generated card header.

use crate::prelude::*;

/// "Nonbasic lands are Mountains" sets each nonbasic land's land type in layer 4
/// ([CR 305.7]): "If an effect sets a land's subtype to one or more of the basic land types,
/// the land no longer has its old land type. It loses all abilities generated from its rules
/// text, its old land types, and any copiable effects affecting that land, and it gains the
/// appropriate mana ability for each new basic land type." So a nonbasic land taps for {R}
/// and nothing else, and its other abilities are gone, though it keeps its name, its other
/// card types and its supertypes. Dryad Arbor stays a green Dryad creature.
///
/// Urborg, Tomb of Yawgmoth's ability depends on this one, whichever entered first: this
/// effect removes Urborg's ability, so Urborg's waits for it and then does not exist
/// ([CR 613.8a]): "An effect is said to “depend on” another if (a) it's applied in the same
/// layer (and, if applicable, sublayer) as the other effect; (b) applying the other would
/// change the text or the existence of the first effect".
///
/// [CR 305.7]: https://yawgatog.com/resources/magic-rules/#R3057
/// [CR 613.8a]: https://yawgatog.com/resources/magic-rules/#R6138a
pub static CARD: CardDef = Printed::BloodMoon
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::All(&[
                Filter::Types(Types::LAND),
                Filter::Not(&Filter::Supertypes(Supertypes::BASIC)),
            ]),
        },
        condition: None,
        modifications: &[Modification::SetLandTypes(BasicLandTypes::MOUNTAIN)],
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const BLOOD_MOON: &str = Card::BloodMoon;

    // `Prop::Flier` is there for an attack to be the first decision when no spell can be
    // cast.

    pub const SCENARIOS: &[Scenario] = &[
        // Tundra is a Mountain, so it pays the {R} of a second Blood Moon.
        Scenario::new("a nonbasic land taps for {R}")
            .battlefield(P0, &[BLOOD_MOON, Card::Tundra, Card::Island, Card::Island])
            .hand(P0, &[BLOOD_MOON])
            .actions(&[ME.casts(BLOOD_MOON), Resolve, ME.controls(BLOOD_MOON, 2)]),
        // Tundra has lost the mana abilities of its rules text and its old land types, so
        // it can't pay the artifact's {W}: P0 is given no priority in the main phase.
        Scenario::new("a nonbasic land loses its own mana abilities")
            .hand(P0, &[Prop::Artifact])
            .battlefield(P0, &[BLOOD_MOON, Card::Tundra, Prop::Flier])
            .actions(&[ME.deciding(DecisionKind::Attacker, 2)]),
        // A basic land is no nonbasic land: the Plains still pays for the artifact.
        Scenario::new("a basic land keeps its land type")
            .hand(P0, &[Prop::Artifact])
            .battlefield(P0, &[BLOOD_MOON, Card::Plains])
            .actions(&[
                ME.casts(Prop::Artifact),
                Resolve,
                ME.controls(Prop::Artifact, 1),
            ]),
        // Dryad Arbor stays a 1/1 creature, a Dryad, and taps for {R} as a Mountain.
        Scenario::new("Dryad Arbor is a 1/1 Mountain creature that taps for {R}")
            .battlefield(
                P0,
                &[BLOOD_MOON, Card::DryadArbor, Card::Island, Card::Island],
            )
            .hand(P0, &[BLOOD_MOON])
            .actions(&[
                ME.creature(Card::DryadArbor, true),
                ME.pt(Card::DryadArbor, 1, 1),
                ME.casts(BLOOD_MOON),
                Resolve,
                ME.controls(BLOOD_MOON, 2),
            ]),
        // Urborg arrives under Blood Moon as a Mountain with no ability, so the Plains are no
        // Swamps and can't pay Damn's {B}{B}, and Urborg itself taps only for {R}.
        Scenario::new("Urborg entering under it makes no Swamps")
            .interaction(&[Card::UrborgTombOfYawgmoth, Card::Damn])
            .battlefield(P0, &[BLOOD_MOON, Card::Plains, Card::Plains, Prop::Flier])
            .hand(P0, &[Card::UrborgTombOfYawgmoth, Card::Damn])
            .actions(&[
                ME.plays(Card::UrborgTombOfYawgmoth),
                ME.deciding(DecisionKind::Attacker, 2),
            ]),
        // With Urborg first, every land is a Swamp and Damn could be cast. Blood Moon arriving
        // later removes Urborg's ability all the same: the basic lands left untapped are
        // Swamps no longer, and passing is all P0 can do.
        Scenario::new("Blood Moon entering after Urborg ends its Swamps")
            .interaction(&[Card::UrborgTombOfYawgmoth, Card::Damn])
            .battlefield(
                P0,
                &[
                    Card::UrborgTombOfYawgmoth,
                    Card::Mountain,
                    Card::Island,
                    Card::Island,
                    Card::Plains,
                    Card::Plains,
                    Prop::Flier,
                ],
            )
            .hand(P0, &[BLOOD_MOON, Card::Damn])
            .actions(&[
                ME.casts(BLOOD_MOON),
                Resolve,
                ME.controls(BLOOD_MOON, 1),
                ME.deciding(DecisionKind::Priority, 1),
            ]),
        // CR 614.12: an entering replacement looks at the land "as it would exist on the
        // battlefield". Soporific Springs would be a Mountain with no ability of its text, so
        // it asks for no life and enters untapped.
        Scenario::new("a land that asks for life as it enters enters untapped")
            .battlefield(P0, &[BLOOD_MOON])
            .hand(P0, &[Card::SinkIntoStupor])
            .actions(&[
                ME.plays(Card::SinkIntoStupor),
                ME.life(20),
                ME.tapped("Soporific Springs", 0),
            ]),
        // Seachrome Coast would enter tapped with three other lands, but as a Mountain it has
        // no such ability.
        Scenario::new("a land that enters tapped unless few lands enters untapped")
            .interaction(&[Card::SeachromeCoast])
            .battlefield(P0, &[BLOOD_MOON, Card::Plains, Card::Plains, Card::Plains])
            .hand(P0, &[Card::SeachromeCoast])
            .actions(&[
                ME.plays(Card::SeachromeCoast),
                ME.tapped(Card::SeachromeCoast, 0),
            ]),
    ];
}
