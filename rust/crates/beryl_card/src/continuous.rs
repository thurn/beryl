//! Continuous effects: what a static ability does all the time, and the replacement effects
//! and layered modifications cards create.

use crate::characteristics::{Colors, Keywords, Subtype, Types};
use crate::effect::{Amount, Condition, Objects, Players};
use crate::filter::Filter;
use crate::index::{AbilityIx, GrantIx};

/// A change to a rule of the game rather than to an object's characteristics
/// ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311)): "Some continuous
/// effects affect game rules rather than objects." A static ability makes one all the time
/// ([CR 604.1](https://yawgatog.com/resources/magic-rules/#R6041): "Static abilities do
/// something all the time rather than being activated or triggered."), a resolved spell or
/// ability for as long as it says.
///
/// It says who may or may not do what, and when: "Each opponent can cast spells only any
/// time they could cast a sorcery." is each opponent, casting spells, only as a sorcery. A
/// static ability's "as long as" is its [`condition`](crate::Ability::condition), and how
/// long a resolved one lasts is its effect's duration.
#[derive(Clone, Copy, Debug)]
pub struct RuleChange {
    /// Who it applies to, read for the controller of the ability that makes it: of a static
    /// ability, whoever that is now, and of a resolving one, as it resolves.
    pub players: Players,
    /// What they do that it governs.
    pub what: Governed,
    /// When they may do it, or may only.
    pub when: When,
}

/// What a [`RuleChange`] governs the timing of.
#[derive(Clone, Copy, Debug)]
pub enum Governed {
    /// Casting the spells a filter accepts, read of each spell's own characteristics.
    CastSpells(Filter),
    /// Activating the loyalty abilities of these permanents.
    ActivateLoyaltyAbilities(Objects),
}

/// When a [`RuleChange`] has what it governs done.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum When {
    /// "Only any time they could cast a sorcery": a restriction, which beats a permission
    /// ([CR 101.2](https://yawgatog.com/resources/magic-rules/#R1012)): "When a rule or
    /// effect allows or directs something to happen, and another effect states that it
    /// can't happen, the 'can't' effect takes precedence."
    OnlyAsSorcery,
    /// "Any time they could cast an instant", "as though it had flash"
    /// ([CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a)): "'Flash' means
    /// 'You may play this card any time you could cast an instant.'" A permission.
    AsInstant,
}

impl RuleChange {
    /// The change in words, said of who it applies to, as in "player 1 casting sorceries
    /// as though they had flash": how a description of a game names it, as a save's record
    /// of what a checkpoint would lose does.
    pub fn words(&self) -> String {
        match (self.what, self.when) {
            (Governed::CastSpells(filter), When::OnlyAsSorcery) => {
                format!(
                    "casting {} only when they could cast a sorcery",
                    spells(filter)
                )
            }
            (Governed::CastSpells(filter), When::AsInstant) => {
                format!("casting {} as though they had flash", spells(filter))
            }
            (Governed::ActivateLoyaltyAbilities(_), When::OnlyAsSorcery) => {
                "with loyalty abilities to activate only as a sorcery".to_owned()
            }
            (Governed::ActivateLoyaltyAbilities(_), When::AsInstant) => {
                "with loyalty abilities to activate at instant speed".to_owned()
            }
        }
    }
}

/// The spells a filter accepts, in a word or two.
fn spells(filter: Filter) -> &'static str {
    match filter {
        Filter::Any => "spells",
        Filter::Types(types) if types == Types::SORCERY => "sorceries",
        Filter::Types(types) if types == Types::INSTANT => "instants",
        Filter::Types(types) if types == Types::CREATURE => "creature spells",
        _ => "some spells",
    }
}

/// A replacement effect of a permanent's own static ability: the event it replaces. Most
/// modify how the permanent enters
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
    /// "If you would draw a card [while condition], [effect] instead."
    /// ([CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)): "Effects that use
    /// the word 'instead' are replacement effects." Not a way of entering: the event is a
    /// draw by the permanent's controller, the ability's
    /// [`condition`](crate::Ability::condition) is the "while", read for that player before
    /// each card they would draw, and its [`body`](crate::Ability::body) is what happens
    /// instead of that draw
    /// ([CR 121.6a](https://yawgatog.com/resources/magic-rules/#R1216a)): "An effect that
    /// replaces a card draw is applied even if no cards could be drawn because there are no
    /// cards in the affected player's library."
    WouldDraw,
}

/// A static ability's continuous effect: what it affects, when, and how
/// ([CR 604.1](https://yawgatog.com/resources/magic-rules/#R6041)): "Static abilities do
/// something all the time rather than being activated or triggered."
///
/// Its expressions are read for the permanent with the ability and that permanent's
/// controller, at each moment the game looks
/// ([CR 611.3a](https://yawgatog.com/resources/magic-rules/#R6113a)): "A continuous effect
/// generated by a static ability isn't 'locked in'; it applies at any given moment to
/// whatever its text indicates." No spell or ability is resolving, so one that means a
/// target, X or what an instruction before did says nothing here
/// ([`LayerSpec::needs_a_resolution`]), and no game is played with it.
#[derive(Clone, Copy, Debug)]
pub struct LayerSpec {
    /// The permanents it affects: [`Objects::This`] for the permanent with the ability,
    /// [`Objects::AttachedTo`] for the one it enchants, [`Objects::All`] for those a filter
    /// accepts ("other Squirrels you control").
    pub affected: Objects,
    /// Its "as long as", or `None` for an ability that applies all the time.
    pub condition: Option<Condition>,
    /// What it does to each permanent it affects, in the order the card says it: one
    /// effect, whose parts may apply in different layers ([CR 613.6]): "If an effect should
    /// be applied in different layers and/or sublayers, the parts of the effect each apply in
    /// their appropriate ones. If an effect starts to apply in one layer and/or sublayer, it
    /// will continue to be applied to the same set of objects in each other applicable layer and/or
    /// sublayer, even if the ability generating the effect is removed during this process."
    /// "All creatures lose all abilities and have base power and toughness 1/1" is one
    /// static ability with two parts, in layers 6 and 7b.
    ///
    /// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
    pub modifications: &'static [Modification],
}

