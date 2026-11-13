//! Replaying a record's log into a game, as [`Game::from_replay`] and [`Game::replay_exactly`] do.

use std::collections::VecDeque;

use beryl_engine::card::Purpose;
use beryl_engine::chance::{Mismatch, Random};
use beryl_engine::{
    CardTable, Choice, Decision, DecisionKind, GameTable, Large, Opt, Relayout, Seat, view,
};
use beryl_search::Budget;

use serde_json::json;

use crate::SessionError;
use crate::checkpoint::Checkpoint;
use crate::protocol::{CandidateIndex, PlayerView, SessionId};
use crate::replay::{Entry, FORMAT_VERSION, ReplayFile};
use crate::save::{self, Chosen, KindBeforeV10};
use crate::{journal, perturb};

use super::playing::{only_nonland_mana, without_floating};
use super::{At, Boards, ChoiceOrigin, Game, Open, OpenDecision, on_board};

/// Where a replay and its record parted ([`Game::from_replay`]): answers found away from
/// their recorded index, and places it went on where the rules left nothing to choose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Drift {
    pub(super) moved: u32,
    pub(super) lenient: u32,
}

/// The record's turn-start checkpoints as a replay reaches them ([`Game::from_replay`]).
pub(super) struct Checks<'a> {
    /// Those not reached yet, in order: the board the record was rebuilt from, if it was, then
    /// those its log has.
    pub(super) ahead: VecDeque<&'a Checkpoint>,
    /// Those reached, in order.
    pub(super) reached: Vec<&'a Checkpoint>,
    /// The turns of those dropped unreached, in order.
    pub(super) missed: Vec<u16>,
}

