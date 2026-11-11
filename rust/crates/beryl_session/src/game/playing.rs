//! Playing the game on between views, and the decisions the session answers for the human.

use beryl_engine::card::Purpose;
use beryl_engine::layout::Queue;
use beryl_engine::state::StackEntry;
use beryl_engine::{
    Candidates, Choice, Decision, DecisionKind, GameTable, Large, Layout, Opt, Relayout, State,
    Step, TurnStep,
};

use crate::checkpoint::Checkpoint;
use crate::replay::Entry;

use super::{Answer, At, Boards, ChoiceOrigin, Game, Open, on_board};

impl Game {
    /// Plays on from [`At::Playing`] to the next place a view shows: a step beginning, where
    /// the human may be halted, a decision, or the end.
    pub(super) fn play_on(&mut self, replaying: bool) {
        debug_assert!(matches!(self.at, At::Playing));
        loop {
            let step = self
                .board
                .next_in_step(&self.table, self.seen, &mut self.chance);
            self.log_chance();
            // Once a cast is complete, or another player decides, it can no longer be taken back.
            if !matches!(&step, Some(Step::Decide(d)) if d.player == self.human && self.casting()) {
                self.before_cast = None;
            }
            // Only the human's decisions that follow their answer at once are its prompt's.
            if !matches!(&step, Some(Step::Decide(d)) if d.player == self.human || self.reviewing())
            {
                self.run = None;
            }
            self.at = match step {
                None => {
                    let turn = self.board.turn();
                    let began = turn.number() != self.seen.0;
                    self.seen = (turn.number(), turn.step());
                    self.answered = None;
                    if began {
                        let checkpoint =
                            on_board!(&self.board, b => Checkpoint::of(&self.table, &b.state));
                        self.replay
                            .log
                            .push(Entry::Checkpoint(Box::new(checkpoint)));
                    }
                    if self.halts() {
                        self.seq += 1;
                        At::Human(self.decision_id(), Open::Halt)
                    } else {
                        At::Playing
                    }
                }
                Some(Step::Over(outcome)) => At::Over {
                    outcome,
                    cause: None,
                    hurt_by: None,
                },
                Some(Step::Decide(d)) if d.player == self.human || self.reviewing() => {
                    if let Some(choice) = self.pick_in_run(&d) {
                        self.seq += 1;
                        let id = self.decision_id();
                        self.at = At::Human(id, Open::Decision(d));
                        let answer = Answer {
                            id,
                            choice,
                            continuation: true,
                        };
                        self.answer_as(answer, true);
                        continue;
                    }
                    if !self.reviewing()
                        && let Some(c) = self.auto_answer(&d, replaying)
                    {
                        self.apply(&d, c, ChoiceOrigin::Auto);
                        self.answered = Some(self.board.stack());
                        continue;
                    }
                    self.seq += 1;
                    At::Human(self.decision_id(), Open::Decision(d))
                }
                Some(Step::Decide(d)) => At::Agent(d),
            };
            if matches!(self.at, At::Human(..)) {
                self.passing = None;
            }
            return;
        }
    }

