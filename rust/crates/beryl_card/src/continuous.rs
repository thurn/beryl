//! Continuous effects: what a static ability does all the time, and the replacement effects
//! and layered modifications cards create.

use crate::characteristics::{BasicLandTypes, Colors, Keywords, Subtype, Types};
use crate::effect::{Amount, Condition, Objects, Players};
use crate::filter::Filter;
use crate::ids::Zone;
use crate::index::{AbilityIx, GrantIx};
use crate::prompt::Purpose;

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

/// A replacement effect of a static ability
/// ([CR 614.1](https://yawgatog.com/resources/magic-rules/#R6141)): "Some continuous effects
/// are replacement effects." It is what it replaces ([`Replaced`]) and what happens instead
/// ([`Rewrite`]). Whether it applies at a given moment is also its ability's
/// [`condition`](crate::Ability::condition), and a [`Rewrite::Instead`] is its ability's
/// [`body`](crate::Ability::body).
///
/// The engine applies the pairs it has code for and refuses the rest when it builds a game's
/// card table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Replacement {
    pub replaces: Replaced,
    pub rewrite: Rewrite,
}

impl Replacement {
    /// No replacement: the ability is of another kind.
    pub const NONE: Replacement = Replacement {
        replaces: Replaced::None,
        rewrite: Rewrite::None,
    };

    /// "This enters tapped."
    pub const ENTERS_TAPPED: Replacement = Replacement::this_enters(Rewrite::EntersTapped);

    /// A replacement of how the permanent with the ability enters.
    pub const fn this_enters(rewrite: Rewrite) -> Replacement {
        Replacement {
            replaces: Replaced::ThisEnters,
            rewrite,
        }
    }

    /// "This enters tapped unless you control N or fewer other lands." (Seachrome Coast)
    pub const fn enters_tapped_unless_few_lands(n: u8) -> Replacement {
        Replacement::this_enters(Rewrite::EntersTappedUnlessFewLands(n))
    }

    /// "As this enters, you may pay N life. If you don't, it enters tapped." (Soporific
    /// Springs)
    pub const fn enters_tapped_unless_pay_life(n: u8) -> Replacement {
        Replacement::this_enters(Rewrite::EntersTappedUnlessPayLife(n))
    }

    /// "As this enters, choose a color" (or a creature type, or a card name)
    /// ([CR 614.12a](https://yawgatog.com/resources/magic-rules/#R61412a)): "If a replacement
    /// effect that modifies how a permanent enters the battlefield requires a choice, that
    /// choice is made before the permanent enters the battlefield." The permanent remembers
    /// the value for as long as it stays.
    pub const fn choose_as_enters(kind: ChosenKind) -> Replacement {
        Replacement::this_enters(Rewrite::Choose(kind))
    }

    /// "If [players] would draw a card, [the ability's body] instead."
    pub const fn draws(players: Players) -> Replacement {
        Replacement {
            replaces: Replaced::Draw(players, Draws::Each),
            rewrite: Rewrite::Instead,
        }
    }

    /// "If [players] would draw a card except the first one they draw in each of their draw
    /// steps, [the ability's body] instead." In the body, "that player" is the player who
    /// would draw.
    pub const fn draws_except_first_in_draw_step(players: Players) -> Replacement {
        Replacement {
            replaces: Replaced::Draw(players, Draws::ExceptFirstInDrawStep),
            rewrite: Rewrite::Instead,
        }
    }

    /// Dredge N ([CR 702.52a]): "Dredge is a static ability that functions only while the
    /// card with dredge is in a player's graveyard. 'Dredge N' means 'As long as you have at
    /// least N cards in your library, if you would draw a card, you may instead mill N cards
    /// and return this card from your graveyard to your hand.'"
    ///
    /// [CR 702.52a]: https://yawgatog.com/resources/magic-rules/#R70252a
    pub const fn dredge(n: u8) -> Replacement {
        Replacement {
            replaces: Replaced::Draw(Players::You, Draws::Each),
            rewrite: Rewrite::Dredge(n),
        }
    }

    /// "If [objects] would be put into [zone] from anywhere, put it into [instead] instead":
    /// "If a card or token would be put into a graveyard from anywhere, exile it instead."
    pub const fn put_into(objects: &'static Filter, zone: Zone, instead: Zone) -> Replacement {
        Replacement {
            replaces: Replaced::PutInto(objects, zone),
            rewrite: Rewrite::ToZone(instead),
        }
    }