impl<'a> Checks<'a> {
    pub(super) fn new(file: &'a ReplayFile) -> Checks<'a> {
        let mut logged = file
            .log
            .iter()
            .filter_map(|e| match e {
                Entry::Checkpoint(c) => Some(&**c),
                _ => None,
            })
            .peekable();
        // Resuming from a checkpoint records that same opening board in the new log.
        // It is one reached board, not a second turn that exact replay must reach again.
        if logged.peek().copied() == file.from.as_deref() {
            logged.next();
        }
        Checks {
            ahead: file.from.as_deref().into_iter().chain(logged).collect(),
            reached: Vec::new(),
            missed: Vec::new(),
        }
    }

    /// Checks `board`, the board a replay reached as a turn began, against the record's
    /// checkpoint of that turn, if it has one: where they differ, the replay has diverged.
    /// Checkpoints of earlier turns not reached are dropped; an undo past a turn's beginning
    /// begins it again, so a record may have two of a turn, which the replay reaches in turn.
    pub(super) fn reached(
        &mut self,
        t: &GameTable,
        board: &Checkpoint,
    ) -> Result<(), SessionError> {
        while let Some(missed) = self.ahead.pop_front_if(|c| c.turn < board.turn) {
            self.missed.push(missed.turn);
        }
        let Some(recorded) = self.ahead.front().copied().filter(|c| c.turn == board.turn) else {
            return Ok(());
        };
        self.ahead.pop_front();
        self.reached.push(recorded);
        match recorded.difference(t, board) {
            None => Ok(()),
            Some(difference) => Err(SessionError::BadReplay {
                reason: format!(
                    "turn {}: the board as it began is not the record's: {difference}",
                    board.turn
                ),
            }),
        }
    }

    /// `file` rebuilt from the latest checkpoint the replay reached of turn `turn` or before,
    /// where the replay stopped for `reason` in turn `turn`: the board as that turn began, with
    /// no log yet. Fails with `reason` where there is none, or where something lasting past
    /// that turn's beginning off the board would be lost ([`Checkpoint::lingering`]).
    pub(super) fn rebuild(
        &self,
        file: &ReplayFile,
        turn: u16,
        reason: String,
    ) -> Result<ReplayFile, SessionError> {
        let Some(from) = self.reached.iter().rev().find(|c| c.turn <= turn) else {
            return Err(SessionError::BadReplay { reason });
        };
        if let Some(lingering) = &from.lingering {
            return Err(SessionError::BadReplay {
                reason: format!(
                    "{reason}; no rebuild from turn {}'s beginning, which would lose {lingering}",
                    from.turn
                ),
            });
        }
        journal::warn(
            "replay",
            "rebuilt the game from a turn's beginning",
            json!({ "turn": from.turn, "reason": reason }),
        );
        Ok(ReplayFile {
            format_version: FORMAT_VERSION,
            chooser: None,
            from: Some(Box::new((*from).clone())),
            log: Vec::new(),
            ..file.clone()
        })
    }
}

impl Game {
    /// Replays a recorded game to where its record ends, and returns it with the view live
    /// play emitted there: the human's open decision or halt, the agent about to decide, or
    /// the end. [`Game::next_view`] plays on from there.
    ///
    /// Each answer takes the candidate that does what the recorded one did
    /// ([`Chosen::find`]), wherever it now stands. Where the record and the game part, replay
    /// goes on only where the rules leave nothing to choose, journaling each time:
    /// - a decision the record lacks with one candidate (a halt among them) is answered with
    ///   it;
    /// - a priority the record lacks, before the turn and step of its next answer, is passed:
    ///   every decision is recorded, so the original game passed it unasked;
    /// - a recorded priority pass, or halt, that the game no longer asks is skipped, as passing
    ///   changes nothing where no priority is open.
    ///
    /// Anything else fails, naming the turn and step. A version 1 record, of indices alone,
    /// replays as it always did.
    ///
    /// Since version 3, chance has the outcomes the record logged after each entry, while that
    /// entry plays out and play goes on to the next: each hidden card as a player first learns
    /// it is dealt the card the record names, and each coin flip falls as it fell
    /// ([`beryl_engine::chance`]). The seed then decides only what no player has seen. An
    /// outcome the game cannot reproduce, one it makes that the record lacks, or one the record
    /// has that never happens stops the replay. An older record replays from its seed.
    ///
    /// Since version 4, the board as each turn begins is checked against the record's
    /// ([`crate::checkpoint`]): where they differ, the replay stops. Where a replay stops, the
    /// game is rebuilt from the latest board the replay reached as a turn began at or before the
    /// turn it stopped in, journaled, and resumed there: the human plays that turn again. The
    /// rebuilt game's record begins from that board ([`ReplayFile::from`]). Where no board was
    /// reached, or the board's turn had something lasting past its beginning off the board
    /// ([`crate::checkpoint::Checkpoint::lingering`]), the replay fails, saying why.
    pub fn from_replay(
        cards: &'static CardTable,
        file: &ReplayFile,
        session: SessionId,
    ) -> Result<(Game, PlayerView), SessionError> {
        Self::from_replay_with_budget(cards, file, session, Budget::DEFAULT)
    }

    /// Resumes a game with the search budget chosen by its caller.
    pub fn from_replay_with_budget(
        cards: &'static CardTable,
        file: &ReplayFile,
        session: SessionId,
        budget: Budget,
    ) -> Result<(Game, PlayerView), SessionError> {
        let mut game = Game::begin(cards, file, session, budget)?;
        let mut checks = Checks::new(file);
        match game.replay_log(file, &mut checks) {
            Ok(()) => {}
            Err(SessionError::BadReplay { reason }) => {
                let rebuilt = checks.rebuild(file, game.board.turn().number(), reason)?;
                game = Game::begin(cards, &rebuilt, session, budget)?;
                game.replay_log(&rebuilt, &mut Checks::new(&rebuilt))?;
            }
            Err(e) => return Err(e),
        }
        let view = game.view();
        Ok((game, view))
    }

