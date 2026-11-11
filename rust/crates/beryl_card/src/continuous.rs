//! Continuous effects: what a static ability does all the time, and the replacement effects
//! and layered modifications cards create.

use crate::characteristics::{Keywords, Subtype, Types};
use crate::counter::CounterKind;
use crate::filter::Whose;
use crate::index::GrantIx;

/// The rules, each with its words. A `const fn` cannot enumerate an enum's variants.
macro_rules! rules {
    ($($(#[$doc:meta])* $variant:ident = $words:literal),* $(,)?) => {
        /// A change to a rule of the game rather than to an object's characteristics
        /// ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311)): "Some
        /// continuous effects affect game rules rather than objects." A static ability
        /// makes one all the time
        /// ([CR 604.1](https://yawgatog.com/resources/magic-rules/#R6041): "Static abilities
        /// do something all the time rather than being activated or triggered."), a resolved
        /// spell or ability for as long as it says.
        ///
        /// The list is open: a new rule is an entry here, and a set of rules ([`Rules`])
        /// has room for however many there are.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum Rule {
            $($(#[$doc])* $variant),*
        }

        impl Rule {
            /// Every rule, in the order of the list.
            pub const ALL: &[Rule] = &[$(Rule::$variant),*];

            /// The change in words, said of who it applies to, as in "player 1 casting
            /// sorceries as though they had flash": how a description of a game names
            /// it, as a save's record of what a checkpoint would lose does.
            pub const fn words(self) -> &'static str {
                match self {
                    $(Rule::$variant => $words),*
                }
            }
        }
    };
}

rules! {
    /// No rule: an ability that changes none.
    None = "under no changed rule",
    /// "Each opponent can cast spells only any time they could cast a sorcery." (Teferi, Time
    /// Raveler)
    OpponentsCastAsSorceries = "casting spells only when they could cast a sorcery",
    /// "As long as this entered this turn, you may activate its loyalty abilities any time you
    /// could cast an instant." (The Wandering Emperor)
    InstantLoyaltyTheTurnItEntered = "with loyalty abilities to activate at instant speed",
    /// "You may cast sorcery spells as though they had flash." (Teferi, Time Raveler)
    SorceriesAsFlash = "casting sorceries as though they had flash",
}

impl Rule {
    /// Whose rule it changes, read for the controller of what makes the change: the players
    /// it applies to. `None` for a rule about the permanent with the ability, which is
    /// asked of that permanent.
    #[inline]
    pub const fn whose(self) -> Option<Whose> {
        match self {
            Rule::None | Rule::InstantLoyaltyTheTurnItEntered => None,
            Rule::OpponentsCastAsSorceries => Some(Whose::AnOpponent),
            Rule::SorceriesAsFlash => Some(Whose::You),
        }
    }
}

/// A set of [`Rule`]s: a bit for each rule of the list, in as many bytes as the list needs,
/// so a rule added to the list widens every set of them and no mask is widened by hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rules([u8; Rules::BYTES]);

impl Rules {
    const BYTES: usize = Rule::ALL.len().div_ceil(8);

    pub const NONE: Rules = Rules([0; Rules::BYTES]);

    /// The rules that apply to players ([`Rule::whose`]).
    pub const OF_PLAYERS: Rules = {
        let mut rules = Rules::NONE;
        let mut i = 0;
        while i < Rule::ALL.len() {
            if Rule::ALL[i].whose().is_some() {
                rules = rules.with(Rule::ALL[i]);
            }
            i += 1;
        }
        rules
    };

    /// The set with `rule` in it too.
    #[inline]
    #[must_use]
    pub const fn with(mut self, rule: Rule) -> Rules {
        self.0[rule as usize / 8] |= 1 << (rule as usize % 8);
        self
    }

    #[inline]
    #[must_use]
    pub const fn union(mut self, other: Rules) -> Rules {
        let mut i = 0;
        while i < Rules::BYTES {
            self.0[i] |= other.0[i];
            i += 1;
        }
        self
    }

    /// The rules in both sets.
    #[inline]
    #[must_use]
    pub const fn common(mut self, other: Rules) -> Rules {
        let mut i = 0;
        while i < Rules::BYTES {
            self.0[i] &= other.0[i];
            i += 1;
        }
        self
    }

    #[inline]
    pub const fn contains(self, rule: Rule) -> bool {
        self.0[rule as usize / 8] & (1 << (rule as usize % 8)) != 0
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        let mut i = 0;
        while i < Rules::BYTES {
            if self.0[i] != 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// Its rules, in the order of the list.
    #[inline]
    pub fn iter(self) -> impl Iterator<Item = Rule> {
        Rule::ALL.iter().copied().filter(move |&r| self.contains(r))
    }
}

impl Default for Rules {
    fn default() -> Self {
        Rules::NONE
    }
}

/// A replacement effect of a permanent's own static ability: most modify how it enters
/// ([CR 614.1c](https://yawgatog.com/resources/magic-rules/#R6141c)): "Effects that read
/// '[This permanent] enters with . . . ,' 'As [this permanent] enters . . . ,' or '[This
/// permanent] enters as . . . ' are replacement effects."
///
/// [CR 614.12](https://yawgatog.com/resources/magic-rules/#R61412): "Some replacement effects
/// modify how a permanent enters the battlefield. ... Such effects may come from the
/// permanent itself if they affect only that permanent".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Replacement {
    None,
    /// "This enters tapped."
    EntersTapped,
    /// "This enters tapped unless you control N or fewer other lands." (Seachrome Coast)
    EntersTappedUnlessFewLands(u8),
    /// "As this enters, you may pay N life. If you don't, it enters tapped." (Soporific
    /// Springs) Whoever puts it onto the battlefield asks its controller first.
    EntersTappedUnlessPayLife(u8),
    /// "If you would draw a card while your library has no cards in it, you win the game
    /// instead." (Jace, Wielder of Mysteries) Not a way of entering: a draw replacement
    /// ([CR 121.6a](https://yawgatog.com/resources/magic-rules/#R1216a)): "An effect that
    /// replaces a card draw is applied even if no cards could be drawn because there are no
    /// cards in the affected player's library."
    WinInsteadOfDrawingFromEmptyLibrary,
}

/// A static ability's continuous effect: what it affects, when, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerSpec {
    pub affected: Affected,
    pub condition: StaticCondition,
    pub modification: Modification,
}

/// Which permanent a static ability affects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Affected {
    /// The permanent with the ability.
    Itself,
    /// The permanent an Aura with the ability enchants.
    Enchanted,
}

/// When a static ability applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StaticCondition {
    Always,
    /// "As long as it has a [kind] counter on it".
    HasCounter(CounterKind),
    /// Impending's "As long as this permanent's impending cost was paid and it has a time
    /// counter on it" (CR 702.176a).
    Impending,
}