    /// "If [objects] would enter and it wasn't cast, put it into [instead] instead": "If a
    /// nontoken creature would enter and it wasn't cast, exile it instead."
    pub const fn enters_not_cast(objects: &'static Filter, instead: Zone) -> Replacement {
        Replacement {
            replaces: Replaced::Enters {
                objects,
                not_cast: true,
            },
            rewrite: Rewrite::ToZone(instead),
        }
    }
}

/// The event a replacement effect replaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Replaced {
    /// No event.
    None,
    /// The permanent with the ability entering the battlefield
    /// ([CR 614.1c](https://yawgatog.com/resources/magic-rules/#R6141c)): "Effects that read
    /// '[This permanent] enters with . . . ,' 'As [this permanent] enters . . . ,' or '[This
    /// permanent] enters as . . . ' are replacement effects."
    /// ([CR 614.12](https://yawgatog.com/resources/magic-rules/#R61412)): "Some replacement
    /// effects modify how a permanent enters the battlefield. ... Such effects may come from
    /// the permanent itself if they affect only that permanent".
    ThisEnters,
    /// A card drawn by these players, "you" being the permanent's controller
    /// ([CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)): "Effects that use the
    /// word 'instead' are replacement effects." It applies even to a draw from an empty
    /// library ([CR 121.6a](https://yawgatog.com/resources/magic-rules/#R1216a)): "An effect
    /// that replaces a card draw is applied even if no cards could be drawn because there are
    /// no cards in the affected player's library." Which of their draws it replaces is
    /// [`Draws`].
    Draw(Players, Draws),
    /// An object the filter accepts, read for the permanent's controller, being put into a
    /// zone from any zone.
    PutInto(&'static Filter, Zone),
    /// An object the filter accepts entering the battlefield from any zone, other than the
    /// permanent with the ability. The filter is read for the permanent's controller and asked
    /// of the object as it would exist on the battlefield
    /// ([CR 614.12](https://yawgatog.com/resources/magic-rules/#R61412)): "To determine which
    /// replacement effects apply and how they apply, check the characteristics of the
    /// permanent as it would exist on the battlefield". With `not_cast`, only an object that
    /// is not the permanent a resolving spell becomes ("and it wasn't cast").
    Enters {
        objects: &'static Filter,
        not_cast: bool,
    },
}

/// Which of a player's draws a replacement of draws ([`Replaced::Draw`]) replaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Draws {
    /// Every card they would draw.
    Each,
    /// Every card but the first one they draw in each of their draw steps
    /// ([CR 504.1](https://yawgatog.com/resources/magic-rules/#R5041)): "First, the active
    /// player draws a card." A draw that was replaced was never made, so the first card they
    /// do draw in their draw step is that first one.
    ExceptFirstInDrawStep,
}

/// What a replacement effect has happen instead of the event it replaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rewrite {
    /// Nothing.
    None,
    /// The object is put into this zone instead ("exile it instead").
    ToZone(Zone),
    /// It enters tapped.
    EntersTapped,
    /// It enters tapped unless its controller controls N or fewer other lands.
    EntersTappedUnlessFewLands(u8),
    /// Its controller may pay N life as it enters; if they don't, it enters tapped. Whoever
    /// puts it onto the battlefield asks its controller first.
    EntersTappedUnlessPayLife(u8),
    /// The ability's [`body`](crate::Ability::body) happens instead.
    Instead,
    /// Dredge N ([`Replacement::dredge`]): its owner may mill N cards instead of drawing, if
    /// their library has at least N, and the card returns from their graveyard to their
    /// hand. It works only from its owner's graveyard and is optional, so it is a choice even
    /// when it is the only replacement of the draw.
    Dredge(u8),
    /// Its controller chooses a value of this kind as it enters, which the permanent
    /// remembers ([`Replacement::choose_as_enters`]).
    Choose(ChosenKind),
}

/// The kind of value a permanent chooses as it enters ([`Rewrite::Choose`]): one entry of a
/// catalog, asked in the catalog prompt shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChosenKind {
    /// One of the five colors ([CR 105.4](https://yawgatog.com/resources/magic-rules/#R1054)):
    /// "If a player is asked to choose a color, they must choose one of the five colors."
    Color,
    /// A creature type ([CR 205.3e](https://yawgatog.com/resources/magic-rules/#R2053e)): "If
    /// an effect instructs a player to choose a subtype, that player must choose one, and
    /// only one, existing subtype".
    CreatureType,
    /// A card name ([CR 201.4](https://yawgatog.com/resources/magic-rules/#R2014)): "If an
    /// effect instructs a player to choose a card name, the player must choose the name of a
    /// card in the Oracle card reference."
    CardName,
}