impl LayerSpec {
    /// Whether this says something only a resolving spell or ability has ([`Objects::Target`],
    /// [`Objects::Those`], [`Amount::X`](crate::Amount::X), [`Condition::Did`] and the like),
    /// which a static ability, having no resolution, cannot mean.
    pub const fn needs_a_resolution(&self) -> bool {
        self.affected.needs_a_resolution()
            || match &self.condition {
                Some(condition) => condition.needs_a_resolution(),
                None => false,
            }
    }
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
    /// "Power is equal to [amount]". As a static ability of the permanent itself with no
    /// condition it is characteristic-defining
    /// ([CR 604.3](https://yawgatog.com/resources/magic-rules/#R6043)) and applies in layer
    /// 7a, the amount read as the layer applies and not once
    /// ([CR 613.4a](https://yawgatog.com/resources/magic-rules/#R6134a)): "Effects from
    /// characteristic-defining abilities that define power and/or toughness are applied."
    SetPower(&'static Amount),
    /// Layer 7c: "gets +X/+Y".
    ModifyPt(i8, i8),
    /// Layer 6: "loses all abilities" ([CR 613.1f]): "Layer 6: Ability-adding effects,
    /// keyword counters, ability-removing effects, and effects that say an object can't have
    /// an ability are applied." It removes its printed abilities and those that effects
    /// applied before it in layer 6 granted; one applied after it still grants its ability.
    ///
    /// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
    LoseAllAbilities,
    /// Layer 6: "loses [ability]", the ability at this index of the affected permanent's
    /// own face, as a static ability of that permanent says it of itself.
    LoseAbility(AbilityIx),
    /// Layer 5: "is [colors]", replacing its colors ([CR 613.1e]): "Layer 5: Color-changing
    /// effects are applied."
    ///
    /// [CR 613.1e]: https://yawgatog.com/resources/magic-rules/#R6131e
    SetColors(Colors),
    /// Layer 5: "is [colors] in addition to its other colors".
    AddColors(Colors),
    /// Layer 7b: "has base power and toughness [power]/[toughness]" ([CR 613.4b]): "Effects
    /// that set power and/or toughness to a specific number or value are applied." A
    /// resolving spell or ability settles each amount as its effect begins ([CR 608.2h]); a
    /// static ability's is read for each permanent it affects as the layer applies.
    ///
    /// [CR 613.4b]: https://yawgatog.com/resources/magic-rules/#R6134b
    /// [CR 608.2h]: https://yawgatog.com/resources/magic-rules/#R6082h
    SetBasePt(&'static Amount, &'static Amount),
    /// Layer 7d: "switch its power and toughness" ([CR 613.4d]): "Effects that switch a
    /// creature's power and toughness are applied."
    ///
    /// [CR 613.4d]: https://yawgatog.com/resources/magic-rules/#R6134d
    SwitchPt,
}

/// The layers and sublayers continuous effects apply in, in order
/// ([CR 613.1](https://yawgatog.com/resources/magic-rules/#R6131),
/// [CR 613.4](https://yawgatog.com/resources/magic-rules/#R6134)). Layer 1, copy effects, is
/// applied by choosing the copiable values before any of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Control = 2,
    Type = 4,
    Color = 5,
    Ability = 6,
    /// 7a: characteristic-defining abilities that define power and toughness.
    PtDefining = 7,
    /// 7b: effects that set power and toughness to a number or value.
    PtSetting = 8,
    /// 7c: effects and counters that modify power and toughness.
    PtModifying = 9,
    /// 7d: effects that switch power and toughness.
    PtSwitching = 10,
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

    /// A description of a game names a rule change in words made from what it says: those
    /// a save holds for the three shapes the first cards with one wrote.
    #[test]
    fn a_rule_change_is_named_in_the_words_saves_hold() {
        let change = |players, what, when| {
            RuleChange {
                players,
                what,
                when,
            }
            .words()
        };
        let this = Governed::ActivateLoyaltyAbilities(Objects::This);
        assert_eq!(
            change(
                Players::EachOpponent,
                Governed::CastSpells(Filter::Any),
                When::OnlyAsSorcery
            ),
            "casting spells only when they could cast a sorcery"
        );
        assert_eq!(
            change(
                Players::You,
                Governed::CastSpells(Filter::Types(Types::SORCERY)),
                When::AsInstant
            ),
            "casting sorceries as though they had flash"
        );
        assert_eq!(
            change(Players::You, this, When::AsInstant),
            "with loyalty abilities to activate at instant speed"
        );
    }
}
