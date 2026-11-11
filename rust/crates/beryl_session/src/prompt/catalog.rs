//! The catalog shape: one entry of a list, chosen and then confirmed.
//!
//! No card asks one yet. A decision of the shape offers a number for each entry
//! (`Opt::Number`, the entry's place in its catalog), and the prompt lists the entries in
//! the decision's order under their names, where the session knows the catalog's.

use beryl_engine::card::Purpose;

use super::held::{Clicked, Run, Step, button};
use super::{ButtonId, Click, PromptView, ShapeView};
use crate::protocol::{Candidate, DecisionView};

/// The colors by number, as the rules list them
/// ([CR 105.1](https://yawgatog.com/resources/magic-rules/#R1051): "There are five colors in
/// the Magic game: white, blue, black, red, and green").
const COLORS: [&str; 5] = ["White", "Blue", "Black", "Red", "Green"];

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
        let names: &[&str] = match purpose {
            Purpose::Color => &COLORS,
            // A catalog the session has no names for yet lists its entries by number: its
            // names come with the first card that asks it.
            _ => &[],
        };
        let entries: Vec<(String, u16)> = (decision.candidates.iter().enumerate())
            .filter_map(|(i, candidate)| {
                let Candidate::Number { n } = candidate.option else {
                    return None;
                };
                let name = names.get(usize::from(n)).copied();
                let name = name.unwrap_or(&candidate.label).to_owned();
                Some((name, u16::try_from(i).ok()?))
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
        view.state = Some(ShapeView::Catalog {
            entries: self.entries.iter().map(|(name, _)| name.clone()).collect(),
            chosen: self.chosen,
        });
    }
}
