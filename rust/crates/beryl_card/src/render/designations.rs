//! Class level bars, Case keywords and Saga chapter numerals.

use super::Ctx;
use super::words::capitalized;
use crate::{Ability, Designation};

impl Ctx {
    pub(super) fn designation_paragraph(&mut self, ability: &Ability) -> Option<String> {
        match ability.designation() {
            Designation::LevelUp(level) => {
                Some(format!("{}: Level {level}", self.cost_text(ability.cost())))
            }
            Designation::ToSolve => {
                let condition = ability
                    .condition()
                    .expect("a Case solves under a condition");
                Some(format!(
                    "To solve — {}.",
                    capitalized(&self.condition(*condition))
                ))
            }
            Designation::None
            | Designation::AtLevel(_)
            | Designation::Solved
            | Designation::Imprint => None,
        }
    }
}

/// Chapter symbols use Roman numerals (see [CR 715.2a]).
///
/// [CR 715.2a]: https://yawgatog.com/resources/magic-rules/#R7152a
pub(super) fn roman(number: u8) -> String {
    let mut number = number;
    let mut text = String::new();
    for (value, symbol) in [
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ] {
        while number >= value {
            text.push_str(symbol);
            number -= value;
        }
    }
    text
}
