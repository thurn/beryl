//! Which costs the engine has code to pay, so that a card table refuses a definition with any
//! other ([`crate::table::TableError::Cost`]). The rules' casting steps then pay every part
//! of every cost in a game, and none is skipped
//! (see [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h)): "The player pays the
//! total cost. ... Partial payments are not allowed. Unpayable costs can't be paid."
//!
//! The vocabulary says more than this ([`CostPart`]): a part the engine does not pay is a
//! rule not yet implemented, listed in `docs/rules-coverage.md`.
//!
//! Implements: none. Checks whether the engine supports the costs in a card definition.

use beryl_card::{Ability, AbilityKind, CardDef, Chosen, CostAction, CostPart, Measure, Zone};

use crate::table::{is_sacrificed, is_tap_for_mana};

/// Why the engine cannot play a card for one of its costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unpayable {
    /// A part of a cost that no code pays.
    Part(CostPart),
    /// A mana ability whose cost is neither {T} nor {T} and sacrificing its source: the
    /// mana solver activates no other.
    ManaAbility,
    /// An effect or an intervening "if" names what a cost took
    /// ([`beryl_card::Objects::Paid`]), which the engine does not keep.
    Named,
}

/// Whether the engine pays this part of an activation cost: something done to the ability's
/// source, a loyalty cost, a payment of life, or tapping permanents measured by their total power or by the
/// generic mana they pay.
pub fn pays_for_ability(part: &CostPart) -> bool {
    match *part {
        CostPart::This(
            CostAction::Tap
            | CostAction::Sacrifice
            | CostAction::Discard
            | CostAction::RemoveCounter(_),
        )
        | CostPart::Loyalty(_)
        | CostPart::Life(_) => true,
        CostPart::Chosen(CostAction::Tap, chosen) => matches!(
            chosen,
            Chosen {
                zone: Zone::Battlefield,
                measure: Measure::TotalPower(_) | Measure::ForGeneric(_),
                ..
            }
        ),
        CostPart::This(_) | CostPart::Chosen(..) | CostPart::OneOf(_) | CostPart::DiscardHand => {
            false
        }
    }
}

/// Whether the engine pays this part of a spell's cost: sacrificing permanents, discarding
/// or exiling cards, or paying life.
pub fn pays_for_spell(part: &CostPart) -> bool {
    matches!(
        *part,
        CostPart::Chosen(
            CostAction::Exile,
            Chosen {
                zone: Zone::Hand | Zone::Graveyard,
                measure: Measure::Count(_),
                ..
            }
        ) | CostPart::Chosen(
            CostAction::Sacrifice,
            Chosen {
                zone: Zone::Battlefield,
                measure: Measure::Count(_),
                ..
            }
        ) | CostPart::Chosen(
            CostAction::Discard,
            Chosen {
                zone: Zone::Hand,
                measure: Measure::Count(_),
                ..
            }
        ) | CostPart::Life(_)
    )
}

/// Whether the engine pays the cost of this ability. A mana ability is paid by the mana
/// solver, which taps its source or taps and sacrifices it, and pays nothing else
/// ([`is_tap_for_mana`], restricted or not, and [`is_sacrificed`]), or is activated only at
/// priority ([`is_instant_mana`]).
fn ability(a: &Ability) -> Result<(), Unpayable> {
    if is_instant_mana(a) {
        return Ok(());
    }
    if a.kind() == AbilityKind::Mana && !is_tap_for_mana(a) && !is_sacrificed(a) {
        return Err(Unpayable::ManaAbility);
    }
    if let Some(part) = a.cost().parts().iter().find(|part| !pays_for_ability(part)) {
        return Err(Unpayable::Part(*part));
    }
    // At most one part is paid rather than generic mana: the casting steps keep one count.
    let substitutes = |part: &&CostPart| matches!(part, CostPart::Chosen(_, chosen) if matches!(chosen.measure, Measure::ForGeneric(_)));
    if let Some(part) = a.cost().parts().iter().filter(substitutes).nth(1) {
        return Err(Unpayable::Part(*part));
    }
    if a.names_paid() {
        return Err(Unpayable::Named);
    }
    Ok(())
}