    /// Replays `file` as [`Game::from_replay`] does, but never rebuilds the game from a turn's
    /// beginning: where the record and the game part, a turn's board included, it fails, and
    /// so does a checkpoint of the record the replay never reached. The frozen save corpus is
    /// held to this ([`crate::corpus`]).
    pub fn replay_exactly(
        cards: &'static CardTable,
        file: &ReplayFile,
        session: SessionId,
    ) -> Result<(Game, PlayerView), SessionError> {
        let mut game = Game::begin(cards, file, session, Budget::DEFAULT)?;
        let mut checks = Checks::new(file);
        game.replay_log(file, &mut checks)?;
        if let Some(turn) = checks
            .missed
            .first()
            .or(checks.ahead.front().map(|c| &c.turn))
        {
            return Err(SessionError::BadReplay {
                reason: format!("the record's board as turn {turn} began was never reached"),
            });
        }
        let view = game.view();
        Ok((game, view))
    }

    /// Sets up the game `file` records, before its first entry: as a new game, or as the board
    /// it was rebuilt from ([`ReplayFile::from`]) stood before that turn began.
    pub(super) fn begin(
        cards: &'static CardTable,
        file: &ReplayFile,
        session: SessionId,
        budget: Budget,
    ) -> Result<Game, SessionError> {
        let mut game = Game::with_budget(cards, &file.start()?, session, budget)?;
        let Some(from) = &file.from else {
            return Ok(game);
        };
        let mut state = perturb::new_game(&game.table, file.seed, game.board.profile())?;
        from.arrange(&game.table, &mut state, perturb::seed(file.seed))
            .map_err(|reason| SessionError::BadReplay {
                reason: format!("the checkpoint of turn {}: {reason}", from.turn),
            })?;
        let turn = state.turn();
        game.seen = (turn.number(), turn.step());
        game.board = Boards::new(&state);
        game.flip_winner = None;
        game.replay.chooser = None;
        game.replay.from = Some(from.clone());
        Ok(game)
    }

    /// Replays `file`'s log on the game [`Game::begin`] set up, to where it ends, checking each
    /// turn's board against the record's checkpoint ([`Checks`]).
    pub(super) fn replay_log(
        &mut self,
        file: &ReplayFile,
        checks: &mut Checks,
    ) -> Result<(), SessionError> {
        let chance = file.format_version >= 3;
        if chance {
            // A game rebuilt from a turn's beginning is past the coin flip.
            if let Some(chooser) = file.chooser.filter(|_| file.from.is_none()) {
                let chooser = Seat::try_new(chooser).ok_or_else(|| SessionError::BadReplay {
                    reason: format!("no seat {chooser} won the coin flip"),
                })?;
                self.set_flip_winner(chooser);
            }
            self.chance.replay([]);
        }
        // Each entry replay takes, with the outcomes of chance logged after it, and those logged
        // before the first. The session's own answers are found again as it plays on, and
        // checkpoints are checked as each turn begins.
        let mut before = Vec::new();
        let mut steps: Vec<(&Entry, Vec<Random>)> = Vec::new();
        for entry in &file.log {
            match entry.random(&self.table)? {
                Some(outcome) => match steps.last_mut() {
                    Some((_, outcomes)) => outcomes.push(outcome),
                    None => before.push(outcome),
                },
                // Damage assignments and targets are recorded choices, even when the
                // session took them. Replaying them preserves earlier manual choices.
                None if matches!(entry, Entry::Auto(c)
                if c.what.as_ref().is_some_and(|w| {
                    matches!(w.kind, save::Kind::DamageDivision | save::Kind::Target)
                })) =>
                {
                    steps.push((entry, Vec::new()));
                }
                None if matches!(entry, Entry::Auto(_) | Entry::Checkpoint(_)) => {}
                None => steps.push((entry, Vec::new())),
            }
        }
        if chance {
            self.chance.replay(before);
        }
        let mut steps = steps.into_iter().peekable();
        let mut drift = Drift::default();
        // Whether chance has the outcomes logged after the next entry.
        let mut dealt = false;
        loop {
            if matches!(self.at, At::Playing) {
                let logged = self.replay.log.len();
                self.play_on(true);
                self.chance_reproduced()?;
                if let Some(Entry::Checkpoint(reached)) = self
                    .replay
                    .log
                    .get(logged..)
                    .and_then(|new| new.iter().find(|e| matches!(e, Entry::Checkpoint(_))))
                {
                    checks.reached(&self.table, reached)?;
                }
                if matches!(self.at, At::Playing) {
                    // A step began, and live play went on after its view.
                    self.skip_events();
                    continue;
                }
            }
            if self.replay_implicit_waterbend(file, steps.peek().map(|(e, _)| *e)) {
                self.chance_reproduced()?;
                continue;
            }
            // Before v7, an interrupted cast could have exile picks but no mana plan yet.
            // Stop at the newly required payment instead of inventing its answer. The v7
            // migration re-records this prefix, restoring those unfinished exile picks.
            if file.format_version < 7
                && self.unpaid_cast()
                && steps.clone().all(|(entry, _)| {
                    matches!(entry, Entry::Choice(c)
                    if c.what.as_ref().is_some_and(|w| w.before_v10 == Some(KindBeforeV10::ExileCost)))
                })
            {
                break;
            }
            let Some((entry, outcomes)) = steps.peek_mut() else {
                break;
            };
            // The entry before should have played out, with every outcome logged after it. Where
            // some are still to come, the open decision may be one the record lacks, such as a
            // priority in the upkeep before a recorded draw's reveal: answering it plays on to
            // them. Only taking the entry with outcomes still to come is an error.
            let unspent = (!dealt).then(|| self.chance_spent().err()).flatten();
            if !dealt && unspent.is_none() {
                if chance {
                    self.chance.replay(std::mem::take(outcomes));
                }
                dealt = true;
            }
            let entry: &Entry = entry;
            // Live play's view here took the events so far.
            self.skip_events();
            let took = self.replay_entry(entry, &mut drift)?;
            if took && let Some(unspent) = unspent {
                return Err(unspent);
            }
            self.chance_reproduced()?;
            if took {
                steps.next();
                dealt = false;
            }
        }
        self.chance_spent()?;
        self.chance.stop_replaying();
        self.screens = Default::default();
        if drift != Drift::default() {
            journal::info(
                "replay",
                "the record replayed with drift",
                json!({ "moved": drift.moved, "lenient": drift.lenient }),
            );
        }
        Ok(())
    }

