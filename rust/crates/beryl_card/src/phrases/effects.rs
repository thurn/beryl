use crate::effect::build::{
    counter, discard, draw, gain_life, its_controller, target, unless_pays, you,
};
use crate::{Effect, ManaCost};

/// "Draw a card."
pub const DRAW_A_CARD: Effect = draw(you(), 1);

/// "Draw three cards."
pub const DRAW_THREE_CARDS: Effect = draw(you(), 3);

/// "Draw seven cards."
pub const DRAW_SEVEN_CARDS: Effect = draw(you(), 7);

/// "Discard a card."
pub const DISCARD_A_CARD: Effect = discard(you(), 1);

/// "You gain 2 life."
pub const GAIN_TWO_LIFE: Effect = gain_life(you(), 2);

/// "Counter target spell."
/// Use with `TargetSpec::spell()` as the first target.
pub const COUNTER_TARGET_SPELL: Effect = counter(target());

/// "{effect} unless its controller pays {mana}."
/// The first target's controller pays. The effect must name that target.
pub const fn unless_its_controller_pays(effect: &'static Effect, mana: ManaCost) -> Effect {
    unless_pays(effect, its_controller(), mana)
}
