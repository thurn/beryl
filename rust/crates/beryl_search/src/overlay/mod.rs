//! The per-deck overlay: hand-tuned card knowledge for the supported decks, applied on top of
//! what the rollout policy derives from every card's definition. Hand-tuned numbers
//! survive only here, as an optional overlay for the supported decks.
//!
//! It was tuned for the WU Control mirror (`decks::WU_CONTROL`) and the Vintage WU Control
//! deck before it (`decks::VINTAGE_WU_CONTROL`). A card with no entry keeps what
//! [`crate::CardValues`] derives, and a game of cards none of which have an entry plays
//! exactly as well as derivation allows. Only [`crate::Knowledge::Tuned`] reads it.
//!
//! This is the one place outside the expert system where `beryl_search` names cards (the
//! card-names check exempts this directory). It holds judgement, not facts a definition
//! already states: a value, a card never worth casting, an ability never worth activating,
//! and the abilities the decks were tuned as though they did not draw.

use beryl_cards::names;
use beryl_engine::card::{Ability, AbilityKind, Behavior, CardDef, CostAction, Effect};

use crate::values::CardInfo;

/// The most abilities one card's tuning names.
const MOST_TUNED_ABILITIES: usize = 2;

/// One card's hand-tuned knowledge.
#[derive(Clone, Copy, Debug)]
struct Tuning {
    name: &'static str,
    /// Roughly how much a strong player would rather have the card than not, on a scale where
    /// a game-winning planeswalker is about 90 and a basic land 10.
    value: Option<i16>,
    never_cast: bool,
    /// The abilities of the front face it tunes, each with what it expects to find there.
    abilities: [Option<(Tune, Expect)>; MOST_TUNED_ABILITIES],
}

/// What a tuning does to one ability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tune {
    /// Never activate it.
    Never,
    /// It draws its controller cards, but the decks were tuned with the policy treating it as
    /// one that does not, so it is still played while the library runs low. Each entry's
    /// comment says why.
    AsNotDrawing,
}

/// An ability of the front face, by its position, and what that ability is expected to be.
/// A card file that inserts, removes or reorders abilities moves the position; the shape
/// is what notices. [`apply`] tunes the ability only while it still has the expected shape,
/// and the overlay's tests report every tuning whose ability does not.
#[derive(Clone, Copy, Debug)]
struct Expect {
    index: u8,
    shape: Shape,
}

impl Expect {
    /// Ability `index` of the front face, expected to have `shape`. The derived flags hold
    /// only the first eight abilities, so a later one cannot be tuned.
    const fn ability(index: u8, shape: Shape) -> Expect {
        assert!(index < 8, "only the first eight abilities can be tuned");
        Expect { index, shape }
    }
}

/// What a tuned ability is expected to be, read from its definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// A loyalty ability that puts on (or, negative, removes) this many loyalty counters.
    Loyalty(i8),
    /// Cycling: "[Cost], Discard this card: Draw a card."
    Cycling,
    /// An activated ability that is neither a mana ability nor a loyalty ability.
    Activated,
}

impl Shape {
    fn matches(self, a: &Ability) -> bool {
        let cost = a.cost();
        match self {
            Shape::Loyalty(n) => cost.loyalty_change() == Some(n),
            Shape::Cycling => {
                a.kind() == AbilityKind::Activated
                    && cost.does(CostAction::Discard)
                    && matches!(a.body(), Behavior::Data(Effect::Draw { .. }))
            }
            Shape::Activated => {
                a.kind() == AbilityKind::Activated && cost.loyalty_change().is_none()
            }
        }
    }
}

impl Tuning {
    const fn value(name: &'static str, value: i16) -> Tuning {
        Tuning {
            name,
            value: Some(value),
            never_cast: false,
            abilities: [None; MOST_TUNED_ABILITIES],
        }
    }

    const fn never_cast(self) -> Tuning {
        Tuning {
            never_cast: true,
            ..self
        }
    }

    const fn never(self, ability: Expect) -> Tuning {
        self.tune(Tune::Never, ability)
    }

    const fn tuned_as_not_drawing(self, ability: Expect) -> Tuning {
        self.tune(Tune::AsNotDrawing, ability)
    }

    const fn tune(mut self, tune: Tune, ability: Expect) -> Tuning {
        let mut i = 0;
        while i < MOST_TUNED_ABILITIES {
            if self.abilities[i].is_none() {
                self.abilities[i] = Some((tune, ability));
                return self;
            }
            i += 1;
        }
        panic!("a tuning names more than MOST_TUNED_ABILITIES abilities");
    }

    /// The abilities it tunes.
    fn tuned(&self) -> impl Iterator<Item = (Tune, Expect)> + '_ {
        self.abilities.iter().flatten().copied()
    }
}

