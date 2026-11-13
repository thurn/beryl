//! The yes-or-no shape: a question with two answers, each a button, about the cards it shows.
//!
//! A click on a button answers; the prompt has no other state. The session says which answer
//! it puts forward. Where the answers are two places for one card, neither: the top of a
//! library or its bottom is the player's to weigh. Where yes pays life for a land to enter
//! untapped, the yes only when the land's mana would let them cast a card this turn that they
//! otherwise could not, by the engine's own check. Elsewhere the yes, as a "may" is most
//! often taken.

use beryl_engine::card::Purpose;
use beryl_engine::{Choice, Layout};

use super::ahead;
use super::held::{Clicked, Run, Step, Table, button};
use super::{ButtonId, Click, Marked, ObjectId, PlaceName, PromptButton, PromptView, ShapeBody};
use crate::protocol::{Candidate, DecisionView};
use crate::script::{Mark, Subject};

/// The state of a prompt of the yes-or-no shape.
#[derive(Clone, Debug)]
pub(crate) struct YesNo {
    purpose: Purpose,
    /// The cards the question is about.
    about: Vec<ObjectId>,
    /// Where the card the question is about goes on a yes, and on a no, if it goes anywhere.
    places: Option<[PlaceName; 2]>,
    buttons: Vec<PromptButton>,
    /// The permanents a yes that pays would tap, by the engine's default payment.
    taps: Vec<u16>,
}

impl YesNo {
    /// The state the yes or no for `purpose` of `decision`, the open decision on `table`,
    /// opens in.
    pub(crate) fn open<L: Layout>(
        table: &Table<'_, L>,
        purpose: Purpose,
        decision: &DecisionView,
    ) -> Option<YesNo> {
        let prompt = &decision.prompt;
        let index = |wanted: Candidate| {
            let at = (decision.candidates.iter()).position(|c| c.option == wanted)?;
            u16::try_from(at).ok()
        };
        let (yes, no) = (index(Candidate::Yes)?, index(Candidate::No)?);
        let [yes_place, no_place] = table.record.places.map(PlaceName::of);
        let places = yes_place.zip(no_place).map(|(yes, no)| [yes, no]);
        let primary = match purpose {
            _ if places.is_some() => None,
            Purpose::PayLife if !worth_paying(table, yes, no) => Some(no),
            _ => Some(yes),
        };
        let answer = |index: u16, words: &str| {
            let label = if words.is_empty() {
                &decision.candidates[usize::from(index)].label
            } else {
                words
            };
            button(
                ButtonId::Option { index },
                label,
                true,
                primary == Some(index),
            )
        };
        Some(YesNo {
            purpose,
            about: prompt.about.clone(),
            places,
            buttons: vec![answer(yes, &prompt.words.yes), answer(no, &prompt.words.no)],
            taps: (decision.candidates[usize::from(yes)].payments.first())
                .map(|payment| payment.taps.clone())
                .unwrap_or_default(),
        })
    }

    pub(crate) fn click(&self, click: Click) -> Clicked {
        match click {
            Click::Button {
                button: id @ ButtonId::Option { index },
            } if self.buttons.iter().any(|b| b.id == id) => {
                Clicked::Answers(Run::new(self.purpose, [Step::Index(index)]))
            }
            _ => Clicked::Same,
        }
    }

    /// Writes the state into `view`: what a yes would tap is marked as the payment the
    /// session suggests.
    pub(crate) fn show(&self, view: &mut PromptView) {
        view.marks = (self.taps.iter())
            .map(|&slot| Marked {
                on: Subject::Object { slot },
                mark: Mark::Suggested,
            })
            .collect();
        view.buttons.clone_from(&self.buttons);
        view.body = ShapeBody::YesNo {
            about: self.about.clone(),
            places: self.places,
        };
    }
}

/// Whether paying the life a land asks for, the answer at `yes` to the decision open on
/// `table`, lets its player cast more of the cards in their hand than the answer at `no`
/// does: the land's mana, untapped, pays for a card this turn
/// ([CR 614.1c](https://yawgatog.com/resources/magic-rules/#R6141c): "Effects that read
/// '[This permanent] enters with . . . ,' 'As [this permanent] enters . . . ,' or '[This
/// permanent] enters as . . . ' are replacement effects"; one that asks for life has the land
/// enter tapped unless it is paid). Each answer is played on a copy of the game to the next
/// thing asked, and the engine says what may be cast there.
fn worth_paying<L: Layout>(table: &Table<'_, L>, yes: u16, no: u16) -> bool {
    let Some(d) = table.d else {
        return false;
    };
    let castable = |index: u16| {
        let mut copy = ahead::forced(table.s);
        ahead::answer(table, &mut copy, Choice(index));
        ahead::castable(table, d.player, &copy)
    };
    castable(yes) > castable(no)
}