    /// Before v8, a required singleton Waterbend answer was made inside the engine,
    /// without a log entry. Restore it here without consuming the next recorded answer.
    pub(super) fn replay_implicit_waterbend(
        &mut self,
        file: &ReplayFile,
        next: Option<&Entry>,
    ) -> bool {
        let At::Human(id, open @ Open::Decision(d)) = &self.at else {
            return false;
        };
        if d.prompt.purpose != Purpose::Waterbend {
            return false;
        }
        let lone_bender = (file.legacy_waterbend || file.format_version < 8)
            && without_floating(d.clone()).len() == 1;
        // A v8 payment with only nonland mana to activate never opened the picker at all.
        let nonland_only = (file.legacy_waterbend_mana || file.format_version < 9)
            && on_board!(&self.board, b => only_nonland_mana(&b.state, &d.relayout()));
        if !lone_bender && !nonland_only {
            return false;
        }
        if matches!(next, Some(Entry::Choice(c)) if c.what.as_ref()
            .is_some_and(|w| w.before_v10 == Some(KindBeforeV10::Waterbend)))
        {
            return false;
        }
        let answer = OpenDecision::new(*id, open, false, false)
            .answer(*id, CandidateIndex(0))
            .expect("waterbend has one non-mana answer");
        self.choose(answer.as_continuation());
        true
    }

    /// Settles the coin flip of [CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)
    /// as a record says: `chooser` won it and chooses to play or draw.
    pub(super) fn set_flip_winner(&mut self, chooser: Seat) {
        on_board!(&mut self.board, b => b.state.board().set_flip_winner(chooser));
        self.flip_winner = Some(chooser);
        self.replay.chooser = Some(chooser.get());
    }

    /// Whether replay is waiting for the mana choice of an unfinished cast.
    pub(super) fn unpaid_cast(&self) -> bool {
        matches!(&self.at,
            At::Agent(d) | At::Human(_, Open::Decision(d))
                if d.prompt.purpose == Purpose::Mana)
    }

