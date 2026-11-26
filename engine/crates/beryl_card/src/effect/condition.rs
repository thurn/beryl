//! Conditions: whether an instruction is carried out, or an ability triggers.

use crate::cost::CastingKind;
use crate::effect::{Amount, Objects, Players};
use crate::filter::Filter;

/// Something true or false of the game: the "if" of an [`crate::Effect::If`], or the intervening
/// "if" of a triggered ability ([CR 603.4]): "When the trigger event occurs, the ability checks
/// whether the stated condition is true. The ability triggers only if it is; otherwise it does
/// nothing. If the ability triggers, it checks the stated condition again as it resolves."
///
/// [CR 603.4]: https://yawgatog.com/resources/magic-rules/#R6034
#[derive(Clone, Copy, Debug)]
pub enum Condition {
    /// "If you do", "if they do", "if a card was discarded this way": the instruction before
    /// did what it says (the [module](crate::effect) says which instruction that is).
    Did,
    /// "If it was kicked", "if its evoke cost was paid": the spell was cast with this kind of
    /// alternative or additional cost, or the permanent's spell was ([CR 702.33d]): "If a spell's
    /// controller declares the intention to pay any of that spell's kicker costs, that spell has
    /// been 'kicked.'"
    ///
    /// [CR 702.33d]: https://yawgatog.com/resources/magic-rules/#R70233d
    Paid(CastingKind),
    /// One amount against another: "if you control three or more artifacts", "if it has a
    /// time counter on it".
    Compare(Amount, Comparison, Amount),
    /// "If it's a [filter]": there is such an object and every one passes the filter, read
    /// for the ability's controller.
    Is(Objects, Filter),
    /// "If it entered this turn", "as long as this entered this turn": there is such an
    /// object, and every one is a permanent that entered the battlefield this turn.
    EnteredThisTurn(Objects),
    /// "If it's on the battlefield": there is such an object, on the battlefield. Of the
    /// ability's source, it is still the object that it was when the ability triggered: one
    /// that left and came back is a new object
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)).
    OnBattlefield(Objects),
    /// "If it's a token", and with [`Condition::Not`], "if it isn't a token": there is such an
    /// object, and every one is a token ([CR 111.1]): "Some effects put tokens onto the
    /// battlefield." Of the permanent whose leaving the battlefield triggered the ability, as it
    /// last was there ([CR 603.10a]): "Some zone-change triggers look back in time."
    ///
    /// [CR 111.1]: https://yawgatog.com/resources/magic-rules/#R1111
    /// [CR 603.10a]: https://yawgatog.com/resources/magic-rules/#R60310a
    Token(Objects),
    /// "If you win the flip", and with [`Condition::Not`], "if you lose the flip": the instruction
    /// before was a coin flip ([`crate::Effect::FlipCoin`]) and the player won it ([CR 705.2]): "If
    /// the call matches the result, the player wins the flip. Otherwise, the player loses the
    /// flip."
    ///
    /// [CR 705.2]: https://yawgatog.com/resources/magic-rules/#R7052
    WonFlip,
    /// "If [player] has drawn the first card of their draw step this turn": there is such a player,
    /// and every one has drawn a card in their own draw step this turn. It tells the first card a
    /// player draws in their draw step ([CR 504.1]): "First, the active player draws a card." from
    /// every later one. Kept only in a game with a card that reads it (`beryl_kernel`'s `Count`).
    ///
    /// [CR 504.1]: https://yawgatog.com/resources/magic-rules/#R5041
    DrewInDrawStep(Players),
    /// "If it's your turn", "if it's [player]'s turn": the active player ([CR 102.1]: "The
    /// active player is the player whose turn it is.") is one of `players`.
    ///
    /// [CR 102.1]: https://yawgatog.com/resources/magic-rules/#R1021
    TurnOf(Players),
    /// "If you attacked this turn", "unless you attacked this turn": there is such a player,
    /// and every one declared at least one creature as an attacker this turn. A creature put
    /// onto the battlefield attacking never "attacked" for effects ([CR 508.4]): "for the
    /// purposes of trigger events and effects, ... never 'attacked.'" Kept only in a game with
    /// a card that reads it (`beryl_kernel`'s `Count`).
    ///
    /// [CR 508.4]: https://yawgatog.com/resources/magic-rules/#R5084
    AttackedThisTurn(Players),
    /// "If you cast it": there is such an object, and every one is a permanent that was a
    /// spell its controller cast, which became that permanent as it resolved. A copy of a
    /// spell was not cast ([CR 707.10]): "a copy of a spell isn't cast". A permanent that was
    /// put onto the battlefield, or that left and came back, is a new object that was not
    /// (see [CR 400.7]). Kept only in a game with a card that asks (`beryl_kernel`'s
    /// `Memo::Cast`).
    ///
    /// [CR 707.10]: https://yawgatog.com/resources/magic-rules/#R70710
    /// [CR 400.7]: https://yawgatog.com/resources/magic-rules/#R4007
    Cast(Objects),
    /// "If you both own and control this and a [filter] named [its partner]": the ability's
    /// controller owns and controls this permanent and a permanent the filter accepts that has
    /// the name of the other card of this card's meld pair
    /// ([`crate::definition::Shape::Meld`]). It is the condition of the ability that melds them
    /// ([CR 701.42a]): "Meld is a keyword action that appears in an ability on one card in a meld
    /// pair."
    ///
    /// [CR 701.42a]: https://yawgatog.com/resources/magic-rules/#R70142a
    MeldPair(Filter),
    /// "If a card with the chosen name was milled this way": at least one of the objects has
    /// the card name an [`crate::Effect::ChooseValue`] earlier in the same resolution chose
    /// ([CR 201.2a](https://yawgatog.com/resources/magic-rules/#R2012a)): "Two or more objects
    /// have the same name if they have at least one name in common". A milled card is found
    /// wherever it went, as long as that zone is public ([CR 701.17c]): "An effect that refers
    /// to a milled card can find that card in the zone it moved to from the library, as long
    /// as that zone is a public zone." With no name chosen, or "another card" chosen, no object
    /// has it.
    ///
    /// [CR 701.17c]: https://yawgatog.com/resources/magic-rules/#R70117c
    HasChosenName(Objects),
    Not(&'static Condition),
    /// Every one of them: "if ... and ...".
    All(&'static [Condition]),
    /// At least one of them: "if ... or ...".
    Any(&'static [Condition]),
}

impl Condition {
    /// Whether this means something only a resolving spell or ability has
    /// ([`Objects::needs_a_resolution`]): whether an instruction before did anything, or an
    /// expression of it that does. A cost paid is none: a permanent remembers what its spell
    /// paid.
    pub const fn needs_a_resolution(&self) -> bool {
        match self {
            Condition::Did | Condition::WonFlip | Condition::HasChosenName(_) => true,
            Condition::Paid(_) => false,
            Condition::MeldPair(partner) => partner.needs_a_resolution(),
            Condition::DrewInDrawStep(players)
            | Condition::TurnOf(players)
            | Condition::AttackedThisTurn(players) => players.needs_a_resolution(),
            Condition::Compare(left, _, right) => {
                left.needs_a_resolution() || right.needs_a_resolution()
            }
            Condition::Is(objects, filter) => {
                objects.needs_a_resolution() || filter.needs_a_resolution()
            }
            Condition::EnteredThisTurn(objects)
            | Condition::Token(objects)
            | Condition::OnBattlefield(objects)
            | Condition::Cast(objects) => objects.needs_a_resolution(),
            Condition::Not(condition) => condition.needs_a_resolution(),
            Condition::All(conditions) | Condition::Any(conditions) => {
                let mut i = 0;
                while i < conditions.len() {
                    if conditions[i].needs_a_resolution() {
                        return true;
                    }
                    i += 1;
                }
                false
            }
        }
    }
}

impl Condition {
    /// Whether it reads what a cost took ([`Objects::Paid`]).
    pub const fn names_paid(&self) -> bool {
        match self {
            Condition::Did | Condition::WonFlip | Condition::Paid(_) | Condition::MeldPair(_) => {
                false
            }
            Condition::DrewInDrawStep(players)
            | Condition::TurnOf(players)
            | Condition::AttackedThisTurn(players) => players.names_paid(),
            Condition::Compare(left, _, right) => left.names_paid() || right.names_paid(),
            Condition::Is(objects, _)
            | Condition::EnteredThisTurn(objects)
            | Condition::Token(objects)
            | Condition::OnBattlefield(objects)
            | Condition::Cast(objects)
            | Condition::HasChosenName(objects) => objects.names_paid(),
            Condition::Not(condition) => condition.names_paid(),
            Condition::All(conditions) | Condition::Any(conditions) => {
                let mut i = 0;
                while i < conditions.len() {
                    if conditions[i].names_paid() {
                        return true;
                    }
                    i += 1;
                }
                false
            }
        }
    }
}

impl Condition {
    /// Whether `test` holds of this condition or of one it is made of.
    pub fn any(&self, test: &impl Fn(&Condition) -> bool) -> bool {
        test(self)
            || match *self {
                Condition::Not(condition) => condition.any(test),
                Condition::All(conditions) | Condition::Any(conditions) => {
                    conditions.iter().any(|condition| condition.any(test))
                }
                Condition::Did
                | Condition::Paid(_)
                | Condition::Compare(..)
                | Condition::Is(..)
                | Condition::EnteredThisTurn(_)
                | Condition::OnBattlefield(_)
                | Condition::Token(_)
                | Condition::Cast(_)
                | Condition::HasChosenName(_)
                | Condition::WonFlip
                | Condition::MeldPair(_)
                | Condition::DrewInDrawStep(_)
                | Condition::TurnOf(_)
                | Condition::AttackedThisTurn(_) => false,
            }
    }
}

/// How one [`Amount`] is held against another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Comparison {
    /// "N or more", "at least", "greater than or equal to".
    AtLeast,
    /// "N or fewer", "N or less", "at most".
    AtMost,
    /// "Exactly", "equal to".
    Equal,
    /// "More than", "greater than".
    MoreThan,
    /// "Fewer than", "less than".
    LessThan,
}

impl Comparison {
    /// Whether `left` is this to `right`: 3 is [`Comparison::AtLeast`] 2. The amounts are as
    /// wide as any the game counts, a life total among them.
    #[inline]
    pub const fn holds(self, left: i32, right: i32) -> bool {
        match self {
            Comparison::AtLeast => left >= right,
            Comparison::AtMost => left <= right,
            Comparison::Equal => left == right,
            Comparison::MoreThan => left > right,
            Comparison::LessThan => left < right,
        }
    }
}