    /// The session's own answer to the human's decision `d`, if it gives one: under the
    /// smart stops, it passes their priority while only their own doings sit on the stack
    /// ([`Game::own_top`]) or in combat with nothing new to answer ([`Game::quiet_combat`]),
    /// orders their simultaneous triggers ([`trigger_to_stack`]), searches their library
    /// whenever they may ([`Game::searches_unasked`]), and takes unambiguous lethal combat
    /// assignments ([`beryl_engine::preferred_damage`]). Waterbend remains a human payment
    /// choice even when mana alone could pay it.
    /// Like halts, these are the session's alone: the engine's smart stops, which the agent's
    /// search shares, are unchanged, and exact stops ask every one. A function of the position,
    /// the stops and the human's answers, so a replay finds each again; the replay records
    /// each as an [`Entry::Auto`]. Replays play recorded damage assignments directly,
    /// including any manual choices made before automatic assignment was added.
    pub(super) fn auto_answer(&self, d: &Decision<Large>, replaying: bool) -> Option<Choice> {
        if d.kind == DecisionKind::Priority && self.passing.is_some_and(|p| self.passes_for(p)) {
            debug_assert_eq!(d.candidates.option(Choice(0)), Some(Opt::Pass));
            return Some(Choice(0));
        }
        if self.stops.full_control {
            return None;
        }
        match d.kind {
            DecisionKind::Priority if self.own_top() || self.quiet_combat() => {
                debug_assert_eq!(d.candidates.option(Choice(0)), Some(Opt::Pass));
                Some(Choice(0))
            }
            DecisionKind::TriggerOrder => Some(on_board!(&self.board, b => {
                trigger_to_stack(&b.state, &self.table, &d.relayout())
            })),
            DecisionKind::DamageDivision if !replaying => on_board!(&self.board, b => {
                beryl_engine::preferred_damage(&b.state, &d.relayout())
                    .and_then(|(choice, unambiguous)| unambiguous.then_some(choice))
            }),
            DecisionKind::YesNo if Self::searches_unasked(d) => (0..d.len() as u16)
                .map(Choice)
                .find(|&c| d.candidates.option(c) == Some(Opt::Yes)),
            _ => None,
        }
    }

    /// Whether `d` asks whether to search the human's library (Trinket Mage's "you may
    /// search"), which the smart stops answer by searching, as Arena does: the search itself
    /// offers taking nothing. Declining differs from searching and finding nothing
    /// ([CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b): "If a player is
    /// searching a hidden zone for cards with a stated quality ... that player isn't required
    /// to find some or all of those cards") only by the shuffle
    /// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a): "To shuffle a
    /// library ..., randomize the cards within it so that no player knows their order"). That
    /// shuffle loses a player a card they put back on top, a cost not worth a question before
    /// every search: a player who must keep it plays under exact stops, which ask.
    pub(super) fn searches_unasked(d: &Decision<Large>) -> bool {
        d.prompt.purpose == Purpose::MaySearch
    }

    /// Whether the top of the stack is the human's own spell or ability and, since their
    /// latest answer in this step, nothing of the opponent's came onto the stack: what they
    /// cast, its triggers, and its resolution, or the resolution of an opponent's response
    /// they already passed to, but not a new response, which they get priority to see and
    /// answer. Their priority then would only let them respond to themselves, as an Arena
    /// player auto-passes.
    pub(super) fn own_top(&self) -> bool {
        let answered = self.answered.as_deref().unwrap_or_default();
        let now = self.board.stack();
        let own = |e: &StackEntry<Large>| e.controller == self.human;
        let kept = answered
            .iter()
            .zip(&now)
            .take_while(|(then, now)| then == now)
            .count();
        now.last().is_some_and(own) && now[kept..].iter().all(own)
    }

    /// Whether the human's priority, with the stack empty, comes in a combat step where they
    /// have nothing new to answer: after declaring their own attackers, which they chose
    /// knowing all a response could use, or after a declaration of no blockers, which leaves
    /// no block for a trick to change. Players get priority in each of those steps once its
    /// declaration is made
    /// ([CR 117.3a](https://yawgatog.com/resources/magic-rules/#R1173a): "The active player
    /// receives priority at the beginning of most steps and phases, after any turn-based
    /// actions ... have been dealt with"), and the engine's smart stops, which the agent's
    /// search shares, give it to both players whenever creatures attack. Like Arena, the
    /// session stops the human after the opponent's attack, and after blocks.
    pub(super) fn quiet_combat(&self) -> bool {
        if !self.board.stack_is_empty() {
            return false;
        }
        let turn = self.board.turn();
        let attacker = turn.active() == self.human;
        match turn.step() {
            TurnStep::DeclareAttackers => attacker,
            TurnStep::DeclareBlockers => !self.any_blocking(),
            _ => false,
        }
    }

    pub(super) fn any_blocking(&self) -> bool {
        self.board.any_blocking()
    }

