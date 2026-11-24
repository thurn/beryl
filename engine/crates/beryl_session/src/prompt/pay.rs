//! The pay shape: a cost paid by tapping permanents the player picks.
//!
//! One prompt serves every cost paid this way: mana, from a payment plan or one source at a
//! time; permanents tapped in place of mana; permanents whose power is counted. The player
//! picks the permanents that pay; the session says what else could, what its default would
//! tap for the rest (`suggested`), what is left to pay, and whether the default can pay it,
//! which the Auto button then does (UI Conventions, "Paying with permanents").
//!
//! The engine takes most such costs one permanent at a time (`taps.rs`), and a cost in mana
//! alone as one of the payment plans it offers ([`Plans`]). Where the purpose has a confirm
//! button, the button answers once the picks pay the cost in full; where it has none, the
//! pick that does answers.

mod taps;

use beryl_engine::card::{ManaCost, Part, Purpose};
use beryl_engine::{Decision, GameTable, Layout, State};

use self::taps::Due;
use super::held::{Clicked, Run, Step, Table, Want, button};
use super::{ButtonId, Click, Marked, PartName, PromptView, ShapeBody};
use crate::protocol::ObjectId;
use crate::protocol::{Candidate, DecisionView, mana_text};
use crate::script::{Mark, Subject};

/// The state of a payment.
#[derive(Clone, Debug)]
pub(crate) struct Pay {
    how: How,
    /// The permanents picked to pay, in the order they were.
    picked: Vec<ObjectId>,
    /// What the picks come to.
    now: Outlook,
    /// The whole cost, as the screen writes it.
    cost: String,
    /// The confirm button's words, `{picked}` still to fill: empty where the purpose has no
    /// such button, and the pick that pays the cost in full answers.
    confirm: String,
    auto: String,
}

/// How the engine takes the payment.
#[derive(Clone, Debug)]
enum How {
    /// Whole: one of the plans it offers.
    Plans(Plans),
    /// One permanent at a time.
    Taps(Due),
}

/// What a set of picks comes to.
#[derive(Clone, Debug)]
pub(crate) struct Outlook {
    /// The permanents a click can add.
    eligible: Vec<ObjectId>,
    /// The permanents the default would tap beyond those picked.
    suggested: Vec<ObjectId>,
    /// What is still to pay, as the screen writes it.
    left: String,
    /// The picks pay the cost in full.
    paid: bool,
    /// The picks that pay: those of the permanents picked, and the default's for what they
    /// leave. None where the default cannot pay it.
    run: Option<Run>,
}

impl Pay {
    /// The state the payment `decision`, the open decision on `table`, opens in: nothing
    /// picked.
    pub(crate) fn open<L: Layout>(
        table: &Table<'_, L>,
        purpose: Purpose,
        decision: &DecisionView,
    ) -> Option<Pay> {
        let prompt = &decision.prompt;
        let d = table.d?;
        let mana = table.cost();
        let (how, now) = if prompt.part == PartName::Plan {
            let plans = Plans::of(purpose, decision, mana.clone().unwrap_or_default())?;
            let now = plans.outlook(&[])?;
            (How::Plans(plans), now)
        } else {
            let due = Due::of(table.s, table.t, d);
            if !due.is_payable() {
                return None;
            }
            let now = taps::outlook(table.s, table.t, d, &due, &[])?;
            (How::Taps(due), now)
        };
        Some(Pay {
            how,
            picked: Vec::new(),
            now,
            cost: mana.unwrap_or_else(|| table.record.count.to_string()),
            confirm: prompt.words.confirm.clone(),
            auto: prompt.words.auto.clone(),
        })
    }

    /// What the picks of `picked` come to in the game `s`, at the payment's decision `d`.
    fn outlook<L: Layout>(
        &self,
        s: &State<L>,
        t: &GameTable,
        d: &Decision<L>,
        picked: &[ObjectId],
    ) -> Option<Outlook> {
        match &self.how {
            How::Plans(plans) => plans.outlook(picked),
            How::Taps(due) => taps::outlook(s, t, d, due, picked),
        }
    }