    /// Fails where a replay's chance could not reproduce what its record says chance produced.
    pub(super) fn chance_reproduced(&mut self) -> Result<(), SessionError> {
        let Some(m) = self.chance.take_mismatch() else {
            return Ok(());
        };
        let Mismatch {
            turn,
            step,
            recorded,
            happened,
        } = m;
        let what = match (recorded, happened) {
            (None, happened) => format!("{} that the record lacks", self.outcome(happened)),
            (
                Some(Random::Reveal { owner, card }),
                Random::Reveal {
                    owner: revealed, ..
                },
            ) if owner == revealed => format!(
                "the record reveals player {}'s {} where no card of theirs that no player knows \
                 can be it",
                owner.get(),
                self.table.def(card).name
            ),
            (Some(recorded), happened) => format!(
                "the record has {} where the game has {}",
                self.outcome(recorded),
                self.outcome(happened)
            ),
        };
        Err(SessionError::BadReplay {
            reason: format!("turn {turn} {step:?}: {what}"),
        })
    }

    /// Fails where a replay's record has an outcome of chance that never happened.
    pub(super) fn chance_spent(&self) -> Result<(), SessionError> {
        match self.chance.next_recorded() {
            Some(outcome) => Err(self.parted(&format!(
                "the record has {} that never happened",
                self.outcome(outcome)
            ))),
            None => Ok(()),
        }
    }

    /// An outcome of chance, in words.
    pub(super) fn outcome(&self, outcome: Random) -> String {
        match outcome {
            Random::Reveal { owner, card } => format!(
                "player {}'s {} revealed",
                owner.get(),
                self.table.def(card).name
            ),
            Random::Flip { player, won } => format!(
                "a coin flip player {} {}",
                player.get(),
                if won { "won" } else { "lost" }
            ),
        }
    }

    /// Plays the record's next entry, `entry`, at the game's open decision or halt. Returns
    /// whether it took the entry: false where it answered a decision the record lacks.
    pub(super) fn replay_entry(
        &mut self,
        entry: &Entry,
        drift: &mut Drift,
    ) -> Result<bool, SessionError> {
        match (self.at.clone(), entry) {
            (At::Agent(d), Entry::Choice(chosen)) => {
                let Some(c) = self.found(&d, chosen, drift) else {
                    return self.lenient(&d, chosen, drift);
                };
                self.agent_decides(c);
            }
            (At::Human(_, Open::Decision(d)), Entry::Auto(chosen)) => {
                let Some(c) = self.found(&d, chosen, drift) else {
                    return self.lenient(&d, chosen, drift);
                };
                self.apply(&d, c, ChoiceOrigin::Auto);
                self.answered = Some(self.board.stack());
                self.at = At::Playing;
            }
            (At::Human(id, open @ Open::Decision(_)), Entry::Choice(chosen)) => {
                let Open::Decision(d) = &open else {
                    unreachable!("matched a decision")
                };
                let Some(c) = self.found(d, chosen, drift) else {
                    return self.lenient(d, chosen, drift);
                };
                let answer = OpenDecision::new(id, &open, false, false)
                    .answer(id, CandidateIndex(c.0))
                    .expect("a found candidate is in range");
                // The record says whether it joined the answer before it: a later pick of a
                // prompt's run did, in a cast or out of one.
                self.answer_as(answer, chosen.continuation);
            }
            (At::Human(..), Entry::Stops(stops)) => self.change_stops(*stops),
            (At::Human(_, Open::Decision(_)), Entry::PassOn) => {
                if !self.start_passing() {
                    return Err(self.parted("a pass on where there is none"));
                }
            }
            (At::Human(_, Open::Halt), Entry::Halt) => self.answer_first(),
            (At::Human(..), Entry::Undo) => {
                self.undo()
                    .map_err(|_| self.parted("an undo with nothing to take back"))?;
            }
            (At::Agent(_) | At::Human(..), Entry::Concede) => {
                self.concede();
            }
            (At::Over { .. }, _) => return Err(self.parted("the log goes on after the game ended")),
            // A halt the record lacks: the human goes on from it, their one candidate.
            (At::Human(_, Open::Halt), _) => {
                self.lenient_note("went on from a halt the record lacks", entry, drift);
                self.answer_first();
                return Ok(false);
            }
            // A recorded halt that no longer happens: going on from it changed nothing.
            (At::Agent(_) | At::Human(..), Entry::Halt) => {
                self.lenient_note("skipped a halt the game no longer makes", entry, drift);
            }
            (_, entry) => {
                return Err(self.parted(&format!("{entry:?} where the game does not ask it")));
            }
        }
        Ok(true)
    }

