//! The pick shape: objects picked one click at a time, where they lie or from a tray.
//!
//! A priority is a pick of what to use ([`Priority`]). A choice of targets, an untap of lands
//! and a choice among permanents are picks of objects ([`Objects`]): each click picks or
//! unpicks one, and the prompt answers by a button, or by the click itself where one object
//! on the table is all it takes.

mod priority;
mod untap;

use beryl_engine::card::{Part, Purpose};
use beryl_engine::{Layout, target_removes_permanent};

use self::priority::Priority;
use super::held::{Clicked, Run, Step, Table, Want};
use super::wording::{Holes, OWN_TARGET};
use super::{ButtonId, Click, Marked, PartName, PromptButton, PromptView, ShapeName, ShapeView};
use crate::protocol::{Candidate, DecisionView, ObjectView, Side, TargetRef, ZoneName};
use crate::script::{Mark, Subject};

/// The most cards a choice among cards picks where they lie: past it they are laid out in a
/// tray.
const ON_THE_TABLE: usize = 5;

/// The state of a prompt of the pick shape.
#[derive(Clone, Debug)]
pub(crate) enum Pick {
    Priority(Priority),
    Objects(Objects),
}

impl Pick {
    /// The state the prompt for `purpose` of `decision`, the open decision on `table`, opens
    /// in, if it is a pick whose state the session holds.
    pub(crate) fn open<L: Layout>(
        table: &Table<'_, L>,
        purpose: Purpose,
        decision: &DecisionView,
    ) -> Option<Pick> {
        let prompt = &decision.prompt;
        if prompt.shape != ShapeName::Pick {
            return None;
        }
        match (purpose, prompt.part) {
            (Purpose::Priority, _) => Some(Pick::Priority(Priority::open(table, decision))),
            (Purpose::Target, PartName::Count | PartName::Pick) => {
                Some(Pick::Objects(Objects::targets(table, decision)))
            }
            (Purpose::Untap | Purpose::LegendRule | Purpose::Choose, PartName::Pick) => {
                Some(Pick::Objects(Objects::on_table(table, purpose, decision)))
            }
            _ => None,
        }
    }

    pub(crate) fn click(&mut self, click: Click) -> Clicked {
        match self {
            Pick::Priority(priority) => priority.click(click),
            Pick::Objects(objects) => objects.click(click),
        }
    }

    pub(crate) fn reword(&mut self, decision: &DecisionView) {
        if let Pick::Priority(priority) = self {
            priority.reword(decision);
        }
    }

    /// Writes the state into `view`: its marks, its buttons and the shape's own.
    pub(crate) fn show(&self, view: &mut PromptView) {
        match self {
            Pick::Priority(priority) => priority.show(view),
            Pick::Objects(objects) => objects.show(view),
        }
    }
}

/// A pick of objects: targets, or cards and permanents chosen for an effect.
#[derive(Clone, Debug)]
pub(crate) struct Objects {
    purpose: Purpose,
    /// What the prompt asks as it opened.
    title: String,
    /// What a click can pick: what the decision offers, in its order, then the cards that
    /// stand for one of those ([`Table::stand_ins`]).
    eligible: Vec<Subject>,
    /// The cards laid out to pick from; empty when they are picked where they lie.
    tray: Vec<u16>,
    least: u8,
    most: u8,
    picked: Vec<Subject>,
    /// The run opens with the number picked: a count of targets
    /// ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c): "If the spell has a
    /// variable number of targets, the player announces how many targets they will choose
    /// before they announce those targets").
    counted: bool,
    /// The decision offers an end to the picks, which the run ends with.
    ends: bool,
    /// For targets: those the viewer controls, which a pick marks as theirs.
    friendly: Option<Vec<Subject>>,
    /// Those the prompt asks about before it answers with them, each with its question.
    warned: Vec<(Subject, String)>,
    /// The buttons' words, `{picked}` and `{of}` still to fill.
    confirm: String,
    several: String,
    decline: String,
}

fn card(slot: u16) -> Subject {
    Subject::Object { slot }
}

fn object(view: &[ObjectView], slot: u16) -> Option<&ObjectView> {
    view.iter().find(|o| o.slot == slot)
}