    /// Whether the human is halted as the step that just began, a session-level stop the
    /// engine knows nothing of: under the smart stops, at their own precombat main phase while
    /// it is one of their stops ([`StopsSetting::yours`]), unless the engine offers them a decision there before anything else is decided. The
    /// game would otherwise pass through it unseen whenever the human can do nothing but
    /// pass, and their turn would end without them. The opponent's turn plays on to theirs
    /// unhalted, as on Arena: the engine stops the human at its end step only when they can
    /// act there. Halts are a function of the position and the stops, so a replay finds them
    /// again; the agent never sees them.
    pub(super) fn halts(&mut self) -> bool {
        let turn = self.board.turn();
        let halting_step = turn.step() == TurnStep::PrecombatMain
            && turn.active() == self.human
            && self.stops.yours.has(TurnStep::PrecombatMain);
        if self.review.is_some() || self.stops.full_control || !halting_step {
            return false;
        }
        match self.board.ahead(&self.table, self.seen, &self.chance) {
            Some(Step::Decide(d)) => d.player != self.human,
            Some(Step::Over(_)) => false,
            None => true,
        }
    }
}

/// Which of the human's waiting triggers to put on the stack next, under the smart stops. Each
/// pick goes on the stack above the ones before, so the first picked resolves last
/// ([CR 603.3b](https://yawgatog.com/resources/magic-rules/#R6033b): "each player, in APNAP
/// order, puts each triggered ability they control with a trigger condition that has been
/// met on the stack in any order they choose").
///
/// The heuristic, by what the engine knows of each ability, resolves:
///
/// 1. first, the triggers with neither targets nor a question as they resolve: plain effects
///    (cards, tokens, counters, life, a sacrifice), whose results the others may act on;
/// 2. then the ones with targets, which were chosen as they went on the stack (CR 603.3d);
/// 3. last, the ones that ask their controller something as they resolve, so the answer is
///    made seeing everything the others did;
///
/// and, within each group, in the order they triggered.
pub fn trigger_to_stack<L: Layout>(s: &State<L>, t: &GameTable, d: &Decision<L>) -> Choice {
    let resolves_late = |opt: Opt<L>| {
        let Opt::Trigger(i) = opt else {
            unreachable!("a trigger-order decision offers only triggers");
        };
        let tr = &s.triggers()[Into::<usize>::into(i)];
        let ability = &t.def(tr.card).face(tr.face).abilities[tr.ability.index()];
        let group = if ability.asks() {
            2
        } else {
            u8::from(!ability.targets.is_empty())
        };
        (group, i)
    };
    let Candidates::Options(triggers) = &d.candidates else {
        unreachable!("a trigger-order decision offers options");
    };
    let (last, _) = triggers
        .iter()
        .enumerate()
        .max_by_key(|&(_, &opt)| resolves_late(opt))
        .expect("a decision has a candidate");
    Choice(u16::try_from(last).expect("a choice names its candidate"))
}

/// A payment that v8 completed without a prompt: it needs no benders and none of its
/// optional mana activations were offered at the time.
pub(super) fn only_nonland_mana<L: Layout>(s: &State<L>, d: &Decision<L>) -> bool {
    d.candidates.option(Choice(0)) == Some(Opt::Done)
        && (1..d.len() as u16).all(|i| {
            matches!(d.candidates.option(Choice(i)), Some(Opt::Mana { slot, .. })
                if !s.derived(slot).types.contains(beryl_engine::card::Types::LAND))
        })
}

/// `d` without the optional mana activations offered at human priority or during waterbend
/// ([`Profile::taps_mana`]), for an agent standing in for them: tapping ahead of a payment is
/// dominated by tapping during it (architecture §10), and search never sees them. The engine
/// lists them last, so every other candidate keeps its index.
pub(super) fn without_floating<L: Layout>(mut d: Decision<L>) -> Decision<L> {
    let floats = d.kind == DecisionKind::Priority || d.prompt.purpose == Purpose::Waterbend;
    if let (true, Candidates::Options(list)) = (floats, &mut d.candidates) {
        let kept = list
            .iter()
            .take_while(|o| !matches!(o, Opt::Mana { .. }))
            .count();
        debug_assert!(
            list.as_slice()[kept..]
                .iter()
                .all(|o| matches!(o, Opt::Mana { .. })),
            "mana activations come last"
        );
        list.retain(|o| !matches!(o, Opt::Mana { .. }));
    }
    d
}
