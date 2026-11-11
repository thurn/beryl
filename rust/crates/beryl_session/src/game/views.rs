//! Drawing the views of the game that the human and the agent's seat are shown.

use beryl_engine::event::{Event, Recipient};
use beryl_engine::state::Pending;
use beryl_engine::{
    Large, Observation, Outcome, Relayout, Seat, paused_events, two_player::TwoPlayerSeat as _,
    view,
};

use crate::preview;
use crate::protocol::{Moment, PlayerView, Side, StackItem, Status, player_view};

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
        let paused: Vec<Event<Large>> = on_board!(&mut self.board, b => {
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
        for e in &fresh {
            match *e {
                Event::Damage {
                    source,
                    to: Recipient::Player(player),
                    ..
                } => self.hurt_by[player.index()] = Some(source),
                Event::LifeLoss { player, .. } => self.hurt_by[player.index()] = None,
                _ => {}
            }
        }
        // The loser's cause, from the event that ended the game.
        if let At::Over {
            outcome: Outcome::Win(winner),
            cause: cause @ None,
            hurt_by,
        } = &mut self.at
        {
            let loser = winner.opponent();
            *cause = fresh.iter().find_map(|e| match *e {
                Event::Lose { player, cause } if player == loser => Some(cause),
                _ => None,
            });
            *hurt_by = self.hurt_by[loser.index()];
        }
        self.events = fresh;
        self.paused = paused;
        self.draw()
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
        let resolving = self
            .board
            .resolving()
            .then(|| self.board.stack().pop())
            .flatten();
        let reviewing = self.reviewing();
        let mut view = on_board!(&mut self.board, b => {
            // The session's decision, stack entry and events, in the game's layout.
            let decision = match &self.at {
                At::Agent(d) | At::Human(_, Open::Decision(d)) => Some(d.relayout()),
                _ => None,
            };
            let resolving = resolving.as_ref().map(Relayout::relayout);
            let events: Vec<_> = self.events.iter().map(Relayout::relayout).collect();
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
            // Let the opponent watch the Waterbend payment before the activation
            // lifts from its source. The controller's payment view still needs the
            // pending stack item to identify what they are paying for.
            if matches!(shown.pending(), Pending::Casting(_))
                && matches!(
                    view.stack.last(),
                    Some(StackItem::Ability {
                        controller: Side::Opponent,
                        waterbend: Some(true),
                        ..
                    })
                )
            {
                view.stack.pop();
            }
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
            view
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
        view.script = Some(self.screens[viewer.index()].script(&view, resolving.is_some()));
        view
    }
}