/// What a continuous effect does to a permanent, as a card says it. The engine applies its
/// resolved form (`beryl_kernel`'s `LayerChange`), which names the layer it applies in
/// ([CR 613.1](https://yawgatog.com/resources/magic-rules/#R6131)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Modification {
    /// Layer 2: "gain control of": the controller of the ability that says it does.
    Control,
    /// Layer 4: "becomes [types] in addition to its other types".
    AddTypes(Types),
    /// Layer 4: "isn't a [type]".
    RemoveTypes(Types),
    /// Layer 4: "is a [types] and loses all other card types". Subtypes go with the card
    /// types they belong to; subtypes it gains are added separately.
    SetTypes(Types),
    /// Layer 4: "is a [subtype]".
    AddSubtype(Subtype),
    /// Layer 6: "gains [keywords]".
    AddKeywords(Keywords),
    /// Layer 6: "has [ability]": the ability at this index of the effect source's front
    /// face's [`Face::grants`], such as the crew ability Swift Reconfiguration grants.
    Grant(GrantIx),
    /// Layer 7a: a characteristic-defining ability, "power is equal to the number of
    /// creatures you control" (Adeline).
    PowerIsCreaturesYouControl,
    /// Layer 7c: "gets +X/+Y".
    ModifyPt(i8, i8),
}

/// The layers and sublayers continuous effects apply in, in order
/// ([CR 613.1](https://yawgatog.com/resources/magic-rules/#R6131),
/// [CR 613.4](https://yawgatog.com/resources/magic-rules/#R6134)). Layer 1, copy effects, is
/// applied by choosing the copiable values before any of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Control = 2,
    Type = 4,
    Ability = 6,
    /// 7a: characteristic-defining abilities that define power and toughness.
    PtDefining = 7,
    /// 7c: effects and counters that modify power and toughness.
    PtModifying = 9,
}

impl Modification {
    /// "Has [ability]", the ability at `index` of the source's [`Face::grants`], counting
    /// from 0: [`Modification::Grant`], as a card file writes it.
    pub const fn grant(index: u8) -> Modification {
        Modification::Grant(GrantIx::new(index))
    }
}

/// How long a continuous effect from a resolving spell or ability lasts, as the card says it
/// ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)): "A continuous effect
/// generated by the resolution of a spell or ability lasts as long as stated by the spell or
/// ability creating it (such as 'until end of turn'). If no duration is stated, it lasts
/// until the end of the game."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Duration {
    /// "Until end of turn", "this turn"
    /// ([CR 514.2](https://yawgatog.com/resources/magic-rules/#R5142)).
    EndOfTurn,
    /// "Until your next turn": it ends as the next turn of the ability's controller begins.
    UntilYourNextTurn,
    /// No duration stated.
    Indefinite,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A set has a bit of its own for every rule of the list, however long the list is: a
    /// rule added to it needs no wider mask.
    #[test]
    fn a_set_of_rules_has_room_for_every_rule_of_the_list() {
        assert!(Rules::BYTES * 8 >= Rule::ALL.len());
        let mut all = Rules::NONE;
        for (i, &rule) in Rule::ALL.iter().enumerate() {
            assert_eq!(rule as usize, i);
            let only = Rules::NONE.with(rule);
            assert_eq!(only.iter().collect::<Vec<_>>(), [rule]);
            assert!(!all.contains(rule));
            all = all.union(only);
        }
        assert_eq!(all.iter().collect::<Vec<_>>(), Rule::ALL);
        assert!(Rules::NONE.is_empty() && Rules::NONE.common(all).is_empty());
    }

    #[test]
    fn the_rules_of_players_are_those_that_name_whose_they_are() {
        for &rule in Rule::ALL {
            assert_eq!(Rules::OF_PLAYERS.contains(rule), rule.whose().is_some());
        }
    }
}
