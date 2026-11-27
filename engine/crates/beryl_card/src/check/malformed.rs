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
    /// A single-faced card or meld component defines a different number of faces.
    ExpectedOneFace,
    /// A card with an alternate set of printed characteristics lacks exactly two faces.
    ExpectedTwoFaces,
    /// A split card with fuse must define its combined spell after its printed halves.
    ExpectedFusedFace,
    /// A face's generated use does not agree with its definition.
    FaceUse,
    /// [`Objects::Target`] or [`Players::Target`] of a target its ability does not have, or
    /// of the other kind: an object where the ability targets a player, or a player where it
    /// targets an object ([CR 115.1](https://yawgatog.com/resources/magic-rules/#R1151)).
    Target,
    /// [`Players::ThatPlayer`] outside an [`Effect::ForEachPlayer`] and the body (not the
    /// condition) of a replacement of draws, or a cast or discard trigger: no player the
    /// instruction is being done for.
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
    /// [`Effect::AddMana`] or [`Effect::AddProduct`] outside a mana ability, whose activator
    /// chooses the kind
    /// ([CR 605.1a](https://yawgatog.com/resources/magic-rules/#R6051a)): any other ability
    /// names the kind it adds ([`Effect::AddManaOfKind`]).
    AddMana,
    /// [`Effect::EndTurn`] in a replacement ability, where no spell or ability is resolving.
    EndTurn,
    /// [`Effect::RestartGame`] or [`Effect::Subgame`] outside a spell or nonmana ability.
    NewGame,
    /// [`Effect::CounterInto`] a zone other than exile or the top of its owner's library, which
    /// are the places the engine puts a countered spell instead of its owner's graveyard.
    Countered,
    /// [`Effect::Delay`] of an ability its face does not have, or of one that is not a
    /// delayed triggered ability ([`crate::Trigger::next`]): nothing would wait for it. Or
    /// one carrying a set named by what is in it ([`Objects::All`] and the like), where it
    /// carries one object. Or [`Effect::Reflexive`] of an ability that is not written with
    /// [`crate::Trigger::REFLEXIVE`], or an [`Effect::Delay`] of one that is.
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
    /// A mana-value limit requires a cast without paying its mana cost.
    CastLimit,
    /// Exiling from the top until a match only reads one library.
    ExilePlayer,
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
    /// What is wrong and what to change in the card file, as the build failure says it. A
    /// card writer reads it with the card file open, so it names the builder, the entry or
    /// the field to change.
    #[allow(clippy::too_many_lines)] // One exhaustive match, a message per variant.
    pub const fn says(self) -> &'static str {
        match self {
            Malformed::ExpectedOneFace => {
                "this card's shape takes exactly one `Face`: remove the extra faces from `card(...)`, or pass the card's printed shape to `card_with_shape`"
            }
            Malformed::ExpectedTwoFaces => {
                "this card's shape takes two faces: list the back face after the front face in `card_with_shape`"
            }
            Malformed::ExpectedFusedFace => {
                "a split card with fuse takes three faces: its two halves, then `Face::fused(...)` for the combined spell"
            }
            Malformed::FaceUse => {
                "the faces are out of order: list the card's generated printed faces front first, then any `Face::prototyped()` or `Face::fused(...)` face"
            }
            Malformed::Target => {
                "an `Objects::Target(n)` or `Players::Target(n)` has no matching target: give the ability a `TargetSpec` at index n in `.targeting(...)`, of a player kind for `Players::Target` and an object kind for `Objects::Target`, with `count` at least the number required; an ability target's filter is `Filter::Any`, and damage divided between targets needs one `TargetSpec` of count two"
            }
            Malformed::ThatPlayer => {
                "`Players::ThatPlayer` names no player here: use it only inside `for_each_player(...)`, in the effect of a replacement of draws or in a cast or discard trigger, and otherwise name the player (`you()`, `Players::Target(n)`, `Players::ControllerOf(...)`)"
            }
            Malformed::Paid => {
                "`Condition::Paid` asks about a cost the face cannot be cast with: add the matching `CastingOption` to the face's `.casting(...)`, or remove the condition"
            }
            Malformed::Grant => {
                "`Modification::Grant(ix)` points past the face's `.grants(...)`: add the granted ability to `.grants(...)`, or fix the index"
            }
            Malformed::Lost => {
                "`Modification::LoseAbility(ix)` belongs in a static ability whose `LayerSpec` affects `Objects::This`, and its index must name one of the face's own `.abilities(...)`"
            }
            Malformed::Under => {
                "`PutOnto::under` names players that may be several: name one player, or wrap the instruction in `for_each_player(...)` and put it under `Players::ThatPlayer`"
            }
            Malformed::NoModification => {
                "a `LayerSpec` or an `Effect::Modify` lists no `Modification`: add what it changes, or remove it"
            }
            Malformed::Parts => {
                "an `Effect::Modify` lists more modifications than `MODIFY_PARTS`: split it into two `Effect::Modify` instructions in a `sequence(...)`"
            }
            Malformed::Copy => {
                "a copy names a `CopyException` past the face's `.excepts(...)`, or a `CopyChange::Has(ix)` names an ability past its `.grants(...)`: add the exception or the ability to the face, or fix the index"
            }
            Malformed::AddMana => {
                "`Effect::AddMana` and `Effect::AddProduct` let the activator choose the kind, which only a mana ability (`Ability::mana`, `Ability::mana_of`) can do: in any other spell or ability, use `Effect::AddManaOfKind` with the kind it adds"
            }
            Malformed::NewGame => {
                "`Effect::RestartGame` and `Effect::Subgame` belong in a spell or an activated or triggered ability: move them out of the replacement or mana ability"
            }
            Malformed::EndTurn => {
                "`Effect::EndTurn` cannot be in a replacement ability: move it to a spell, or an activated or triggered ability"
            }
            Malformed::Countered => {
                "`Effect::CounterInto` takes only `Zone::Exile` or `Zone::Library`: escalate any other zone in `data/card-escalations.json`"
            }
            Malformed::Cost => {
                "an `Effect::Unless` cost must be mana alone or the discard of one card from hand: escalate any other cost in `data/card-escalations.json`"
            }
            Malformed::Fateseal => {
                "`Effect::Fateseal` takes only `Amount::Fixed(1)`: escalate any other count in `data/card-escalations.json`"
            }
            Malformed::Rules => {
                "an ability's effect starts more than one `Effect::ChangeRule`: give each rule change an ability of its own, or escalate the card in `data/card-escalations.json`"
            }
            Malformed::LastKnown => {
                "a condition or amount reads an object that an earlier instruction of the same effect moved: read it before the move, or read it of a target or `Objects::Those`, whose controller and numbers are kept"
            }
            Malformed::Delayed => {
                "`Effect::Delay` or `Effect::ExtraTurn` must name, by index, one of the face's own abilities whose trigger is delayed (`Trigger::next()`; for an extra turn, a step trigger of that turn), and a delay carries one object, not a set such as `Objects::All`. `Effect::Reflexive` must name one whose trigger is `Trigger::REFLEXIVE`, which no `Effect::Delay` may name"
            }
            Malformed::Carried => {
                "`Objects::Carried` and `Amount::Carried` work only in a delayed triggered ability (`Trigger::next()`), and `Amount::Carried` also in a deals-damage trigger: name the object or the amount directly here"
            }
            Malformed::CastBefore => {
                "`Amount::SpellsCastBefore` works only in a \"when you cast this spell\" trigger (`TriggerEvent::Casts` with `Subject::This`): move the instruction there"
            }
            Malformed::TriggeredOn => {
                "`Objects::TriggeredOn` needs a triggered ability, and `ExileMatch::SharesTypeWithTriggeringSpell` a cast trigger: move the instruction into one"
            }
            Malformed::Chosen => {
                "`Effect::Choose` must pick exactly one object (a pick prompt with `most` 1), and a hidden pick must be optional and among the cards of a hand or library (`Objects::InZoneOf`)"
            }
            Malformed::ZoneOf => {
                "`Objects::InZoneOf` takes only `Zone::Battlefield`, `Zone::Library`, `Zone::Hand` or `Zone::Graveyard`: for exile or the stack, use `Objects::All`"
            }
            Malformed::Permitted => {
                "`Effect::PermitCast` takes `Duration::EndOfTurn`, or `Duration::Indefinite` for as long as the card stays exiled: change the duration, or escalate the card in `data/card-escalations.json`"
            }
            Malformed::CastPlayer => {
                "`Effect::MayCast` names one player: wrap it in `for_each_player(...)` with `Players::ThatPlayer` to give each player an offer in turn order"
            }
            Malformed::CastLimit => {
                "an `Effect::MayCast` with `free_below` must also set `free: true`"
            }
            Malformed::ExilePlayer => {
                "`Effect::ExileUntil` reads one library: name one player, or wrap it in `for_each_player(...)` with `Players::ThatPlayer`"
            }
            Malformed::ChosenColor => {
                "`Modification::AddChosenColor` works only in a static ability: in a resolving `Effect::Modify`, name the color with `Modification::AddColorWords`"
            }
            Malformed::Everywhere => {
                "`Objects::Everywhere` works only as what a static ability's `LayerSpec` affects, with no condition and with modifications that change only types, subtypes or colors, or grant escape: elsewhere, use `Objects::All` with a zone"
            }
        }
    }
}
