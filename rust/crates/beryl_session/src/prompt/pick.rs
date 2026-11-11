//! The pick shape: objects picked one click at a time, where they lie or from a tray.
//!
//! A priority is a pick of what to use ([`Priority`]). A choice of targets, an untap of lands,
//! a choice among permanents and a choice of cards from a hand, a library or a graveyard
//! (`zone.rs`) are picks of objects ([`Objects`]): each click picks or unpicks one, and the
//! prompt answers by a button, or by the click itself where one object on the table is all it
//! takes.

mod priority;
mod untap;
mod zone;

use beryl_engine::card::{Part, Purpose};
use beryl_engine::{Layout, target_removes_permanent};

use self::priority::Priority;
use super::held::{Clicked, Run, Step, Table, Want, offered};
use super::wording::{AIMED, Holes, OWN_TARGET, Wording};
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
    Objects(Box<Objects>),
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
                Some(Pick::Objects(Box::new(Objects::targets(table, decision))))
            }
            (Purpose::Untap | Purpose::LegendRule | Purpose::Choose, PartName::Pick) => Some(
                Pick::Objects(Box::new(Objects::on_table(table, purpose, decision))),
            ),
            (
                Purpose::Search
                | Purpose::Look
                | Purpose::Discard
                | Purpose::Cleanup
                | Purpose::PutOnBottom
                | Purpose::ExileCost
                | Purpose::ManifestDread,
                _,
            ) => Objects::from_zone(table, purpose, decision)
                .map(Box::new)
                .map(Pick::Objects),
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
    /// What the run opens with, before its picks.
    opens: Opens,
    /// The decision offers an end to the picks, which the run ends with.
    ends: bool,
    /// The prompt answers by its button alone, never by a click on a card: the cards are not
    /// on the table for every player to see, and a card clicked is looked over first.
    confirmed: bool,
    /// The cards lie in a pile the engine takes them from in order (`zone.rs`).
    pile: Option<zone::Pile>,
    /// For targets: those the viewer controls, which a pick marks as theirs.
    friendly: Option<Vec<Subject>>,
    /// Those the prompt asks about before it answers with them, each with its question.
    warned: Vec<(Subject, String)>,
    /// The pick waits for the prompt's button, which confirms it: what the prompt says of
    /// each while it is the one picked. Empty where a click on one answers.
    aimed: Vec<(Subject, String)>,
    /// The buttons' words, `{picked}` and `{of}` still to fill.
    confirm: String,
    several: String,
    decline: String,
}