impl ChosenKind {
    /// The purpose the choice is asked with.
    pub const fn purpose(self) -> Purpose {
        match self {
            ChosenKind::Color => Purpose::Color,
            ChosenKind::CreatureType => Purpose::CreatureType,
            ChosenKind::CardName => Purpose::CardName,
        }
    }

    /// The kind a question with `purpose` asks for, if it asks for a chosen value.
    pub const fn of_purpose(purpose: Purpose) -> Option<ChosenKind> {
        match purpose {
            Purpose::Color => Some(ChosenKind::Color),
            Purpose::CreatureType => Some(ChosenKind::CreatureType),
            Purpose::CardName => Some(ChosenKind::CardName),
            _ => None,
        }
    }
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
            || Modification::any_asks(self.modifications)
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
    /// Layer 4: "is a [subtype] in addition to its other types". A land that becomes a basic land type it
    /// was not gains that type's intrinsic mana ability ([CR 305.6]).
    ///
    /// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
    AddSubtype(Subtype),
    /// Layer 4: "is a [subtype]", which replaces its subtypes of the same kind ([CR 205.1a]):
    /// "when an effect sets one or more of an object's subtypes, the new subtype(s) replaces
    /// any existing subtypes from the appropriate set (creature types, land types, artifact
    /// types, enchantment types, planeswalker types, or spell types)." "Becomes a green Elk
    /// creature" makes it an Elk and no other creature type.
    ///
    /// [CR 205.1a]: https://yawgatog.com/resources/magic-rules/#R2051a
    SetSubtype(Subtype),
    /// Layer 4: "is a [basic land types]", setting a land's land types ([CR 305.7]): "If an
    /// effect sets a land's subtype to one or more of the basic land types, the land no longer
    /// has its old land type. It loses all abilities generated from its rules text, its old
    /// land types, and any copiable effects affecting that land, and it gains the appropriate mana
    /// ability for each new basic land type. Note that this doesn't remove any abilities that
    /// were granted to the land by other effects."
    ///
    /// [CR 305.7]: https://yawgatog.com/resources/magic-rules/#R3057
    SetLandTypes(BasicLandTypes),
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
    /// "Power is equal to [amount] and toughness is equal to [amount]": a characteristic-defining
    /// ability of the permanent itself that defines both, applied in layer 7a as
    /// [`Modification::SetPower`] is.
    SetPt(&'static Amount, &'static Amount),
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
    /// Layer 6: "gains protection from [quality]" ([CR 702.16a]): "Protection is a static
    /// ability, written “Protection from [quality].”"
    ///
    /// [CR 702.16a]: https://yawgatog.com/resources/magic-rules/#R70216a
    AddProtection(Protection),
}

impl Modification {
    /// Whether one of `modifications` asks a player to choose as its effect begins, which only
    /// a resolving spell or ability can ([`Protection::ChosenColor`]).
    pub const fn any_asks(modifications: &[Modification]) -> bool {
        let mut i = 0;
        while i < modifications.len() {
            if let Modification::AddProtection(Protection::ChosenColor) = modifications[i] {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// What a permanent has protection from: the quality of [CR 702.16a], "usually a color (as in
/// “protection from black”) but can be any characteristic value or information." Colors are
/// the only quality the engine has yet; a card type, a player or everything would each be a
/// variant here.
///
/// [CR 702.16a]: https://yawgatog.com/resources/magic-rules/#R70216a
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Protection {
    /// From each of these colors, as "protection from red" ([CR 702.16g]): "“Protection from
    /// [quality A] and from [quality B]” is shorthand for “protection from [quality A]” and
    /// “protection from [quality B]”".
    ///
    /// [CR 702.16g]: https://yawgatog.com/resources/magic-rules/#R70216g
    Colors(Colors),
    /// From the color the controller of the resolving spell or ability chooses as the effect
    /// begins, "protection from the color of your choice" ([CR 105.4]): "If a player is asked
    /// to choose a color, they must choose one of the five colors." Only a resolving effect
    /// can say it.
    ///
    /// [CR 105.4]: https://yawgatog.com/resources/magic-rules/#R1054
    ChosenColor,
}

/// The parts of one resolving continuous effect ([`crate::Effect::Modify`]) the engine begins
/// at once: as many as a card says ("loses all abilities and becomes a green Elk creature with
/// base power and toughness 3/3" is five) with room to spare.
pub const MODIFY_PARTS: usize = 8;

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
