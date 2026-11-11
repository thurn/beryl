//! An app game's record replayed decision by decision on the engine, as `beryl_session`'s
//! `Game::replay` plays it: stops changes, halts, the human's passes and undos included. Each
//! decision comes with the answer the record gives it; the caller looks, then applies it.
//! [`crate::review`] writes transcripts from it and [`crate::blunders`] reaches the positions
//! of the blunder suite with it.

use std::collections::VecDeque;

use beryl_cards::decks::WU_CONTROL;
use beryl_engine::event::Event;
use beryl_engine::{
    Choice, Decision, GameTable, Outcome, Profile, Replay, Scratch, Seat, State, Step, apply, next,
    view,
};
use beryl_session::replay::Entry;
use beryl_session::{ReplayFile, StopsSetting, UNDO_POINTS, with_stops};

use crate::game;

/// What the record does next.
#[allow(clippy::large_enum_variant, reason = "one is alive at a time")]
pub enum Walked {
    /// Decision `d` is open and the record answers it with `c`; `happened` since the last.
    Decide {
        d: Decision,
        c: Choice,
        happened: Vec<Event>,
    },
    /// The human took back an answer: the game stands where they gave it.
    Undone,
    Conceded,
    /// The record ends before the game does.
    Ends,
    Over {
        outcome: Outcome,
        happened: Vec<Event>,
    },
}

/// A record being replayed.
pub struct Walk {
    pub t: GameTable,
    pub s: State,
    pub x: Scratch,
    pub human: Seat,
    log: VecDeque<Entry>,
    stops: StopsSetting,
    /// The positions an undo goes back to, one per answer of the human's, the latest last, as
    /// `Game::undo` keeps them: the state at each decision or halt they answered, the oldest
    /// dropped beyond `UNDO_POINTS`.
    points: VecDeque<State>,
}

impl Walk {
    /// The record of `file`, from its seed: a WU Control mirror played from its start.
    pub fn of_file(file: &ReplayFile) -> Result<Walk, String> {
        let mirror: Vec<(u8, String)> = WU_CONTROL
            .iter()
            .map(|&(n, name)| (n, name.to_owned()))
            .collect();
        if file.decks.iter().any(|d| *d != mirror) {
            return Err("only WU Control mirror games can be replayed".to_owned());
        }
        if file.from.is_some() {
            return Err("a game rebuilt from a turn's beginning cannot be replayed".to_owned());
        }
        let human = Seat::try_new(file.human).ok_or("no such human seat")?;
        let t = game::mirror();
        let s = State::new(&t, file.seed, with_stops(Profile::SMART, file.stops, human))
            .map_err(|e| e.to_string())?;
        // Each turn's board, as it began, is the session's to check.
        let log = file
            .log
            .iter()
            .filter(|e| !matches!(e, Entry::Checkpoint(_)))
            .cloned()
            .collect();
        Ok(Walk::new(t, &s, log, file.stops, human))
    }

    /// The record `log` from `s`, a state of the game of `t` the human in seat `human` played
    /// under `stops`.
    pub fn new(t: GameTable, s: &State, log: Vec<Entry>, stops: StopsSetting, human: Seat) -> Walk {
        Walk {
            t,
            s: *s,
            x: Scratch::recording(),
            human,
            log: log.into(),
            stops,
            points: VecDeque::new(),
        }
    }

    /// Plays on to the record's next decision, or whatever comes first. A decision's answer
    /// is the caller's to [`Walk::apply`].
    pub fn next(&mut self) -> Result<Walked, String> {
        let d = match next(&mut self.s, &self.t, &mut self.x, &mut Replay) {
            Step::Over(outcome) => {
                let happened = self.x.events.drain().collect();
                return Ok(Walked::Over { outcome, happened });
            }
            Step::Decide(d) => d,
        };
        // As `beryl_session`'s `Game::replay`: a stops change takes effect at the decision
        // open when it was made, and a halt is the session's, not the engine's.
        let before = self.s;
        let entry = loop {
            match self.log.pop_front() {
                Some(Entry::Stops(changed)) => {
                    self.stops = changed;
                    self.s
                        .set_profile(with_stops(self.s.profile(), changed, self.human));
                }
                // A halt is the session's: only decisionless play separates it from this
                // decision.
                Some(Entry::Halt) => self.keep(&before),
                // The session passes for the human after it; each pass is logged. The replay
                // plays from the seed on the build that wrote the file, which deals and flips
                // as the game did.
                Some(Entry::PassOn | Entry::Reveal { .. } | Entry::Flip { .. }) => {}
                Some(Entry::Concede) => return Ok(Walked::Conceded),
                entry => break entry,
            }
        };
        if matches!(entry, Some(Entry::Choice(_))) && d.player == self.human {
            self.keep(&before);
        }
        let c = match entry {
            Some(Entry::Choice(chosen) | Entry::Auto(chosen)) => chosen
                .find(&self.t, view(&self.s, &self.t, &mut self.x), &d)
                .ok_or_else(|| {
                    let shown =
                        crate::transcript::labels(&self.t, view(&self.s, &self.t, &mut self.x), &d);
                    format!(
                        "no candidate of player {}'s {:?} ({}) does what the record did: {chosen:?}",
                        d.player.get(),
                        d.kind,
                        shown.join(" | ")
                    )
                })?,
            Some(Entry::Undo) => {
                let Some(point) = self.points.pop_back() else {
                    return Err("an undo with nothing to take back".to_owned());
                };
                // Stops changed since stay changed, as `Game::undo` keeps them.
                self.s = point;
                self.s
                    .set_profile(with_stops(self.s.profile(), self.stops, self.human));
                self.x.events.drain().for_each(drop);
                return Ok(Walked::Undone);
            }
            _ => return Ok(Walked::Ends),
        };
        let happened = self.x.events.drain().collect();
        Ok(Walked::Decide { d, c, happened })
    }

    /// Applies answer `c` to the open decision.
    pub fn apply(&mut self, c: Choice) {
        apply(&mut self.s, &self.t, &mut self.x, &mut Replay, c);
    }

    fn keep(&mut self, point: &State) {
        self.points.push_back(*point);
        if self.points.len() > UNDO_POINTS {
            self.points.pop_front();
        }
    }
}