    /// The candidate of `d` that does what `chosen` did, if any; one found elsewhere than its
    /// recorded index counts as drift.
    pub(super) fn found(
        &mut self,
        d: &Decision<Large>,
        chosen: &Chosen,
        drift: &mut Drift,
    ) -> Option<Choice> {
        let c = on_board!(&mut self.board, b => chosen.find(
            &self.table,
            view(&b.state, &self.table, &mut b.peek),
            &d.relayout(),
        ))?;
        if c.0 != chosen.index {
            drift.moved += 1;
        }
        Some(c)
    }

    /// Goes on where no candidate of `d` does what `chosen` did, if the rules leave nothing to
    /// choose ([`Game::from_replay`]). Returns whether it took the entry.
    pub(super) fn lenient(
        &mut self,
        d: &Decision<Large>,
        chosen: &Chosen,
        drift: &mut Drift,
    ) -> Result<bool, SessionError> {
        let entry = Entry::Choice(chosen.clone());
        if d.len() == 1 {
            self.lenient_note("answered a decision the record lacks", &entry, drift);
            self.answer_first();
            return Ok(false);
        }
        let now = on_board!(&self.board, b => save::when(&b.state));
        let when = chosen.when();
        // Every decision is recorded: one before the record's next, which the original game
        // never asked, is a priority it passed unasked.
        if d.kind == DecisionKind::Priority && when.is_some_and(|w| w > now) {
            self.lenient_note("passed a priority the record lacks", &entry, drift);
            debug_assert_eq!(d.candidates.option(Choice(0)), Some(Opt::Pass));
            self.answer_first();
            return Ok(false);
        }
        if chosen.is_priority_pass()
            && (d.kind != DecisionKind::Priority || when.is_some_and(|w| w < now))
        {
            self.lenient_note("skipped a pass where no priority is open", &entry, drift);
            return Ok(true);
        }
        Err(self.parted(&format!(
            "no candidate of player {}'s {:?} decision does {}",
            d.player,
            d.kind,
            serde_json::to_string(chosen).expect("an answer serializes"),
        )))
    }

    /// Answers the open decision, the agent's or the human's, or the human's halt, with its
    /// first candidate.
    pub(super) fn answer_first(&mut self) {
        match self.at.clone() {
            At::Human(id, open) => {
                let answer = OpenDecision::new(id, &open, false, false)
                    .answer(id, CandidateIndex(0))
                    .expect("a decision has a first candidate");
                self.choose(answer);
            }
            At::Agent(_) => self.agent_decides(Choice(0)),
            _ => unreachable!("answered at a decision"),
        }
    }

    /// Journals a place where replay went on though the record and the game parted.
    pub(super) fn lenient_note(&self, what: &str, entry: &Entry, drift: &mut Drift) {
        drift.lenient += 1;
        let turn = self.board.turn();
        journal::info(
            "replay",
            what,
            json!({ "turn": turn.number(), "step": format!("{:?}", turn.step()), "entry": entry }),
        );
    }

    /// Why a replay stops where the record and the game parted: `what`, at this turn and step.
    pub(super) fn parted(&self, what: &str) -> SessionError {
        let turn = self.board.turn();
        SessionError::BadReplay {
            reason: format!("turn {} {:?}: {what}", turn.number(), turn.step()),
        }
    }
}