impl Objects {
    /// A choice of targets, opening at its count or at its first target. A count is taken
    /// with the targets: the player picks them and the run announces how many.
    fn targets<L: Layout>(table: &Table<'_, L>, decision: &DecisionView) -> Objects {
        let prompt = &decision.prompt;
        let counted = prompt.part == PartName::Count;
        let mut eligible = Vec::new();
        let mut offer = |on: Subject| {
            if !eligible.contains(&on) {
                eligible.push(on);
            }
        };
        let mut counts = Vec::new();
        for candidate in &decision.candidates {
            match candidate.option {
                Candidate::Object { slot } => offer(card(slot)),
                Candidate::Player { player } => offer(Subject::Player { player }),
                Candidate::Number { n } => counts.push(n),
                _ => {}
            }
            // What a count lets the first target be.
            for target in &candidate.targets {
                offer(match *target {
                    TargetRef::Object { slot } => card(slot),
                    TargetRef::Player { player } => Subject::Player { player },
                });
            }
        }
        with_stand_ins(table, &mut eligible);
        let all = u8::try_from(eligible.len()).unwrap_or(u8::MAX);
        let (least, most) = if counted {
            let most = counts.iter().copied().max().unwrap_or(0).min(all);
            (counts.iter().copied().min().unwrap_or(0).min(most), most)
        } else {
            let most = prompt.most.max(1).min(all);
            (most, most)
        };
        let objects = &table.view.objects;
        let yours = |on: &Subject| match *on {
            Subject::Object { slot } => {
                object(objects, slot).is_some_and(|o| o.controller == Side::You)
            }
            Subject::Player { player } => player == Side::You,
            Subject::Ability { .. } => false,
        };
        let friendly: Vec<Subject> = eligible.iter().copied().filter(yours).collect();
        // UI Conventions, exception 7: a removal aimed at the viewer's own permanent is asked
        // about first. The engine says whether the target being chosen is one a removal takes.
        let removes = target_removes_permanent(table.s, table.t);
        let warned = (friendly.iter().copied())
            .filter_map(|on| {
                let Subject::Object { slot } = on else {
                    return None;
                };
                let o = object(objects, slot).filter(|o| o.zone == ZoneName::Battlefield)?;
                let holes = Holes {
                    name: o.name.as_deref(),
                    ..Holes::default()
                };
                Some((on, holes.title(&OWN_TARGET)))
            })
            .filter(|_| removes)
            .collect();
        Objects {
            purpose: Purpose::Target,
            title: prompt.title.clone(),
            eligible,
            tray: Vec::new(),
            least,
            most,
            picked: Vec::new(),
            counted,
            ends: false,
            friendly: Some(friendly),
            warned,
            confirm: prompt.words.confirm.clone(),
            several: prompt.words.counted.clone(),
            decline: prompt.words.decline.clone(),
        }
    }

    /// A choice among cards and permanents for an effect or a rule: an untap of lands, the
    /// legend to keep, a card. They are picked where they lie when they are few and every
    /// player sees where ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002):
    /// "Graveyard, battlefield, stack, exile, ante, and command are public zones"), and from
    /// a tray otherwise.
    fn on_table<L: Layout>(
        table: &Table<'_, L>,
        purpose: Purpose,
        decision: &DecisionView,
    ) -> Objects {
        let prompt = &decision.prompt;
        let offered: Vec<u16> = (decision.candidates.iter())
            .filter_map(|c| match c.option {
                Candidate::Object { slot } => Some(slot),
                _ => None,
            })
            .collect();
        let ends = (decision.candidates.iter()).any(|c| c.option == Candidate::Done);
        let objects = &table.view.objects;
        let public = |&slot: &u16| {
            object(objects, slot).is_some_and(|o| {
                matches!(
                    o.zone,
                    ZoneName::Battlefield | ZoneName::Graveyard | ZoneName::Exile | ZoneName::Stack
                )
            })
        };
        let tray = if offered.len() <= ON_THE_TABLE && offered.iter().all(public) {
            Vec::new()
        } else {
            offered.clone()
        };
        let mut eligible: Vec<Subject> = offered.iter().copied().map(card).collect();
        with_stand_ins(table, &mut eligible);
        let all = u8::try_from(eligible.len()).unwrap_or(u8::MAX);
        let most = prompt.most.max(1).min(all);
        let picked = if purpose == Purpose::Untap {
            untap::opening(table, &eligible, usize::from(most))
        } else {
            Vec::new()
        };
        Objects {
            purpose,
            title: prompt.title.clone(),
            eligible,
            tray,
            least: if ends { 0 } else { most },
            most,
            picked,
            counted: false,
            ends,
            friendly: None,
            warned: Vec::new(),
            confirm: prompt.words.confirm.clone(),
            several: prompt.words.counted.clone(),
            decline: prompt.words.decline.clone(),
        }
    }

    /// Whether a click on an object answers: one object, picked where it lies, with no end to
    /// the picks offered.
    fn answers_by_click(&self) -> bool {
        self.most == 1 && self.tray.is_empty() && !self.ends
    }

    /// The question the prompt is asking about its pick, if it is: the pick is one it asks
    /// about before a click answers with it.
    fn asking(&self) -> Option<&str> {
        let picked = self.picked.first().filter(|_| self.answers_by_click())?;
        (self.warned.iter())
            .find(|(on, _)| on == picked)
            .map(|(_, question)| question.as_str())
    }

    /// Whether the prompt may answer with nothing: no target, or no pick where the decision
    /// offers an end.
    fn declines(&self) -> bool {
        self.ends || (self.counted && self.least == 0)
    }

