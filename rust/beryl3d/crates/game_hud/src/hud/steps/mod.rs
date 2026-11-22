//! The turn step display and the stops sent from its controls.

use std::collections::VecDeque;
use std::convert::Infallible;

use beryl_protocol::protocol::{DecisionId, SessionId, Side, Step, StepSet, StopsSetting};
use beryl_protocol::script::Turn;
use glam::Vec2;
use kit_base::data::{hud::StepTrack, profiles::Piece};
use kit_scene::{Context, Feature, StateUpdate};
use kit_ui::element::{Element, row};
use kit_ui::layout::Layer;

use super::HudIn;
use crate::inspect::Viewed;
use crate::screens::opening;

mod view;
use view::game_steps;
pub use view::table_tracks;

/// A press on a stop in the turn step display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    Toggle { step: Step, theirs: bool },
}

/// Kept between frames so playback can swap in a row without building one.
#[derive(Debug, Default)]
pub struct State {
    pub turn: Option<Turn>,
    ahead: Vec<(Turn, Element)>,
    coming: Vec<Turn>,
    from: Option<RowsFrom>,
    outstanding: VecDeque<StopsSetting>,
    pending_at: Option<(SessionId, DecisionId)>,
}

type RowsFrom = (
    StopsSetting,
    [Vec2; 2],
    Option<Viewed>,
    bool,
    bool,
    StepTrack,
);
type Lit = (Side, Step);

/// The desktop step row, with stops the player can toggle.
pub struct StepRow;

impl Feature for StepRow {
    const SOURCE: &'static str = file!();

    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs;
    type ViewKey = (RowsFrom, Option<Lit>);
    type Command = Infallible;
    type Request = Message;

    fn update(
        _: &mut State,
        message: &Message,
        context: &mut Context<'_, Infallible, Message>,
    ) -> bool {
        context.request(*message);
        false
    }

    fn view_key(_: &State, input: &Inputs) -> Self::ViewKey {
        (input.from(), input.current())
    }

    fn view(_: &State, input: Inputs) -> Element {
        game_steps(&input).unwrap_or_else(row)
    }
}

/// The inputs the desktop step row reads.
#[derive(Clone, Copy)]
pub struct Inputs {
    centres: [Vec2; 2],
    tuning: StepTrack,
    enabled: bool,
    pub latest: Option<Lit>,
    pub turn: Option<Turn>,
    pub stops: StopsSetting,
    pub viewing: Option<Viewed>,
    pub opening: bool,
}

impl Inputs {
    #[must_use]
    pub fn new(input: &HudIn<'_>) -> Self {
        let enabled = input.profile.data().hud.table.contains(&Piece::StepStops);
        let centres = if enabled {
            [Side::You, Side::Opponent].map(|side| super::players::face_centre(side, input.profile))
        } else {
            [Vec2::ZERO; 2]
        };
        Self {
            centres,
            tuning: input.data.hud.steps,
            enabled,
            latest: input.view.map(|view| (view.turn.active, view.turn.step)),
            turn: input.turn,
            stops: input.stops,
            viewing: input.viewing,
            opening: opening::shown(input),
        }
    }

    #[must_use]
    pub fn visible(self) -> bool {
        self.latest.is_some() && self.enabled && self.viewing.is_none()
    }

    fn from(self) -> RowsFrom {
        (
            self.stops,
            self.centres,
            self.viewing,
            self.opening,
            self.enabled,
            self.tuning,
        )
    }

    fn current(self) -> Option<Lit> {
        self.turn.map(lit).or(self.latest)
    }
}

fn lit(turn: Turn) -> Lit {
    (turn.active, turn.step)
}

/// The step row as it appears within the game screen.
#[must_use]
pub fn steps(input: &HudIn<'_>) -> Option<Element> {
    game_steps(&Inputs::new(input))
}

/// Synchronizes the settings sent and acknowledged at the open decision.
pub enum SyncStops {
    Receive {
        at: Option<(SessionId, DecisionId)>,
        held: StopsSetting,
    },
    Send {
        held: StopsSetting,
        want: StopsSetting,
    },
}

impl StateUpdate<StepRow> for SyncStops {
    type Output = bool;

    fn apply(self, state: &mut State) -> (bool, bool) {
        let send = match self {
            Self::Receive { at, held } => {
                state.receive(at, held);
                false
            }
            Self::Send { held, want } => state.send(held, want),
        };
        (false, send)
    }
}