/// Every card's tuning, by name.
const TUNINGS: &[Tuning] = &[
    Tuning::value(names::JACE_THE_MIND_SCULPTOR, 90),
    Tuning::value(names::TEFERI_HERO_OF_DOMINARIA, 85),
    // −3: "Return up to one target artifact, creature, or enchantment to its owner's hand. Draw
    // a card." Tuned as removal that is used however low the library is.
    Tuning::value(names::TEFERI_TIME_RAVELER, 80)
        .tuned_as_not_drawing(Expect::ability(2, Shape::Loyalty(-3))),
    Tuning::value(names::THE_WANDERING_EMPEROR, 80),
    Tuning::value(names::OVERLORD_OF_THE_MISTMOORS, 75),
    Tuning::value(names::FRACTURED_IDENTITY, 70),
    Tuning::value(names::ADELINE_RESPLENDENT_CATHAR, 70),
    Tuning::value(names::BEZA_THE_BOUNDING_SPRING, 65),
    // −8: "Draw seven cards. Then if your library has no cards in it, you win the game." Its +1
    // draws a card too. The policy only activates them with this Jace on the battlefield,
    // whose static ability makes drawing from an empty library win, so the policy never held
    // them back for a low library, and the −8 is scored by library size instead.
    Tuning::value(names::JACE_WIELDER_OF_MYSTERIES, 65)
        .tuned_as_not_drawing(Expect::ability(1, Shape::Loyalty(1)))
        .tuned_as_not_drawing(Expect::ability(2, Shape::Loyalty(-8))),
    Tuning::value(names::SPHINX_OF_FALSE_CONCLUSIONS, 65),
    Tuning::value(names::ABHORRENT_OCULUS, 60),
    Tuning::value(names::MANA_DRAIN, 60),
    Tuning::value(names::DAMN, 55),
    Tuning::value(names::COUNTERSPELL, 55),
    Tuning::value(names::ELITE_SPELLBINDER, 55),
    Tuning::value(names::GET_LOST, 55),
    // Its waterbend ability sacrifices it, which returns the permanent it exiled.
    Tuning::value(names::AANGS_ICEBERG, 55).never(Expect::ability(1, Shape::Activated)),
    Tuning::value(names::WINDS_OF_ABANDON, 50),
    Tuning::value(names::MALCOLM_ALLURING_SCOUNDREL, 50),
    Tuning::value(names::THIEVING_SKYDIVER, 50),
    Tuning::value(names::JACE_VRYNS_PRODIGY, 50),
    Tuning::value(names::OUST, 50),
    Tuning::value(names::NO_MORE_LIES, 50),
    Tuning::value(names::SUBTLETY, 45),
    Tuning::value(names::SWIFT_RECONFIGURATION, 45),
    Tuning::value(names::SINK_INTO_STUPOR, 45),
    Tuning::value(names::PORTABLE_HOLE, 40),
    Tuning::value(names::STOCK_UP, 40),
    Tuning::value(names::THUNDERTRAP_TRAINER, 40),
    Tuning::value(names::TIME_SPIRAL, 40),
    Tuning::value(names::TIMETWISTER, 40),
    Tuning::value(names::CURRENCY_CONVERTER, 35),
    Tuning::value(names::REPRIEVE, 35),
    Tuning::value(names::SNAPCASTER_MAGE, 35),
    Tuning::value(names::STAFF_OF_THE_STORYTELLER, 35),
    Tuning::value(names::THIRST_FOR_DISCOVERY, 35),
    Tuning::value(names::TRINKET_MAGE, 30),
    Tuning::value(names::MANA_TITHE, 30),
    Tuning::value(names::STRIP_MINE, 15),
    Tuning::value(names::TUNDRA, 14),
    Tuning::value(names::FLOODFARM_VERGE, 13),
    Tuning::value(names::SEACHROME_COAST, 13),
    // Cycling {3}: "{3}, Discard this card: Draw a card." Tuned as a land whose cycling is used
    // however low the library is.
    Tuning::value(names::SPARAS_HEADQUARTERS, 12)
        .tuned_as_not_drawing(Expect::ability(4, Shape::Cycling)),
    Tuning::value(names::PLAINS, 10),
    Tuning::value(names::ISLAND, 10),
    // Its coin flips, 3 damage on half its controller's upkeeps, cost more life over the
    // mirror's long games than its mana is worth to a policy that does not plan its turns. On
    // the battlefield it is worth nothing either: no removal is spent on it.
    Tuning::value(names::MANA_CRYPT, 0).never_cast(),
];

/// Applies the tuning for the card `def`, if it has one, to what was derived for it. An
/// ability that no longer has the shape its tuning expects is left as derived.
pub(crate) fn apply(def: &CardDef, info: &mut CardInfo) {
    let Some(tuning) = TUNINGS.iter().find(|tuning| tuning.name == def.name) else {
        return;
    };
    if let Some(value) = tuning.value {
        info.value = value;
    }
    info.never_cast |= tuning.never_cast;
    let abilities = def.front().abilities;
    for (tune, expect) in tuning.tuned() {
        let index = usize::from(expect.index);
        if !abilities
            .get(index)
            .is_some_and(|a| expect.shape.matches(a))
        {
            continue;
        }
        let bit = 1 << expect.index;
        match tune {
            Tune::Never => info.never |= bit,
            Tune::AsNotDrawing => info.draws &= !bit,
        }
    }
}

#[cfg(test)]
mod tests;
