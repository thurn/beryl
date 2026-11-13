//! The mode shape: one of a few answers, each a button, about the cards it shows.
//!
//! A decision asks three: who plays first, whether to keep a hand, and whether to cast a card
//! a resolved spell or ability offers. Each of the decision's candidates is a button, and a
//! click on one answers.
//!
//! The fourth is the session's own ([`Mode::act`]): the ways to use a card clicked at a
//! priority, when it has several
//! ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b): "If the spell has
//! alternative or additional costs that will be paid as it's being cast ... the player
//! announces their intentions to pay any or all of those costs"; the engine offers each as a
//! candidate of the priority, architecture §7.2). It lies over the priority's own prompt: a
//! button answers the priority with that way, and its cancel button goes back to the
//! priority.

use beryl_engine::card::Purpose;

use super::held::{Clicked, Run, Step, button};
use super::{ButtonId, Click, PromptButton, PromptView, ShapeBody, ids, slots_of};
use crate::protocol::{Candidate, DecisionView};

/// The state of a prompt of the mode shape.
#[derive(Clone, Debug)]
pub(crate) struct Mode {
    /// The purpose of the decision a button answers: the prompt's own, or the priority's
    /// under a choice of ways.
    answers: Purpose,
    /// The cards the choice is about.
    about: Vec<u16>,
    buttons: Vec<PromptButton>,
}

/// A way to use a card, on the choice among them.
#[derive(Clone, Debug)]
pub(crate) enum Way {
    /// A candidate of the priority, by index.
    Offered(u16),
    /// An activated ability of the card that the priority does not offer, as a planeswalker's
    /// loyalty ability that would take more loyalty than it has
    /// ([CR 606.6](https://yawgatog.com/resources/magic-rules/#R6066): "A loyalty ability
    /// with a negative loyalty cost ... can't be activated unless the permanent has at least
    /// that many loyalty counters on it"), by its
    /// [`CandidateView::nth`](crate::protocol::CandidateView::nth) and what an activation of
    /// it is called.
    Unusable { nth: u8, label: String },
}

/// What a click on a mode prompt did.
pub(crate) enum Chosen {
    /// Nothing.
    Same,
    /// It answers with these picks.
    Answers(Run),
    /// It closes the prompt, where it lies over another.
    Closes,
}

impl Mode {
    /// The state the mode prompt for `purpose` of `decision` opens in: a button for each
    /// candidate, in the decision's words where the table has them (the pass that declines an
    /// offer, the one cast it offers) and the candidate's own otherwise. The first that is
    /// not a pass is put forward.
    pub(crate) fn open(purpose: Purpose, decision: &DecisionView) -> Mode {
        let words = &decision.prompt.words;
        let cast = |option: &Candidate| matches!(option, Candidate::Cast { .. });
        let casts = (decision.candidates.iter())
            .filter(|c| cast(&c.option))
            .count();
        let first = (decision.candidates.iter()).position(|c| c.option != Candidate::Pass);
        let buttons = (decision.candidates.iter().enumerate())
            .map(|(i, candidate)| {
                let label = match &candidate.option {
                    Candidate::Pass if !words.decline.is_empty() => &words.decline,
                    option if cast(option) && casts == 1 && !words.confirm.is_empty() => {
                        &words.confirm
                    }
                    _ => &candidate.label,
                };
                let index = u16::try_from(i).expect("at most 64 candidates");
                button(ButtonId::Option { index }, label, true, first == Some(i))
            })
            .collect();
        Mode {
            answers: purpose,
            about: slots_of(&decision.prompt.about),
            buttons,
        }
    }

    /// The choice among `ways` to use the card in `slot`, at a priority whose candidates are
    /// called `labels`: a button for each, under its candidate's label or, for an ability the
    /// priority does not offer, its own and never enabled; and `cancel` to go back.
    pub(crate) fn act(labels: &[String], slot: u16, ways: &[Way], cancel: &str) -> Mode {
        let way = |way: &Way| match *way {
            Way::Offered(index) => {
                let label = &labels[usize::from(index)];
                button(ButtonId::Option { index }, label, true, false)
            }
            Way::Unusable { nth, ref label } => {
                button(ButtonId::Ability { nth }, label, false, false)
            }
        };
        Mode {
            answers: Purpose::Priority,
            about: vec![slot],
            buttons: (ways.iter().map(way))
                .chain([button(ButtonId::Decline, cancel, true, false)])
                .collect(),
        }
    }

    pub(crate) fn click(&self, click: Click) -> Chosen {
        let Click::Button { button: id } = click else {
            return Chosen::Same;
        };
        if !self.buttons.iter().any(|b| b.id == id) {
            return Chosen::Same;
        }
        match id {
            ButtonId::Option { index } => {
                Chosen::Answers(Run::new(self.answers, [Step::Index(index)]))
            }
            ButtonId::Decline => Chosen::Closes,
            _ => Chosen::Same,
        }
    }

    /// What a click did to a mode prompt that lies over no other.
    pub(crate) fn clicked(&self, click: Click) -> Clicked {
        match self.click(click) {
            Chosen::Answers(run) => Clicked::Answers(run),
            Chosen::Same | Chosen::Closes => Clicked::Same,
        }
    }

    /// Writes the state into `view`. Nothing is marked: the answers are buttons.
    pub(crate) fn show(&self, view: &mut PromptView) {
        view.marks = Vec::new();
        view.buttons.clone_from(&self.buttons);
        view.body = ShapeBody::Mode {
            about: ids(self.about.iter().copied()),
        };
    }
}
