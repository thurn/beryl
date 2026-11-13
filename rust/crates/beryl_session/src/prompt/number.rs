//! The number shape: one of the numbers offered, set and then confirmed.
//!
//! Its one purpose is the value of X
//! ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b): "If the spell has a
//! variable cost that will be paid as it's being cast (such as an {X} in its mana cost; see
//! rule 107.3), the player announces the value of that variable"). The prompt opens at the
//! least value, says the value under its button, and says what each makes the spell cost
//! ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f): "The total cost is the
//! mana cost or alternative cost (as determined in rule 601.2b), plus all additional costs and
//! cost increases, and minus all cost reductions"), with what the engine's default payment of
//! it would tap marked. Its answer continues the cast it is announced for.

use beryl_engine::card::Purpose;

use super::held::{Clicked, Run, Step};
use super::wording::X_VALUE;
use super::{ButtonId, Click, Marked, PromptButton, PromptView, ShapeBody};
use crate::protocol::{Candidate, DecisionView};
use crate::script::{Mark, Subject};

/// The state of a prompt of the number shape.
#[derive(Clone, Debug)]
pub(crate) struct Number {
    purpose: Purpose,
    /// The numbers offered, ascending.
    values: Vec<Value>,
    /// The one set, by its place among `values`.
    at: usize,
    confirm: String,
}

/// A number the decision offers.
#[derive(Clone, Debug)]
struct Value {
    n: u8,
    /// Its candidate.
    index: u16,
    /// What it means, where it means more than itself: the total cost an X makes.
    detail: Option<String>,
    /// The permanents the engine's default payment at this value would tap.
    taps: Vec<u16>,
}

impl Number {
    /// The state the number prompt for `purpose` of `decision` opens in.
    pub(crate) fn open(purpose: Purpose, decision: &DecisionView) -> Option<Number> {
        let mut values: Vec<Value> = (decision.candidates.iter().enumerate())
            .filter_map(|(i, candidate)| {
                let Candidate::Number { n } = candidate.option else {
                    return None;
                };
                let payment = candidate.payments.first();
                Some(Value {
                    n,
                    index: u16::try_from(i).ok()?,
                    detail: payment.and_then(|payment| payment.cost.clone()),
                    taps: payment.map(|p| p.taps.clone()).unwrap_or_default(),
                })
            })
            .collect();
        values.sort_by_key(|value| value.n);
        (!values.is_empty()).then(|| Number {
            purpose,
            values,
            at: 0,
            confirm: decision.prompt.words.confirm.clone(),
        })
    }

    pub(crate) fn click(&mut self, click: Click) -> Clicked {
        match click {
            Click::Number { n, on: None } => {
                match self.values.iter().position(|value| value.n == n) {
                    Some(at) if at != self.at => {
                        self.at = at;
                        Clicked::Changed
                    }
                    _ => Clicked::Same,
                }
            }
            Click::Button {
                button: ButtonId::Confirm,
            } => {
                let index = self.values[self.at].index;
                Clicked::Answers(Run::new(self.purpose, [Step::Index(index)]).continuing())
            }
            _ => Clicked::Same,
        }
    }

    pub(crate) fn show(&self, view: &mut PromptView) {
        let value = &self.values[self.at];
        view.marks = (value.taps.iter())
            .map(|&slot| Marked {
                on: Subject::Object { slot },
                mark: Mark::Suggested,
            })
            .collect();
        // The button says the value it answers with under its label.
        view.buttons = vec![PromptButton {
            id: ButtonId::Confirm,
            label: self.confirm.clone(),
            caption: X_VALUE.replace("{n}", &value.n.to_string()),
            enabled: true,
            primary: true,
        }];
        view.body = ShapeBody::Number {
            values: self.values.iter().map(|value| value.n).collect(),
            value: value.n,
            detail: value.detail.clone(),
        };
    }
}
