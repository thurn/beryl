//! [`Malformed`]: what the definition check refuses, and how its build failure says it.

// Named only by the rustdoc links below.
#[cfg(doc)]
use crate::{
    ability::Ability,
    continuous::{MODIFY_PARTS, Modification},
    definition::Face,
    effect::{Amount, Condition, Effect, Objects, Players},
    filter::Filter,
};

/// What is wrong with a definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Malformed {
    /// [`Objects::Target`] or [`Players::Target`] of a target its ability does not have, or
    /// of the other kind: an object where the ability targets a player, or a player where it
    /// targets an object ([CR 115.1](https://yawgatog.com/resources/magic-rules/#R1151)).
    Target,
    /// [`Players::ThatPlayer`] outside an [`Effect::ForEachPlayer`] and the body (not the
    /// condition) of a replacement of draws: no player the instruction is being done for.
    ThatPlayer,
    /// [`Condition::Paid`] of a kind of cost its face has no [`crate::CastingOption`] for:
    /// it could never have been paid.
    Paid,
    /// [`Modification::Grant`] of an ability past the end of its face's [`Face::grants`].
    Grant,
    /// [`Modification::LoseAbility`] of an ability past the end of its face's abilities, or
    /// said by anything but a static ability of the permanent about itself: the index names
    /// an ability of the face it is printed on.
    Lost,
    /// A [`crate::PutOnto`] under the control of players that may be several ("each player",
    /// "each opponent"): a permanent has one controller.
    Under,
    /// A continuous effect that does nothing: no modification.
    NoModification,
    /// A resolving continuous effect of more parts than [`MODIFY_PARTS`].
    Parts,
    /// An [`Effect::CreateCopies`], an [`Effect::BecomeCopy`], an [`Effect::CopySpell`] or an enter-as-a-copy
    /// replacement naming a copy exception its face does not list ([`Face::excepts`]), or an
    /// exception giving an ability the face does not list in [`Face::grants`].
    Copy,
    /// [`Effect::AddMana`] outside a mana ability, whose activator chooses the kind
    /// ([CR 605.1a](https://yawgatog.com/resources/magic-rules/#R6051a)): any other ability
    /// names the kind it adds ([`Effect::AddManaOfKind`]).
    AddMana,
    /// [`Effect::CounterInto`] a zone other than exile, which is the one place the engine
    /// puts a countered spell instead of its owner's graveyard.
    Countered,
    /// [`Effect::Delay`] of an ability its face does not have, or of one that is not a
    /// delayed triggered ability ([`crate::Trigger::next`]): nothing would wait for it. Or
    /// one carrying a set named by what is in it ([`Objects::All`] and the like), where it
    /// carries one object.
    Delayed,
    /// [`Amount::Carried`] or [`Objects::Carried`] in an ability that is not a delayed
    /// triggered ability: no effect made it, so it carries nothing.
    Carried,
    /// [`Amount::SpellsCastBefore`] outside a "when you cast this spell" triggered ability,
    /// the one ability that keeps it as it triggers.
    CastBefore,
    /// [`Objects::TriggeredOn`] in an ability that is not triggered: no event was about anything.
    TriggeredOn,
    /// [`Effect::Choose`] with a prompt that is not a pick of one object, which is the one
    /// choice the interpreter asks; or a hidden pick that is not optional, or not among the
    /// cards of a hand or library each player has ([`Objects::InZoneOf`]), the one kind of
    /// candidates whose filter determinization deals a hidden pick's card from.
    Chosen,
    /// [`Objects::InZoneOf`] a zone the players share: each has a library, a hand and a graveyard
    /// of their own, and no other ([CR 400.1](https://yawgatog.com/resources/magic-rules/#R4001)).
    ZoneOf,
    /// [`Effect::PermitCast`] for a duration other than this turn or for as long as the card
    /// stays where it is, which are the two a permission lasts.
    Permitted,
    /// A cast offer names multiple players. Use [`Effect::ForEachPlayer`] to give each
    /// player a separate offer in turn order.
    CastPlayer,
    /// [`Effect::Unless`] of a cost the engine cannot take as an effect resolves: one other
    /// than mana alone or the discard of one card from a hand.
    Cost,
    /// [`Effect::Fateseal`] of a number of cards other than one, which is all the engine fateseals.
    Fateseal,
    /// More than one [`Effect::ChangeRule`] in an ability, which names one rule change to
    /// the game ([`Ability::rule`]): the others would have no number.
    Rules,
    /// What an object was, asked after an instruction of the same effect moved it, where the
    /// interpreter keeps no such answer
    /// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "the effect uses
    /// the object's last known information." It keeps a moved target's controller, power,
    /// toughness, mana value and counters, and who controlled each of [`Objects::Those`].
    /// So this is [`Condition::Is`], [`Condition::EnteredThisTurn`] or [`Condition::Token`]
    /// of a target, the ability's source, what that is attached to or what it triggered on,
    /// once moved, and the controller, the owner or a number of any of those but a target.
    LastKnown,
    /// [`Objects::Everywhere`] anywhere but a static ability's continuous effect with no "as
    /// long as", whose parts change only card types, subtypes and colors (layers 4 and 5) or
    /// grant escape. Those are what the kernel's query applies to an object off the
    /// battlefield, which has no layer results to hold any other change, and escape, which
    /// the casting rules ask of a card in a graveyard.
    Everywhere,
    /// [`Modification::AddChosenColor`] in a resolving effect: the chosen color is one the
    /// permanent with a static ability remembers, and a resolving effect has no such
    /// permanent.
    ChosenColor,
}

