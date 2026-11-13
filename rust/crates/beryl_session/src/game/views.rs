//! Drawing the views of the game that the human and the agent's seat are shown.

use beryl_engine::event::{Event, Recipient, Tagged};
use beryl_engine::{
    Large, Observation, Outcome, Relayout, Seat, paused_events, two_player::TwoPlayerSeat as _,
    view,
};

use crate::preview;
use crate::protocol::{
    Moment, PlayerView, Side, Status, heard, hold_back_activation_being_paid, player_view,
};
use crate::script::Subject;

use super::{At, Boards, Game, Open, on_board};

impl Game {
    /// The human's view now, with the engine's events since the previous view. A view in a
    /// paused resolution shows its body done up to its question (§7.3), so it reports the
    /// events of that, once: the card a loot draws comes into the hand before the discard is
    /// asked, not after. The body executes them again whenever it runs on, after an answer,
    /// and those repeats go unreported.
    pub(super) fn view(&mut self) -> PlayerView {
        let mut fresh = self.board.drain_events();
        let seen = self.paused.len();
        if seen > 0
            && let Some(at) = fresh.windows(seen).position(|w| w == self.paused)
        {
            fresh.drain(at..at + seen);
        }
        let paused: Vec<Tagged<Large>> = on_board!(&mut self.board, b => {
            paused_events(&b.state, &self.table, &mut b.scratch)
                .iter()
                .map(Relayout::relayout)
                .collect()
        });
        fresh.extend_from_slice(
            paused
                .strip_prefix(self.paused.as_slice())
                .unwrap_or(&paused),
        );
        self.note_hurt(&fresh);
        // The loser's cause, from the event that ended the game.
        if let At::Over {
            outcome: Outcome::Win(winner),
            cause: cause @ None,
            hurt_by,
        } = &mut self.at
        {
            let loser = winner.opponent();
            *cause = fresh.iter().find_map(|e| match e.event {
                Event::Lose { player, cause } if player == loser => Some(cause),
                _ => None,
            });
            *hurt_by = self.hurt_by[loser.index()];
        }
        self.said = self.names.follow(&fresh);
        self.events = fresh;
        self.paused = paused;
        self.draw()
    }

    /// Keeps, for each player, what dealt the damage that last lowered their life in `events`,
    /// which names the culprit should they lose by it.
    fn note_hurt(&mut self, events: &[Tagged<Large>]) {
        for e in events {
            match e.event {
                Event::Damage {
                    source,
                    to: Recipient::Player(player),
                    ..
                } => self.hurt_by[player.index()] = Some(source),
                Event::LifeLoss { player, .. } => self.hurt_by[player.index()] = None,
                _ => {}
            }
        }
    }

    /// Drops the engine's events since the last view, as a replay does where live play drew a
    /// view, still noting what hurt each player ([`Game::note_hurt`]): a game replayed to its
    /// end names the culprit of its loss as live play did.
    pub(super) fn skip_events(&mut self) {
        let events = self.board.drain_events();
        self.note_hurt(&events);
    }

    /// The human's view now, with the events of the latest view.
    pub(super) fn draw(&mut self) -> PlayerView {
        let view = self.draw_for(self.human);
        self.passes_on = self.pass_on_target();
        view
    }

    pub(super) fn draw_for(&mut self, viewer: Seat) -> PlayerView {
        // A paused resolution's entry is still on top of the committed stack; the position
        // replayed to its question has taken it off.
        let mut committed = self.board.stack();
        let resolving = self.board.resolving().then(|| committed.pop()).flatten();
        // Its place among the stack's entries, which it keeps until its resolution ends
        // (`StackIx`).
        let place = resolving.as_ref().map(|_| committed.len());
        let reviewing = self.reviewing();
        let (mut view, heard) = on_board!(&mut self.board, b => {
            // The session's decision, stack entry and events, in the game's layout.
            let decision = match &self.at {
                At::Agent(d) | At::Human(_, Open::Decision(d)) => Some(d.relayout()),
                _ => None,
            };
            let resolving = resolving.as_ref().map(Relayout::relayout);
            let events: Vec<Event<_>> = self.events.iter().map(|e| e.event.relayout()).collect();
            let moment = match (&self.at, &decision) {
                (At::Playing, _) => Moment::Playing,
                (At::Human(id, Open::Decision(_)), Some(d)) if d.player == viewer => {
                    Moment::Deciding(*id, d, resolving.as_ref())
                }
                (At::Human(id, Open::Halt), _) if viewer == self.human => Moment::Halted(*id),
                (At::Agent(_) | At::Human(..), _) => Moment::Thinking {
                    resolving: resolving.as_ref(),
                    decision: decision.as_ref(),
                },
                (
                    At::Over {
                        outcome,
                        cause,
                        hurt_by,
                    },
                    _,
                ) => Moment::Over {
                    outcome: *outcome,
                    cause: cause.as_ref().map(Relayout::relayout),
                    hurt_by: hurt_by.as_ref().map(Relayout::relayout),
                },
                (At::Stopped, _) => Moment::Stopped,
            };
            let shown = view(&b.state, &self.table, &mut b.scratch);
            let obs = Observation::new(shown, viewer);
            let mut view = player_view(&self.table, obs, moment, self.stops, &events);
            // The stack as the engine's events name it: what the position shows, and the
            // resolving entry where it was.
            let below = place.unwrap_or(shown.stack().len()).min(shown.stack().len());
            let (under, over) = shown.stack().split_at(below);
            self.names.agree(under.iter().chain(&resolving).chain(over));
            let mut names: Vec<Subject> = self.names.stack().to_vec();
            if place.is_some() {
                let asking = names.remove(below);
                names.push(asking);
            }
            let held = view.stack.len();
            hold_back_activation_being_paid(&mut view, shown);
            if view.stack.len() < held {
                names.remove(view.stack.len());
            }
            let heard = heard(&self.table, obs, &events, &self.said, names);
            if let Some(d) = &decision
                && d.player == viewer
            {
                preview::add_previews(&mut view, &b.state, &self.table, d);
            }
            if let Status::Deciding { decision } = &mut view.status
                && reviewing
            {
                decision.pass_on = None;
            }
            let asked = decision.as_ref().filter(|d| d.player == viewer);
            super::prompts::hold(&mut self.prompt, &mut view, &b.state, shown, &self.table, asked);
            (view, heard)
        });
        view.turn.flip_winner = self.flip_winner.map(|p| {
            if p == viewer {
                Side::You
            } else {
                Side::Opponent
            }
        });
        if let Status::Deciding { decision } = &mut view.status {
            decision.cancellable = self.cancellable();
            decision.undo = if self.undoable() {
                u16::try_from(self.undo.len()).unwrap_or(u16::MAX)
            } else {
                0
            };
            decision.undo_depth = u32::try_from(self.undo_depth()).unwrap_or(u32::MAX);
        }
        view.script = Some(self.screens[viewer.index()].script(&view, &heard));
        view
    }
}
