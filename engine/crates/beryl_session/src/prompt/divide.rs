//! The divide shape: an amount given out among recipients, a share each.
//!
//! [CR 601.2d](https://yawgatog.com/resources/magic-rules/#R6012d): "If the spell requires
//! the player to divide or distribute an effect (such as damage or counters) among one or
//! more targets, the player announces the division. Each of these targets must receive at
//! least one of whatever is being divided."
//!
//! No card asks one yet. A decision of the shape is about the recipients still to be given a
//! share and offers the shares the first of them may take, so the picks are a number for each
//! recipient in turn. The last recipient's is not among them: what is left is all it can be
//! given, and the engine asks no pick with one answer (architecture §7.2).
//!
//! The prompt opens with the least share on each recipient, sets a share at a click that
//! names its number, and confirms once nothing is left to give. Each recipient is marked as
//! the target it is, the viewer's own apart from another's, so the screen draws the arrow to
//! it that the spell will have once it is cast (UI Conventions, exception 4).

use beryl_engine::Layout;
use beryl_engine::card::{Part, Purpose};

use super::held::{Clicked, Run, Step, Table, Want, button};
use super::{ButtonId, Click, Marked, PromptView, ShapeBody, Share};
use crate::protocol::ObjectId;
use crate::protocol::{Candidate, DecisionView, Side};
use crate::script::{Mark, Subject};

/// The state of a prompt of the divide shape.
#[derive(Clone, Debug)]
pub(crate) struct Divide {
    purpose: Purpose,
    /// What there is to give out among the recipients.
    total: u8,
    /// The least a recipient may be given.
    least: u8,
    /// Each recipient and its share, in the order the engine asks for them.
    shares: Vec<(ObjectId, u8)>,
    /// The recipients the viewer controls.
    friendly: Vec<ObjectId>,
    confirm: String,
}

impl Divide {
    /// The state the divide prompt for `purpose` of `decision` opens in. What there is to
    /// give out is read from the shares the first recipient is offered: the most it may take
    /// leaves each of the others the least.
    pub(crate) fn open<L: Layout>(
        table: &Table<'_, L>,
        purpose: Purpose,
        decision: &DecisionView,
    ) -> Option<Divide> {
        let prompt = &decision.prompt;
        let yours = |slot: &ObjectId| {
            (table.object(*slot)).is_some_and(|object| object.controller == Side::You)
        };
        let offered = (decision.candidates.iter()).filter_map(|c| match c.option {
            Candidate::Number { n } => Some(n),
            _ => None,
        });
        let (least, most) = (offered.clone().min()?, offered.max()?);
        let others = u8::try_from(prompt.about.len().checked_sub(1)?).ok()?;
        Some(Divide {
            purpose,
            total: most.saturating_add(least.saturating_mul(others)),
            least,
            shares: prompt.about.iter().map(|&id| (id, least)).collect(),
            friendly: prompt.about.clone().into_iter().filter(yours).collect(),
            confirm: prompt.words.confirm.clone(),
        })
    }

    /// How much is not yet given out.
    fn left(&self) -> u8 {
        let given: u8 = self.shares.iter().map(|&(_, share)| share).sum();
        self.total - given
    }

    pub(crate) fn click(&mut self, click: Click) -> Clicked {
        match click {
            Click::Number {
                n,
                on: Some(Subject::Object { slot }),
            } => {
                let left = self.left();
                let Some((_, share)) = self.shares.iter_mut().find(|(on, _)| *on == slot) else {
                    return Clicked::Same;
                };
                // A share takes no more than its own and what is left.
                if n == *share || n < self.least || n > *share + left {
                    return Clicked::Same;
                }
                *share = n;
                Clicked::Changed
            }
            Click::Button {
                button: ButtonId::Confirm,
            } if self.left() == 0 => Clicked::Answers(self.run()),
            _ => Clicked::Same,
        }
    }

    /// A number for each recipient in turn but the last, whose share is what is left.
    fn run(&self) -> Run {
        let asked = self.shares.len().max(2) - 1;
        let shares = self.shares.iter().take(asked);
        Run::new(
            self.purpose,
            shares.map(|&(_, share)| Step::One(Part::Pick, Want::Number(share))),
        )
    }

    /// Writes the state into `view`: each recipient marked as a target, and its share.
    pub(crate) fn show(&self, view: &mut PromptView) {
        let given = self.left() == 0;
        view.marks = (self.shares.iter())
            .map(|&(slot, _)| Marked {
                on: Subject::Object { slot },
                mark: if self.friendly.contains(&slot) {
                    Mark::FriendlyTarget
                } else {
                    Mark::HostileTarget
                },
            })
            .collect();
        view.buttons = vec![button(ButtonId::Confirm, &self.confirm, given, given)];
        view.body = ShapeBody::Divide {
            total: self.total,
            left: self.left(),
            shares: (self.shares.iter())
                .map(|&(slot, amount)| Share {
                    on: Subject::Object { slot },
                    amount,
                })
                .collect(),
        };
    }
}
