//! A priority as a prompt: a pick of what to play, cast or activate, with passing its button.

use beryl_engine::card::Purpose;
use beryl_engine::{Layout, Scratch, SlotId as _, observe};

use super::button;
use crate::prompt::held::{Clicked, Run, Step, Table};
use crate::prompt::mode::{Chosen, Mode, Way};
use crate::prompt::open::act_prompt;
use crate::prompt::wording::{PassLeads, pass_words};
use crate::prompt::{ButtonId, Click, Marked, PromptButton, PromptView, ShapeBody};
use crate::protocol::ObjectId;
use crate::protocol::{Candidate, DecisionView, PassTo, Side, own_activated};
use crate::script::{Mark, Subject};

/// What the player may do with their priority, or at a halt, where passing is all.
#[derive(Clone, Debug)]
pub(crate) struct Priority {
    /// Each card a click uses, in the order the decision offers them, then the cards that
    /// stand for one of those.
    cards: Vec<Used>,
    /// The candidate that passes.
    pass: Option<u16>,
    /// Whether the turn is the viewer's.
    yours: bool,
    /// The pass button's word where the view does not say where passing leads.
    word: String,
    buttons: Vec<PromptButton>,
    /// The priority's prompt as it opened, which the choice among a card's ways lies over.
    base: PromptView,
    /// The candidates' labels, by index: what each way to use a card is called.
    labels: Vec<String>,
    /// The choice open among the ways to use a card with several: its prompt as it opened,
    /// and its state.
    acting: Option<(PromptView, Mode)>,
}

/// A card a click on uses.
#[derive(Clone, Debug)]
struct Used {
    slot: ObjectId,
    name: Option<String>,
    /// The ways to use it, in the order the choice among them shows them.
    ways: Vec<Way>,
    /// Whether a way does more than tap it for mana
    /// ([CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a): "A player may
    /// activate an activated mana ability whenever they have priority"): every untapped
    /// source can, which no mark need say.
    plays: bool,
}

/// The card a candidate of a priority uses, and whether it does more than make mana.
fn used(option: &Candidate) -> Option<(ObjectId, bool)> {
    match *option {
        Candidate::PlayLand { slot }
        | Candidate::Cast { slot, .. }
        | Candidate::Activate { slot, .. }
        | Candidate::TurnFaceUp { slot } => Some((slot, true)),
        Candidate::Mana { slot, .. } => Some((slot, false)),
        _ => None,
    }
}

impl Priority {
    pub(super) fn open<L: Layout>(table: &Table<'_, L>, decision: &DecisionView) -> Priority {
        let name = |slot: ObjectId| {
            (table.view.objects.iter())
                .find(|o| o.slot == slot)
                .and_then(|o| o.name.clone())
        };
        let mut cards: Vec<Used> = Vec::new();
        let mut pass = None;
        for (i, candidate) in decision.candidates.iter().enumerate() {
            let i = u16::try_from(i).expect("at most 64 candidates");
            if candidate.option == Candidate::Pass {
                pass = Some(i);
            }
            let Some((slot, plays)) = used(&candidate.option) else {
                continue;
            };
            match cards.iter_mut().find(|card| card.slot == slot) {
                Some(card) => {
                    card.ways.push(Way::Offered(i));
                    card.plays |= plays;
                }
                None => cards.push(Used {
                    slot,
                    name: name(slot),
                    ways: vec![Way::Offered(i)],
                    plays,
                }),
            }
        }
        for card in &mut cards {
            card.ways = in_face_order(table, decision, card.slot, std::mem::take(&mut card.ways));
        }
        // Either of two interchangeable cards uses the one the engine offers for both.
        let offered: Vec<ObjectId> = cards.iter().map(|card| card.slot).collect();
        for (stand_in, offered) in table.stand_ins(&offered) {
            let of = (cards.iter())
                .find(|card| card.slot == offered)
                .expect("a card offered");
            cards.push(Used {
                slot: stand_in,
                name: name(stand_in),
                ..of.clone()
            });
        }
        let mut priority = Priority {
            cards,
            pass,
            yours: table.view.turn.active == Side::You,
            word: decision.prompt.words.decline.clone(),
            buttons: Vec::new(),
            base: (*decision.prompt).clone(),
            labels: (decision.candidates.iter())
                .map(|candidate| candidate.label.clone())
                .collect(),
            acting: None,
        };
        priority.reword(decision);
        priority
    }

    /// Words the buttons for `decision`: the pass, by where it leads, and the pass on to the
    /// viewer's next stop where passing would stop them again in this step.
    pub(super) fn reword(&mut self, decision: &DecisionView) {
        let leads = |to: &PassTo| match *to {
            PassTo::Resolve => PassLeads::Resolve,
            PassTo::CombatDamage => PassLeads::CombatDamage,
            PassTo::Step { step } => PassLeads::Step(step),
            PassTo::NextTurn => PassLeads::NextTurn,
        };
        let (label, caption) = match (&decision.pass_to, decision.halt) {
            (_, true) => pass_words(PassLeads::Halt, self.yours),
            (Some(to), false) => pass_words(leads(to), self.yours),
            (None, false) => (self.word.as_str(), ""),
        };
        let pass = PromptButton {
            caption: caption.to_owned(),
            ..button(ButtonId::Decline, label, self.pass.is_some(), true)
        };
        let on = (decision.pass_on.as_ref()).map(|to| {
            button(
                ButtonId::PassOn,
                pass_words(leads(to), self.yours).1,
                true,
                false,
            )
        });
        self.buttons = std::iter::once(pass).chain(on).collect();
    }