/// The first cost of a card's faces the engine cannot pay, among the abilities each face has
/// and grants and the costs it is cast with.
pub(crate) fn check(def: &CardDef) -> Result<(), Unpayable> {
    for face in def.faces {
        for a in face.abilities.iter().chain(face.grants) {
            ability(a)?;
        }
        let options = face.casting.iter().flat_map(|option| option.parts());
        if let Some(part) = face
            .additional_cost
            .iter()
            .chain(options)
            .find(|part| !pays_for_spell(part))
        {
            return Err(Unpayable::Part(*part));
        }
    }
    Ok(())
}

/// Whether an ability is a mana ability activated only while its controller has priority
/// ([`Timing::AsInstant`](beryl_card::Timing::AsInstant)), with no mana in its cost and parts
/// the engine pays as it activates it: tapping or sacrificing its source and discarding the
/// hand, as "Discard your hand, Sacrifice this artifact: Add three mana of any one color.
/// Activate only as an instant." It makes one kind of mana, or any one color, which its
/// controller chooses as they activate it. The mana solver never activates one: priority
/// offers it as an activation of its own.
#[inline]
pub fn is_instant_mana(a: &Ability) -> bool {
    let kinds = a.produces().colors;
    a.kind() == AbilityKind::Mana
        && a.timing() == beryl_card::Timing::AsInstant
        && (kinds.is_power_of_two() || kinds == ANY_COLOR)
        && a.cost().mana.is_zero()
        && a.cost().parts().iter().all(|part| {
            matches!(
                part,
                CostPart::This(CostAction::Tap | CostAction::Sacrifice) | CostPart::DiscardHand
            )
        })
}

/// The five colors as a [`beryl_card::ManaSet`]'s bits: "any color" (no colorless).
pub const ANY_COLOR: u8 = 0b1_1111;

#[cfg(test)]
mod tests {
    use beryl_card::{
        Amount, Cost, CounterKind, Effect, Face, Filter, ManaColor, ManaCost, ManaSet, Objects,
        Players, Types,
    };

    use super::*;
    use crate::format::Format;
    use crate::table::{CardTable, GameTable, TableError};

    const CREATURE: Filter = Filter::Types(Types::CREATURE);

    const fn activated(cost: Cost) -> Ability {
        Ability::activated(cost)
    }

