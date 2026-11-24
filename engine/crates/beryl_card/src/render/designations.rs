//! Class level bars, Case keywords and Saga chapter numerals.

use super::Ctx;
use super::words::{capitalized, keywords};
use crate::{Ability, Amount, Designation, Modification};

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

    /// A leveler's level symbol and what it gives, a line each (see CR 711.2a): the symbol
    /// as LEVEL 2-6 or LEVEL 7+, then the base power and toughness, then each ability.
    pub(super) fn level_symbol_paragraph(&mut self, ability: &Ability) -> Option<String> {
        let layer = ability.layer()?;
        let (from, to) = layer.level_band()?;
        let mut lines = vec![match to {
            Some(to) => format!("LEVEL {from}-{to}"),
            None => format!("LEVEL {from}+"),
        }];
        for &modification in layer.modifications {
            let line =
                if let Modification::SetBasePt(Amount::Fixed(p), Amount::Fixed(t)) = modification {
                    format!("{p}/{t}")
                } else if let Modification::AddKeywords(set) = modification {
                    capitalized(&keywords(set).join(", "))
                } else if let Modification::Grant(ix) = modification {
                    // A level symbol's abilities are printed bare, each its own paragraph.
                    match self.face.and_then(|face| face.grants.get(ix.index())) {
                        Some(granted) => self.paragraph(granted),
                        None => "[granted ability]".to_owned(),
                    }
                } else {
                    "[unsupported level symbol part]".to_owned()
                };
            lines.push(line);
        }
        Some(lines.join("\n"))
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