    pub(super) fn click(&mut self, click: Click) -> Clicked {
        let answers =
            |index: u16| Clicked::Answers(Run::new(Purpose::Priority, [Step::Index(index)]));
        // The choice among a card's ways takes every click while it is open.
        if let Some((_, mode)) = &self.acting {
            return match mode.click(click) {
                Chosen::Answers(run) => Clicked::Answers(run),
                Chosen::Closes => {
                    self.acting = None;
                    Clicked::Changed
                }
                Chosen::Same => Clicked::Same,
            };
        }
        match click {
            Click::On {
                on: Subject::Object { slot },
            } => match self.cards.iter().find(|card| card.slot == slot) {
                Some(Used { ways, .. }) if let [Way::Offered(way)] = ways[..] => answers(way),
                // Several ways to use it: the choice among them opens over the priority.
                Some(card) => {
                    let prompt = act_prompt(slot, card.name.as_deref());
                    let mode = Mode::act(&self.labels, slot, &card.ways, &prompt.words.cancel);
                    self.acting = Some((prompt, mode));
                    Clicked::Changed
                }
                None => Clicked::Same,
            },
            Click::Button {
                button: ButtonId::Decline,
            } => self.pass.map_or(Clicked::Same, answers),
            Click::Button {
                button: ButtonId::PassOn,
            } if self.buttons.iter().any(|b| b.id == ButtonId::PassOn) => Clicked::PassesOn,
            _ => Clicked::Same,
        }
    }

    /// Writes into `view` the state of the choice among a card's ways while it is open, and
    /// the priority's own otherwise: one prompt lies over the other, and they count their
    /// states as one.
    pub(super) fn show(&self, view: &mut PromptView) {
        let prompt = match &self.acting {
            Some((prompt, _)) => prompt,
            None => &self.base,
        };
        if view.purpose != prompt.purpose {
            *view = PromptView {
                revision: view.revision,
                ..prompt.clone()
            };
        }
        if let Some((_, mode)) = &self.acting {
            return mode.show(view);
        }
        view.marks = (self.cards.iter())
            .filter(|card| card.plays)
            .map(|card| Marked {
                on: Subject::Object { slot: card.slot },
                mark: Mark::Eligible,
            })
            .collect();
        view.buttons.clone_from(&self.buttons);
        view.body = ShapeBody::Pick {
            tray: Vec::new(),
            least: 0,
            most: 1,
            picked: Vec::new(),
            warned: Vec::new(),
        };
    }
}

/// The ways to use the card in `slot` that the priority `decision` offers, `offered`, as the
/// choice among them shows them. A permanent with two or more activated abilities that are not
/// mana abilities shows each of those in the order its face lists them, those it cannot
/// activate now among them, and then its other ways: Arena's "Choose One" for a planeswalker
/// has a frame for every loyalty ability, one its loyalty cannot pay dimmed
/// (`2VW6ZzmS8U4 6:24.5`), so that a click on such a permanent always opens the choice. Any
/// other card shows what is offered.
fn in_face_order<L: Layout>(
    table: &Table<'_, L>,
    decision: &DecisionView,
    slot: ObjectId,
    offered: Vec<Way>,
) -> Vec<Way> {
    let activates = |way: &Way, ability: u8| match *way {
        Way::Offered(i) => {
            decision.candidates[usize::from(i)].option
                == Candidate::Activate {
                    slot,
                    ability,
                    granted: false,
                }
        }
        Way::Unusable { .. } => false,
    };
    let own = |way: &Way| match *way {
        Way::Offered(i) => matches!(
            decision.candidates[usize::from(i)].option,
            Candidate::Activate { granted: false, .. }
        ),
        Way::Unusable { .. } => false,
    };
    let (Some(d), Some(object)) = (table.d, L::Slot::from_num(slot.0)) else {
        return offered;
    };
    if !offered.iter().any(own) {
        return offered;
    }
    let mut scratch = Scratch::new();
    let abilities = own_activated(
        table.t,
        observe(table.shown, table.t, &mut scratch, d.player),
        object,
    );
    if abilities.len() < 2 {
        return offered;
    }
    let mut ways = Vec::with_capacity(abilities.len() + offered.len());
    for ability in &abilities {
        let before = ways.len();
        ways.extend(
            (offered.iter())
                .filter(|w| activates(w, ability.ability))
                .cloned(),
        );
        if ways.len() == before {
            ways.push(Way::Unusable {
                nth: ability.nth,
                label: ability.label.clone(),
            });
        }
    }
    let shown = |way: &Way| abilities.iter().any(|a| activates(way, a.ability));
    ways.extend(offered.into_iter().filter(|way| !shown(way)));
    ways
}