/// What a pick's run opens with, before the picks of its objects.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Opens {
    /// Nothing: the picks are all there is.
    Picks,
    /// The number picked: a count of targets
    /// ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c): "If the spell has a
    /// variable number of targets, the player announces how many targets they will choose
    /// before they announce those targets").
    Count,
    /// Whether to pick at all: yes with something picked, no with nothing.
    Whether,
    /// Whether to answer with one of these alone in place of the picks the prompt takes: yes
    /// with one of them picked and nothing else, no otherwise
    /// ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a): "Some spells,
    /// activated abilities, and triggered abilities read, '[Do something]. If [a player]
    /// [does or doesn't], [effect].' or '[A player] may [do something]. If [that player]
    /// [does or doesn't], [effect].'").
    Instead(Vec<Subject>),
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
        let (mut eligible, counts) = offered_targets(decision);
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
        let of = |on: Subject| match on {
            Subject::Object { slot } => object(objects, slot),
            _ => None,
        };
        let yours = |on: &Subject| match *on {
            Subject::Player { player } => player == Side::You,
            on => of(on).is_some_and(|o| o.controller == Side::You),
        };
        // What the prompt says of `on` in the words of `wording`, which name it.
        let says = |on: Subject, wording: &Wording| {
            let holes = Holes {
                name: of(on).and_then(|o| o.name.as_deref()),
                ..Holes::default()
            };
            (on, holes.title(wording))
        };
        let friendly: Vec<Subject> = eligible.iter().copied().filter(yours).collect();
        // UI Conventions, exception 7: a removal aimed at the viewer's own permanent is asked
        // about first. The engine says whether the target being chosen is one a removal takes.
        let removes = target_removes_permanent(table.s, table.t);
        let warned = (friendly.iter().copied())
            .filter(|&on| removes && of(on).is_some_and(|o| o.zone == ZoneName::Battlefield))
            .map(|on| says(on, &OWN_TARGET))
            .collect();
        // Among two or more spells on the stack, the pick waits for the button, as in Arena's
        // browser of the stack, and none opens picked (UI Conventions, "Targeting a spell"):
        // a click on a spell picks it, and one on another takes its place.
        let spell = |&on: &Subject| of(on).is_some_and(|o| o.zone == ZoneName::Stack);
        let browsed = !counted && most == 1 && eligible.len() > 1 && eligible.iter().all(spell);
        let aimed: Vec<_> = (eligible.iter().copied())
            .filter(|_| browsed)
            .map(|on| says(on, &AIMED))
            .collect();
        let title = if browsed {
            Holes::default().title(&AIMED)
        } else {
            prompt.title.clone()
        };
        // A target in a pile is laid out to be picked: the table shows a graveyard's top card
        // alone, and no card of an exile or a library.
        let piled = |zone| {
            !matches!(
                zone,
                ZoneName::Battlefield | ZoneName::Stack | ZoneName::Hand
            )
        };
        let tray = (eligible.iter())
            .filter_map(|&on| of(on).filter(|o| piled(o.zone)).map(|o| o.slot))
            .collect();
        Objects {
            purpose: Purpose::Target,
            title,
            eligible,
            tray,
            least,
            most,
            picked: Vec::new(),
            opens: if counted { Opens::Count } else { Opens::Picks },
            ends: false,
            confirmed: false,
            pile: None,
            friendly: Some(friendly),
            warned,
            aimed,
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
        let offered = offered(decision);
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
            opens: Opens::Picks,
            ends,
            confirmed: false,
            pile: None,
            friendly: None,
            warned: Vec::new(),
            aimed: Vec::new(),
            confirm: prompt.words.confirm.clone(),
            several: prompt.words.counted.clone(),
            decline: prompt.words.decline.clone(),
        }
    }

    /// Whether a click on an object answers: one object, with no end to the picks offered, in
    /// a prompt whose pick does not wait for its button. A target answers wherever it is picked;
    /// another card only where it lies on the table, since a tray's cards and a hand's are
    /// looked over first.
    fn answers_by_click(&self) -> bool {
        let lies = (self.tray.is_empty() && !self.confirmed) || self.friendly.is_some();
        self.most == 1 && lies && !self.ends && self.aimed.is_empty()
    }

    /// Whether one of the cards the prompt takes alone in place of its picks is all that is
    /// picked.
    fn alone(&self) -> bool {
        match (&self.opens, &self.picked[..]) {
            (Opens::Instead(about), [one]) => about.contains(one),
            _ => false,
        }
    }

    /// What `of` says of the one object picked, if it says anything of it.
    fn says<'a>(&self, of: &'a [(Subject, String)]) -> Option<&'a str> {
        let picked = self.picked.first()?;
        let (_, says) = of.iter().find(|(on, _)| on == picked)?;
        Some(says.as_str())
    }

    /// The question the prompt is asking about its pick, if it is: the pick is one it asks
    /// about before a click answers with it.
    fn asking(&self) -> Option<&str> {
        self.says(&self.warned).filter(|_| self.answers_by_click())
    }

    /// Whether the prompt may answer with nothing: no target, no pick where the decision
    /// offers an end, or a no to whether to pick at all.
    fn declines(&self) -> bool {
        match self.opens {
            Opens::Whether => true,
            Opens::Count => self.ends || self.least == 0,
            Opens::Picks | Opens::Instead(_) => self.ends,
        }
    }

    /// Whether what is picked answers the prompt.
    fn ready(&self) -> bool {
        let picked = self.picked.len();
        let enough = (usize::from(self.least)..=usize::from(self.most)).contains(&picked)
            && (picked > 0 || !self.declines());
        enough || self.alone()
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

    /// The picks that answer with what is picked: what the run opens with (the count, or the
    /// yes or no), each object as the engine offers it, and the end where the decision offers
    /// one and more could be picked.
    fn run(&self) -> Run {
        let count = u8::try_from(self.picked.len()).expect("at most 255 picks");
        let yes = |yes: bool| if yes { Want::Yes } else { Want::No };
        let opens = match &self.opens {
            Opens::Picks => None,
            Opens::Count => Some(Step::One(Part::Count, Want::Number(count))),
            Opens::Whether => Some(Step::One(Part::Whether, yes(count > 0))),
            Opens::Instead(_) => Some(Step::One(Part::Instead, yes(self.alone()))),
        };
        let picks = match &self.pile {
            Some(pile) => Step::InTurn(Part::Pick, pile.asked(&self.picked)),
            None => Step::Among(Part::Pick, self.picked.clone()),
        };
        let steps = [
            opens,
            (!self.picked.is_empty()).then_some(picks),
            // The engine asks for no end once it has as many as the prompt takes.
            (self.ends && self.picked.len() < usize::from(self.most))
                .then_some(Step::One(Part::Pick, Want::Done)),
        ];
        Run::new(self.purpose, steps.into_iter().flatten())
    }

    fn show(&self, view: &mut PromptView) {
        let asking = self.asking();
        (asking.or_else(|| self.says(&self.aimed)))
            .unwrap_or(&self.title)
            .clone_into(&mut view.title);
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

/// What `decision`, a choice of targets, offers to target, in its order, and the numbers of
/// targets it offers to announce: at a count, what each count lets the first target be.
fn offered_targets(decision: &DecisionView) -> (Vec<Subject>, Vec<u8>) {
    let (mut offered, mut counts) = (Vec::new(), Vec::new());
    let mut offer = |on: Subject| {
        if !offered.contains(&on) {
            offered.push(on);
        }
    };
    for candidate in &decision.candidates {
        match candidate.option {
            Candidate::Object { slot } => offer(card(slot)),
            Candidate::Player { player } => offer(Subject::Player { player }),
            Candidate::Number { n } => counts.push(n),
            _ => {}
        }
        for target in &candidate.targets {
            offer(match *target {
                TargetRef::Object { slot } => card(slot),
                TargetRef::Player { player } => Subject::Player { player },
            });
        }
    }
    (offered, counts)
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