    static FILLER: CardDef = CardDef {
        name: "Filler",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            abilities: &[],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    /// Every part of an activation cost the engine pays, and each cost a spell is cast with.
    static PAID: CardDef = CardDef {
        name: "Paid",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            abilities: &[
                activated(Cost::of(
                    ManaCost::generic(2),
                    &[
                        CostPart::TAP,
                        CostPart::SACRIFICE,
                        CostPart::remove(CounterKind::Story),
                        CostPart::waterbend(2),
                    ],
                )),
                activated(Cost::loyalty(-2)),
                Ability::cycling(ManaCost::generic(1)),
                Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
            ],
            grants: &[Ability::crew(3)],
            additional_cost: &[CostPart::exile(2, Zone::Graveyard, Filter::Any)],
            casting: &[beryl_card::CastingOption::Evoke {
                cost: ManaCost::NONE,
                parts: &[CostPart::exile(1, Zone::Hand, CREATURE)],
            }],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    const SACRIFICE_A_CREATURE: CostPart = CostPart::sacrifice(CREATURE);
    static ALTAR: CardDef = CardDef {
        name: "Altar",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            abilities: &[activated(Cost::of_parts(&[SACRIFICE_A_CREATURE]))],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static MAKER: CardDef = CardDef {
        name: "Maker",
        shape: beryl_card::definition::Shape::Single,
        faces: &[],
        tokens: &[&ALTAR],
    };
    static RITE: CardDef = CardDef {
        name: "Rite",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            additional_cost: &[SACRIFICE_A_CREATURE],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static BLOODLETTER: CardDef = CardDef {
        name: "Bloodletter",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            abilities: &[activated(Cost::of_parts(&[CostPart::This(
                CostAction::Reveal,
            )]))],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static FORAGER: CardDef = CardDef {
        name: "Forager",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            abilities: &[activated(Cost::of_parts(&[CostPart::OneOf(&[
                Cost::TAP,
                Cost::mana(ManaCost::generic(3)),
            ])]))],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static TWICE_BENT: CardDef = CardDef {
        name: "Twice Bent",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            abilities: &[activated(Cost::of(
                ManaCost::generic(2),
                &[CostPart::waterbend(1), CostPart::waterbend(1)],
            ))],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static FILTER_LAND: CardDef = CardDef {
        name: "Filter Land",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            abilities: &[Ability::mana(
                Cost::of(ManaCost::generic(1), &[CostPart::TAP]),
                ManaSet::one(ManaColor::Blue),
            )],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    /// Its effect reads the creature its crew cost tapped, which the engine does not keep.
    static REMEMBERER: CardDef = CardDef {
        name: "Rememberer",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            abilities: &[Ability::crew(1).effect(&Effect::GainLife {
                player: Players::You,
                amount: Amount::Power(Objects::Paid(CostAction::Tap)),
            })],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static CARDS: CardTable = CardTable::new(&[
        &FILLER,
        &PAID,
        &ALTAR,
        &MAKER,
        &RITE,
        &BLOODLETTER,
        &FORAGER,
        &TWICE_BENT,
        &FILTER_LAND,
        &REMEMBERER,
    ]);

    fn table(card: &'static str) -> Result<GameTable, TableError> {
        GameTable::new(
            Format::CONSTRUCTED,
            &CARDS,
            [&[(4, "Filler")], &[(1, card)]],
        )
    }

    #[test]
    fn a_table_holds_every_cost_the_engine_pays() {
        assert!(table("Paid").is_ok());
        assert!(table("Rite").is_ok());
    }

    /// A part no code pays would be skipped as its spell is cast or its ability activated:
    /// the table is not built, whether the card is in a deck or is a token a deck's card
    /// makes, and whether the part is an ability's or a spell's.
    #[test]
    fn a_table_refuses_a_cost_the_engine_cannot_pay() {
        let refused = |card, unpayable| Some(TableError::Cost { card, unpayable });
        let sacrifice = Unpayable::Part(SACRIFICE_A_CREATURE);
        assert_eq!(table("Altar").err(), refused("Altar", sacrifice));
        assert_eq!(table("Maker").err(), refused("Altar", sacrifice));
        assert_eq!(
            table("Bloodletter").err(),
            refused(
                "Bloodletter",
                Unpayable::Part(CostPart::This(CostAction::Reveal))
            )
        );
        assert!(matches!(
            table("Forager").err(),
            Some(TableError::Cost {
                card: "Forager",
                unpayable: Unpayable::Part(CostPart::OneOf(_)),
            })
        ));
        assert_eq!(
            table("Twice Bent").err(),
            refused("Twice Bent", Unpayable::Part(CostPart::waterbend(1)))
        );
        assert_eq!(
            table("Filter Land").err(),
            refused("Filter Land", Unpayable::ManaAbility)
        );
    }

    /// A part an ability pays is not one a spell pays, and the other way about: each is paid
    /// in its own place among the casting steps.
    #[test]
    fn a_spell_and_an_ability_each_pay_their_own_parts() {
        let exile = CostPart::exile(1, Zone::Hand, Filter::Any);
        assert!(pays_for_spell(&exile) && !pays_for_ability(&exile));
        for part in [CostPart::TAP, CostPart::crew(2), CostPart::Loyalty(1)] {
            assert!(pays_for_ability(&part) && !pays_for_spell(&part));
        }
        // Life is paid in either place.
        assert!(pays_for_ability(&CostPart::Life(1)) && pays_for_spell(&CostPart::Life(1)));
        // Only its source is exiled or revealed by no code, and only cards are exiled.
        assert!(!pays_for_ability(&CostPart::This(CostAction::Exile)));
        assert!(!pays_for_spell(&CostPart::exile(
            1,
            Zone::Battlefield,
            Filter::Any
        )));
    }

    #[test]
    fn a_table_refuses_an_effect_that_names_what_a_cost_took() {
        let refused = TableError::Cost {
            card: "Rememberer",
            unpayable: Unpayable::Named,
        };
        assert_eq!(table("Rememberer").err(), Some(refused));
    }
}
