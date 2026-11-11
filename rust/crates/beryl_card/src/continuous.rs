//! Continuous effects: what a static ability does all the time, and the replacement effects
//! and layered modifications cards create.

use crate::characteristics::{Keywords, Subtype, Types};
use crate::ids::{CounterKind, Seat};
use crate::index::GrantIx;

/// A static ability that changes a rule of the game rather than an object's characteristics
/// ([CR 604.1](https://yawgatog.com/resources/magic-rules/#R6041)): "Static abilities do
/// something all the time rather than being activated or triggered."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rule {
    None,
    /// "Each opponent can cast spells only any time they could cast a sorcery." (Teferi, Time
    /// Raveler)
    OpponentsCastAsSorceries,
    /// "As long as this entered this turn, you may activate its loyalty abilities any time you
    /// could cast an instant." (The Wandering Emperor)
    InstantLoyaltyTheTurnItEntered,
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

/// What a continuous effect does to a permanent. The layer it applies in follows from the
/// modification ([CR 613.1](https://yawgatog.com/resources/magic-rules/#R6131)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Modification {
    /// Layer 2: "gain control of".
    Control(Seat),
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

    pub fn layer(self) -> Layer {
        match self {
            Modification::Control(_) => Layer::Control,
            Modification::AddTypes(_)
            | Modification::RemoveTypes(_)
            | Modification::SetTypes(_)
            | Modification::AddSubtype(_) => Layer::Type,
            Modification::AddKeywords(_) | Modification::Grant(_) => Layer::Ability,
            Modification::PowerIsCreaturesYouControl => Layer::PtDefining,
            Modification::ModifyPt(..) => Layer::PtModifying,
        }
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