    /// Whether what is picked answers the prompt.
    fn ready(&self) -> bool {
        let picked = self.picked.len();
        (usize::from(self.least)..=usize::from(self.most)).contains(&picked)
            && (picked > 0 || !self.declines())
    }

    fn pick(&mut self, on: Subject) -> Clicked {
        if !self.eligible.contains(&on) {
            return Clicked::Same;
        }
        if let Some(at) = self.picked.iter().position(|&p| p == on) {
            self.picked.remove(at);
            return Clicked::Changed;
        }
        if self.most == 1 {
            // Another pick takes the place of the one made.
            self.picked = vec![on];
            return if self.answers_by_click() && self.asking().is_none() {
                Clicked::Answers(self.run())
            } else {
                Clicked::Changed
            };
        }
        if self.picked.len() >= usize::from(self.most) {
            return Clicked::Same;
        }
        self.picked.push(on);
        Clicked::Changed
    }

    fn click(&mut self, click: Click) -> Clicked {
        match click {
            Click::On { on } => self.pick(on),
            Click::Button {
                button: ButtonId::Confirm,
            } if self.asking().is_some() || (!self.answers_by_click() && self.ready()) => {
                Clicked::Answers(self.run())
            }
            // The pick asked about is taken back.
            Click::Button {
                button: ButtonId::Decline,
            } if self.asking().is_some() => {
                self.picked.clear();
                Clicked::Changed
            }
            Click::Button {
                button: ButtonId::Decline,
            } if self.declines() => {
                self.picked.clear();
                Clicked::Answers(self.run())
            }
            _ => Clicked::Same,
        }
    }

    /// The picks that answer with what is picked: the count where the prompt opened at one,
    /// each object as the engine offers it, and the end where the decision offers one.
    fn run(&self) -> Run {
        let count = u8::try_from(self.picked.len()).expect("at most 255 picks");
        let steps = [
            self.counted
                .then_some(Step::One(Part::Count, Want::Number(count))),
            (!self.picked.is_empty()).then(|| Step::Among(Part::Pick, self.picked.clone())),
            self.ends.then_some(Step::One(Part::Pick, Want::Done)),
        ];
        Run::new(self.purpose, steps.into_iter().flatten())
    }

    fn show(&self, view: &mut PromptView) {
        let asking = self.asking();
        asking.unwrap_or(&self.title).clone_into(&mut view.title);
        // Past the most, a click picks nothing more; with one to pick, it picks another.
        let open = self.most == 1 || self.picked.len() < usize::from(self.most);
        view.marks = (self.eligible.iter())
            .filter_map(|&on| {
                let mark = match (self.picked.contains(&on), &self.friendly) {
                    (true, None) => Mark::Selected,
                    (true, Some(friendly)) if friendly.contains(&on) => Mark::FriendlyTarget,
                    (true, Some(_)) => Mark::HostileTarget,
                    (false, _) if open => Mark::Eligible,
                    (false, _) => return None,
                };
                Some(Marked { on, mark })
            })
            .collect();
        view.buttons = if asking.is_some() {
            vec![
                button(ButtonId::Confirm, OWN_TARGET.confirm, true, false),
                button(ButtonId::Decline, OWN_TARGET.decline, true, true),
            ]
        } else {
            self.buttons()
        };
        view.state = Some(ShapeView::Pick {
            tray: self.tray.clone(),
            least: self.least,
            most: self.most,
            picked: self.picked.clone(),
            warned: self.warned.iter().map(|&(on, _)| on).collect(),
        });
    }

    /// The confirm button, where a click on an object does not answer, and the button that
    /// answers with nothing, where the prompt may.
    fn buttons(&self) -> Vec<PromptButton> {
        let mut buttons = Vec::new();
        let confirms = !self.answers_by_click();
        let ready = self.ready();
        if confirms {
            let words = if self.most > 1 && !self.several.is_empty() {
                &self.several
            } else {
                &self.confirm
            };
            let label = words
                .replace("{picked}", &self.picked.len().to_string())
                .replace("{of}", &self.most.to_string());
            buttons.push(button(ButtonId::Confirm, &label, ready, ready));
        }
        if self.declines() {
            let primary = confirms && !ready;
            buttons.push(button(ButtonId::Decline, &self.decline, true, primary));
        }
        buttons
    }
}

/// Adds to `eligible`, what a decision offers, the cards that stand for one of its objects
/// ([`Table::stand_ins`]).
fn with_stand_ins<L: Layout>(table: &Table<'_, L>, eligible: &mut Vec<Subject>) {
    let offered: Vec<u16> = (eligible.iter())
        .filter_map(|on| match *on {
            Subject::Object { slot } => Some(slot),
            _ => None,
        })
        .collect();
    eligible.extend((table.stand_ins(&offered).into_iter()).map(|(stand_in, _)| card(stand_in)));
}

fn button(id: ButtonId, label: &str, enabled: bool, primary: bool) -> PromptButton {
    PromptButton {
        id,
        label: label.to_owned(),
        caption: String::new(),
        enabled,
        primary,
    }
}