impl Malformed {
    /// What is wrong, as the build failure says it.
    pub const fn says(self) -> &'static str {
        match self {
            Malformed::Target => {
                "an effect names a target its ability does not have, or one of the other kind"
            }
            Malformed::ThatPlayer => "\"that player\" is said where no player is being done for",
            Malformed::Paid => "a condition asks about a cost its face cannot be cast with",
            Malformed::Grant => "a modification grants an ability its face does not list",
            Malformed::Lost => {
                "a modification removes an ability that is not one of its own permanent's"
            }
            Malformed::Under => {
                "a permanent is put onto the battlefield under players that may be several"
            }
            Malformed::NoModification => "a continuous effect modifies nothing",
            Malformed::Parts => {
                "a resolving continuous effect has more parts than the engine begins at once"
            }
            Malformed::Copy => {
                "a copy names an exception, or an exception an ability, its face lacks"
            }
            Malformed::AddMana => "an effect adds mana outside a mana ability",
            Malformed::Countered => "a countered spell is put somewhere other than exile",
            Malformed::Cost => "an effect names a cost the engine cannot take as it resolves",
            Malformed::Fateseal => "an effect fateseals a number of cards other than one",
            Malformed::Rules => "an ability's effect starts more than one rule change",
            Malformed::LastKnown => {
                "an expression asks what an object was after its effect moved it, which is not kept"
            }
            Malformed::Delayed => {
                "an effect delays an ability that is not a delayed trigger of its face, or carries a set"
            }
            Malformed::Carried => {
                "an expression is what a delayed trigger carries, and its ability is not one"
            }
            Malformed::CastBefore => {
                "an expression counts the spells cast before a spell, outside a trigger of casting it"
            }
            Malformed::TriggeredOn => {
                "an effect names what its trigger event was about, and its ability has none"
            }
            Malformed::Chosen => "a choice among objects is not a pick of one of them",
            Malformed::ZoneOf => "a zone of a player's own is not a library, hand or graveyard",
            Malformed::Permitted => {
                "a permission to cast lasts neither this turn nor while the card stays put"
            }
            Malformed::CastPlayer => "a cast offer names more than one player",
            Malformed::ChosenColor => {
                "a resolving effect adds a chosen color, which only a permanent has"
            }
            Malformed::Everywhere => {
                "an effect on objects in every zone is not a static ability's change of types or colors, or escape"
            }
        }
    }
}