    /// Takes `click`, made in the game `s` at the payment's decision `d`.
    pub(crate) fn click<L: Layout>(
        &mut self,
        s: &State<L>,
        t: &GameTable,
        d: &Decision<L>,
        click: Click,
    ) -> Clicked {
        match click {
            Click::On {
                on: Subject::Object { slot },
            } => {
                let mut picked = self.picked.clone();
                if let Some(at) = picked.iter().position(|&picked| picked == slot) {
                    picked.remove(at);
                } else if self.now.eligible.contains(&slot) || self.now.suggested.contains(&slot) {
                    picked.push(slot);
                } else {
                    return Clicked::Same;
                }
                let Some(now) = self.outlook(s, t, d, &picked) else {
                    return Clicked::Same;
                };
                let adds = picked.len() > self.picked.len();
                self.picked = picked;
                self.now = now;
                // Without a button to confirm it, the pick that pays the cost answers.
                match &self.now.run {
                    Some(run) if adds && self.now.paid && self.confirm.is_empty() => {
                        Clicked::Answers(run.clone())
                    }
                    _ => Clicked::Changed,
                }
            }
            Click::Button {
                button: ButtonId::Auto,
            } => (self.now.run.clone()).map_or(Clicked::Same, Clicked::Answers),
            Click::Button {
                button: ButtonId::Confirm,
            } if !self.confirm.is_empty() && self.now.paid => {
                (self.now.run.clone()).map_or(Clicked::Same, Clicked::Answers)
            }
            _ => Clicked::Same,
        }
    }

    pub(crate) fn show(&self, view: &mut PromptView) {
        let marked = |mark: Mark| {
            move |&slot: &ObjectId| Marked {
                on: Subject::Object { slot },
                mark,
            }
        };
        let eligible = (self.now.eligible.iter()).filter(|slot| !self.now.suggested.contains(slot));
        view.marks = (self.picked.iter().map(marked(Mark::Paying)))
            .chain(self.now.suggested.iter().map(marked(Mark::Suggested)))
            .chain(eligible.map(marked(Mark::Eligible)))
            .collect();
        let confirms = !self.confirm.is_empty();
        let paid = confirms && self.now.paid;
        let ready = self.now.run.is_some();
        let auto = button(ButtonId::Auto, &self.auto, ready, ready && !paid);
        let confirm = confirms.then(|| {
            let label = (self.confirm).replace("{picked}", &self.picked.len().to_string());
            button(ButtonId::Confirm, &label, paid, paid)
        });
        view.buttons = [auto].into_iter().chain(confirm).collect();
        view.body = ShapeBody::Pay {
            cost: self.cost.clone(),
            left: self.now.left.clone(),
        };
    }
}

/// The payment plans a cost in mana offers, each by the permanents it taps
/// ([CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g): "If the total cost
/// includes a mana payment, the player then has a chance to activate mana abilities"). The
/// engine offers the plans that differ in what they consume or leave floating (architecture
/// §10): the player picks the permanents to tap, and the plan that taps exactly those
/// answers.
#[derive(Clone, Debug)]
struct Plans {
    purpose: Purpose,
    /// Each plan: its index among the engine's, and what it taps.
    offered: Vec<(u16, Vec<ObjectId>)>,
    cost: String,
}

impl Plans {
    /// The plans `decision` offers for the mana cost `cost`, as Oracle text writes it, if
    /// they can be told apart by what they tap.
    fn of(purpose: Purpose, decision: &DecisionView, cost: String) -> Option<Plans> {
        let mut plans = Vec::new();
        for candidate in &decision.candidates {
            let Candidate::Plan { index } = candidate.option else {
                return None;
            };
            let mut taps = candidate.payments.first()?.taps.clone();
            taps.sort_unstable();
            if plans.iter().any(|(_, other)| *other == taps) {
                return None;
            }
            plans.push((index, taps));
        }
        Some(Plans {
            purpose,
            offered: plans,
            cost,
        })
    }

    /// What the picks of `picked` come to: none if no plan taps them all.
    fn outlook(&self, picked: &[ObjectId]) -> Option<Outlook> {
        let open: Vec<&(u16, Vec<ObjectId>)> = (self.offered.iter())
            .filter(|(_, taps)| picked.iter().all(|slot| taps.contains(slot)))
            .collect();
        // The plan that taps exactly what is picked; or else the default, the first that taps
        // it all: the engine lists the plan it would choose first.
        let exact = open.iter().find(|(_, taps)| taps.len() == picked.len());
        let &(index, ref default) = *exact.or(open.first())?;
        let unpicked = |slot: &&ObjectId| !picked.contains(slot);
        let suggested: Vec<ObjectId> = default.iter().filter(unpicked).copied().collect();
        let mut eligible: Vec<ObjectId> = (open.iter().flat_map(|(_, taps)| taps))
            .filter(unpicked)
            .copied()
            .collect();
        eligible.sort_unstable();
        eligible.dedup();
        // A plan is one pick that pays the whole cost: until the picks make one, all of it
        // is left.
        let plan = Step::One(Part::Plan, Want::Plan(index));
        Some(Outlook {
            eligible,
            suggested,
            left: if exact.is_some() {
                mana_text(ManaCost::default())
            } else {
                self.cost.clone()
            },
            paid: exact.is_some(),
            run: Some(Run::new(self.purpose, [plan]).continuing()),
        })
    }
}
