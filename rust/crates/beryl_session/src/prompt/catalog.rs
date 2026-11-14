//! The catalog shape: one entry of a list, chosen and then confirmed.
//!
//! A decision of the shape offers a number for each entry (`Opt::Number`, the entry's number
//! in its catalog), and the prompt lists the entries in the decision's order under the names
//! its candidates are labeled with ("Red", "Goblin", "Another card").

use beryl_engine::card::Purpose;

use super::held::{Clicked, Run, Step, button};
use super::{ButtonId, Click, PromptView, ShapeBody};
use crate::protocol::{Candidate, DecisionView};

/// The state of a prompt of the catalog shape.
#[derive(Clone, Debug)]
pub(crate) struct Catalog {
    purpose: Purpose,
    /// Each entry's name and its candidate.
    entries: Vec<(String, u16)>,
    /// The entry chosen, by its place among `entries`.
    chosen: Option<u16>,
    /// The confirm button's words, `{picked}` still to fill.
    confirm: String,
}

impl Catalog {
    /// The state the catalog prompt for `purpose` of `decision` opens in: nothing chosen.
    pub(crate) fn open(purpose: Purpose, decision: &DecisionView) -> Option<Catalog> {
        let entries: Vec<(String, u16)> = (decision.candidates.iter().enumerate())
            .filter_map(|(i, candidate)| {
                let Candidate::Number { .. } = candidate.option else {
                    return None;
                };
                Some((candidate.label.clone(), u16::try_from(i).ok()?))
            })
            .collect();
        (!entries.is_empty()).then(|| Catalog {
            purpose,
            entries,
            chosen: None,
            confirm: decision.prompt.words.confirm.clone(),
        })
    }

    pub(crate) fn click(&mut self, click: Click) -> Clicked {
        match (click, self.chosen) {
            (Click::Entry { index }, chosen)
                if usize::from(index) < self.entries.len() && chosen != Some(index) =>
            {
                self.chosen = Some(index);
                Clicked::Changed
            }
            (
                Click::Button {
                    button: ButtonId::Confirm,
                },
                Some(chosen),
            ) => {
                let (_, candidate) = self.entries[usize::from(chosen)];
                Clicked::Answers(Run::new(self.purpose, [Step::Index(candidate)]))
            }
            _ => Clicked::Same,
        }
    }

    /// Writes the state into `view`. Nothing is marked: the entries are not on the table.
    pub(crate) fn show(&self, view: &mut PromptView) {
        let chosen = self.chosen.is_some();
        let label = (self.confirm).replace("{picked}", &u8::from(chosen).to_string());
        view.marks = Vec::new();
        view.buttons = vec![button(ButtonId::Confirm, &label, chosen, chosen)];
        view.body = ShapeBody::Catalog {
            entries: self.entries.iter().map(|(name, _)| name.clone()).collect(),
            chosen: self.chosen,
        };
    }
}