/// Prepares the step rows that upcoming turns can show while the game is open.
pub struct Prepare<Coming> {
    pub input: Option<Inputs>,
    pub coming: Coming,
}

impl<Coming: Iterator<Item = Turn>> StateUpdate<StepRow> for Prepare<Coming> {
    type Output = ();

    fn apply(self, state: &mut State) -> (bool, ()) {
        if let Some(input) = self.input {
            state.prepare(&input, self.coming);
        } else {
            state.ahead.clear();
        }
        (false, ())
    }
}

/// Samples the current turn step and replaces the step row if it changed.
pub struct Draw<'a> {
    pub layer: &'a mut Layer,
    pub turn: Option<Turn>,
    pub input: Inputs,
    pub game: bool,
}

impl StateUpdate<StepRow> for Draw<'_> {
    type Output = ();

    fn apply(self, state: &mut State) -> (bool, ()) {
        state.write(self.layer, self.turn, &self.input, self.game);
        (false, ())
    }
}

impl State {
    /// Drops acknowledged settings in send order. An update for a new
    /// decision supersedes requests made at the previous decision.
    fn receive(&mut self, at: Option<(SessionId, DecisionId)>, held: StopsSetting) {
        if self.pending_at != at {
            self.outstanding.clear();
            self.pending_at = at;
        } else if self.outstanding.front() == Some(&held) {
            self.outstanding.pop_front();
        }
    }

    /// A setting different from the latest send or the latest session view
    /// needs a command. Only sent settings awaiting a view are kept here.
    fn send(&mut self, held: StopsSetting, want: StopsSetting) -> bool {
        if self.outstanding.back().copied().unwrap_or(held) == want {
            return false;
        }
        self.outstanding.push_back(want);
        true
    }

    /// Builds each row that an upcoming script will show and that has no
    /// prepared row yet, and drops rows no upcoming script can reach.
    fn prepare(&mut self, input: &Inputs, coming: impl Iterator<Item = Turn>) {
        let from = input.from();
        if self.from != Some(from) {
            self.from = Some(from);
            self.ahead.clear();
        }
        let shown = self.turn.map(lit).or(input.latest);
        self.coming.clear();
        for turn in coming {
            if Some(lit(turn)) != shown && !self.coming.iter().any(|&other| lit(other) == lit(turn))
            {
                self.coming.push(turn);
            }
        }
        self.ahead
            .retain(|&(turn, _)| self.coming.iter().any(|&shown| lit(shown) == lit(turn)));
        for &turn in &self.coming {
            if self.ahead.iter().any(|&(shown, _)| lit(shown) == lit(turn)) {
                continue;
            }
            let variant = Inputs {
                turn: Some(turn),
                ..*input
            };
            if let Some(steps) = game_steps(&variant) {
                self.ahead.push((turn, steps));
            }
        }
    }

    /// Changes just the step row as a script reaches a new turn step.
    fn write(&mut self, layer: &mut Layer, turn: Option<Turn>, input: &Inputs, game: bool) {
        if turn == self.turn {
            return;
        }
        let latest = input.latest;
        let shows = turn.map(lit).or(latest);
        let changed = shows != self.turn.map(lit).or(latest);
        self.turn = turn;
        if !changed || !game {
            return;
        }
        let steps = match self
            .ahead
            .iter()
            .position(|(turn, _)| Some(lit(*turn)) == shows)
        {
            Some(index) => Some(self.ahead.swap_remove(index).1),
            None => game_steps(&Inputs { turn, ..*input }),
        };
        if let Some(steps) = steps {
            layer.replace(steps);
        }
    }
}

/// `set` with `step` added if it was not in it, or taken out if it was, together with the other
/// combat damage step when it is one of the two.
#[must_use]
pub fn toggled(set: StepSet, step: Step) -> StepSet {
    // Combat's two damage steps share one stop, as the web's `STEP_STOPS` has it.
    let group: &[Step] = match step {
        Step::FirstStrikeDamage | Step::CombatDamage => {
            &[Step::FirstStrikeDamage, Step::CombatDamage]
        }
        _ => &[step],
    };
    let mut steps: Vec<Step> = set.steps().map(Step::from).collect();
    if group.iter().any(|step| steps.contains(step)) {
        steps.retain(|held| !group.contains(held));
    } else {
        steps.extend_from_slice(group);
    }
    let steps: Vec<_> = steps.into_iter().map(Into::into).collect();
    StepSet::of(&steps)
}
