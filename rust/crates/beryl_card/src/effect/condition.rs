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
            Condition::Did | Condition::WonFlip => true,
            Condition::Paid(_) => false,
            Condition::DrewInDrawStep(players) => players.needs_a_resolution(),
            Condition::Compare(left, _, right) => {
                left.needs_a_resolution() || right.needs_a_resolution()
            }
            Condition::Is(objects, filter) => {
                objects.needs_a_resolution() || filter.needs_a_resolution()
            }
            Condition::EnteredThisTurn(objects) | Condition::Token(objects) => {
                objects.needs_a_resolution()
            }
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
            Condition::Did | Condition::WonFlip | Condition::Paid(_) => false,
            Condition::DrewInDrawStep(players) => players.names_paid(),
            Condition::Compare(left, _, right) => left.names_paid() || right.names_paid(),
            Condition::Is(objects, _)
            | Condition::EnteredThisTurn(objects)
            | Condition::Token(objects) => objects.names_paid(),
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
                _ => false,
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
