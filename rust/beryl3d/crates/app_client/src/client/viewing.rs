//! The viewer of a pile's cards (`inspect::viewer`) and the Revealed Cards pill
//! (`inspect::revealed`): which pile's viewer is open, the cards shown in its boxes, which the
//! table moves there and back (its Show relations), and the pill, shown whenever the Revealed Cards
//! pile holds a face.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{Declaration, ObjectId, Side, Step, StopsSetting, ZoneName};
use beryl_protocol::script::{Act, OwnStep, Turn};
use game_hud::inspect::Viewed;
use glam::Vec2;

use super::Client;
use app_screens::screens::{Screen, ScreenIn};
use game_hud::inspect::revealed::{self, RevealedFace};
use game_play::light::Span;
use game_play::verbs::Interval;
use game_table::table::{Moves, Relaid, Window};
use kit_base::data::space::shown::opening;
use kit_base::pose::Curve;
use kit_frame::frame::Place;
use kit_scene::{ShowPlacement, Tree};
use kit_ui::element::{Element, floats};
use kit_ui::layout::FloatLook;

/// Nanoseconds in a millisecond, as the session's step timings are written in milliseconds.
const NS_PER_MS: f64 = 1_000_000.0;

/// The keys the Revealed Cards pill is laid out under: the desktop's pill, and the phone's
/// button over the pile, which never change.
const PILL: u64 = 0;
const PILE_BUTTON: u64 = 1;

#[derive(Debug, Default)]
struct RevealedControl {
    shown: Option<u64>,
    pile_slot: Option<kit_base::geometry::Rect>,
}

/// The client's record of the viewer and the Revealed Cards pill.
#[derive(Debug, Default)]
pub(super) struct Viewing {
    /// The pile whose viewer is open, if any.
    pub(super) open: Option<Viewed>,
    /// The faces of the Revealed Cards pile as its viewer was last opened.
    pub(super) revealed: Vec<RevealedFace>,
    /// The boxes the screen shows cards in, kept so that finding them allocates nothing.
    placements: Vec<ShowPlacement>,
    /// The Revealed Cards control, once a script has revealed a card.
    revealed_control: Option<RevealedControl>,
    /// Whether the screen shows the opening hand (`screens::opening`), as it was laid out last
    /// and as it was before that, and the window it was laid out in last.
    pub(super) opening: bool,
    was_opening: bool,
    laid_in: Option<Window>,
    /// Where the opening's coin flip stands.
    coin: Coin,
    /// How many views the table had shown at rest when the screen was last checked for a
    /// script's end (`Show::views_shown`).
    views_shown: usize,
    /// The turn and step the step row was last built for (`Show::turn_at`).
    pub(super) turn: Option<Turn>,
    /// The game screen's step row built ahead for each step of the turn the scripts to come
    /// will show, with that turn (`Client::prepare_steps`), so that a frame builds none.
    steps_ahead: Vec<(Turn, Element)>,
    /// The steps of the turn the scripts to come will show, once each, kept so that listing
    /// them allocates nothing.
    coming: Vec<Turn>,
    /// What else the rows in `steps_ahead` were built from (`RowsFrom`).
    steps_from: Option<RowsFrom>,
}

/// Everything besides the turn that the game screen's step row is built from: the stops, the
/// window, the pile whose viewer is open and whether the opening is shown. A row built ahead
/// is kept until one of them changes.
type RowsFrom = (StopsSetting, Window, Option<Viewed>, bool);

/// The active player and step of `turn`, which are all the step row shows of it.
fn lit(turn: Turn) -> (Side, Step) {
    (turn.active, turn.step)
}

/// Where the opening's coin flip stands: no opening has begun; the coin spins until a time in
/// the clock of `Client::frame`; or the opening has begun and its coin is not spinning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Coin {
    #[default]
    Before,
    Spinning {
        until: u64,
    },
    Landed,
}

impl Viewing {
    /// Notes whether `view`, the latest view whose script has played out if the game screen is
    /// shown, is in its opening. As the opening begins after the opponent won the flip and before anyone
    /// has declared, its coin spins for `FLIP_MS` from `now`, however soon they chose, as the
    /// web's `Opening` spins it from when it first shows.
    pub(super) fn spin(&mut self, view: Option<&PlayerView>, now: u64) {
        let Some(view) = view.filter(|view| game_hud::screens::opening::in_opening(view)) else {
            self.coin = Coin::Before;
            return;
        };
        if self.coin != Coin::Before {
            return;
        }
        let you = &view.players.you;
        let spins = view.turn.flip_winner == Some(Side::Opponent)
            && you.mulligans == 0
            && you.mulligan == Declaration::Undeclared;
        self.coin = if spins {
            Coin::Spinning {
                until: now + ns(opening::FLIP_MS),
            }
        } else {
            Coin::Landed
        };
    }

    /// Whether the opening's coin still spins at `now`.
    pub(super) fn spinning(&self, now: u64) -> bool {
        self.spin_due().is_some_and(|until| now < until)
    }

    /// When the opening's coin stops spinning, while it spins.
    pub(super) fn spin_due(&self) -> Option<u64> {
        match self.coin {
            Coin::Spinning { until } => Some(until),
            Coin::Before | Coin::Landed => None,
        }
    }
}

/// How the opening's cards glide between its hand and its slots: the web interface's
/// `cubic-bezier(0.2, 0.7, 0.3, 1)` (`ui/src/screens/glide.ts`), quick and slowing as it
/// arrives.
const GLIDE: Curve = Curve {
    x1: 0.2,
    y1: 0.7,
    x2: 0.3,
    y2: 1.0,
};

/// A length in milliseconds from `space-constants.json`, in nanoseconds.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a move lasts well under a second"
)]
fn ns(ms: f32) -> u64 {
    (f64::from(ms) * NS_PER_MS) as u64
}

impl Client {
    /// Opens the viewer of `viewed`, or closes the viewer with `None`. A pile with no card opens
    /// no viewer. A viewer that opens starts with an empty search, in the pile's order and
    /// ungrouped, showing no look at a library (`manual::viewers::PileViewer`).
    pub(super) fn view(&mut self, viewed: Option<Viewed>) {
        if viewed == Some(Viewed::Revealed) {
            self.viewing.revealed = self.revealed_faces();
            if self.viewing.revealed.is_empty() {
                return;
            }
        }
        self.manual.viewer = game_manual::manual::viewers::PileViewer::new();
        self.layer.set_field(
            game_manual::manual::viewers::SEARCH,
            kit_ui::widgets::FieldState::default(),
        );
        self.viewing.open = viewed;
        self.show_screen();
    }

    /// Puts the Revealed Cards pile away: each face in it leaves now, and its viewer closes.
    pub(super) fn dismiss_revealed(&mut self) {
        let now = Interval {
            from: self.now,
            to: self.now,
        };
        let lit = self.show.lit_mut();
        for index in 0..lit.open().len() {
            let open = lit.open()[index];
            if matches!(open.span, Span::Revealed { .. }) && open.ends.is_none() {
                lit.end(&open.span, now);
            }
        }
        if self.viewing.open == Some(Viewed::Revealed) {
            self.view(None);
        }
    }

    /// The faces the Revealed Cards pile holds now, the first revealed first.
    fn revealed_faces(&self) -> Vec<RevealedFace> {
        let lit = self.show.lit();
        (lit.open().iter())
            .filter(|open| open.ends.is_none())
            .filter_map(|open| match open.span {
                Span::Revealed { slot, .. } => Some(slot),
                _ => None,
            })
            .filter_map(|slot| {
                let object = self.show.table().revealed(slot)?;
                let name = object.name.clone().unwrap_or_default();
                let face = self.show.faces().list().find(&name);
                Some(RevealedFace { slot, name, face })
            })
            .collect()
    }

    /// Hands the table the boxes the screen now shows cards in, after the screen was laid out
    /// or scrolled. A card moves into its box and back as the tray's cards move into the tray
    /// and back (`OwnStep::TrayLifts` and `OwnStep::TrayLetGo`), the steps a client makes
    /// itself that carry cards off the table to be looked at.
    pub(super) fn place_shown(&mut self) {
        Tree::screen_shows(&self.layer, &mut self.viewing.placements);
        let timing = |step: OwnStep| {
            (self.own_timings.entries.iter())
                .find(|entry| entry.step == step)
                .map_or((0, Curve::EASE_OUT), |entry| {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a step lasts well under a second"
                    )]
                    let length = (entry.timing.duration * NS_PER_MS) as u64;
                    (length, game_play::named_curve(entry.timing.curve))
                })
        };
        // The kept opening hand drops from the opening screen into the hand in about an eighth
        // of a second, slowing as it lands (the web's `KEPT` in `ui/src/stage/arrive.ts`), and
        // while the opening is shown a card clicked into a slot under the hand, or back, glides
        // there as the web's `useGlide` moves it. A new window lays the opening out again, and
        // its cards stand at once in their new places.
        let closed = self.viewing.was_opening && !self.viewing.opening;
        let resized = self.viewing.laid_in.is_some_and(|laid| laid != self.window);
        self.viewing.was_opening = self.viewing.opening;
        self.viewing.laid_in = Some(self.window);
        let back = if closed {
            (ns(opening::KEPT_MS), Curve::EASE_OUT)
        } else {
            timing(OwnStep::TrayLetGo)
        };
        let relaid = (self.viewing.opening && !resized).then_some(Relaid {
            cross: (ns(opening::CROSS_MS), GLIDE),
            shift: (ns(opening::SHIFT_MS), GLIDE),
        });
        // A card the opening shows stands in its box at once, as the web's opening draws its own
        // copy of each card in place rather than moving the table's card there.
        let into = if self.viewing.opening {
            (0, Curve::EASE_OUT)
        } else {
            timing(OwnStep::TrayLifts { sorted: false })
        };
        let moves = Moves { into, back, relaid };
        // A pile's viewer covers the table, and so, on a manual table, do the guided start's
        // opening and the banner of a frozen table, whose cards take no press.
        let covered = (self.viewing.open.is_some()
            && matches!(self.screen, Screen::Game | Screen::Table))
            || self.guided_start()
            || self.table_frozen();
        self.show
            .place_shows(&self.viewing.placements, moves, covered);
    }

    /// Lays the Revealed Cards pill out ahead, so that a frame only shows it, once `view`'s
    /// script holds a card up to be read (`Act::Showcase`), adding its face to the pile. It is
    /// laid out once, for the first script that reveals, and never for a game with none.
    pub(super) fn prepare_pill(&mut self, view: &PlayerView) {
        let reveals = (view.script.iter())
            .flat_map(|script| script.beats.iter().flatten())
            .any(|step| matches!(step.act, Act::Showcase { .. }));
        if reveals {
            self.layer
                .prepare(floats::REVEALED, PILL, revealed::revealed);
            self.viewing
                .revealed_control
                .get_or_insert_with(RevealedControl::default);
            self.prepare_revealed_pile();
        }
    }

    /// Sizes the transparent pile button before playback and after a resize.
    pub(super) fn prepare_revealed_pile(&mut self) {
        let Some(control) = &mut self.viewing.revealed_control else {
            return;
        };
        let slot = self
            .show
            .table()
            .profile()
            .slot("revealed.pile")
            .map(|slot| slot.rect);
        if control.pile_slot.map(|area| (area.width, area.height))
            != slot.map(|area| (area.width, area.height))
        {
            if control.shown == Some(PILE_BUTTON) {
                self.layer.show_prepared(floats::REVEALED, None);
                control.shown = None;
            }
            self.layer
                .forget_prepared(floats::REVEALED, |key| key != PILE_BUTTON);
            if let Some(area) = slot {
                self.layer.prepare(floats::REVEALED, PILE_BUTTON, || {
                    revealed::pile_button(area)
                });
            }
        }
        control.pile_slot = slot;
    }

    /// What each frame does for the screen's own clock: once the opening's coin has stopped
    /// spinning, shows who starts and the decision; as a script ends while the opening is or
    /// will be shown, lays the opening out again for the view it played out; and shows or
    /// hides the Revealed Cards pill (`write_pill`) on the game screen with no sheet open. It
    /// also lights the step the table reaches at `table_now` (`steps_frame`).
    pub(super) fn viewing_frame(&mut self, table_now: u64) {
        let visible = self.screen == Screen::Game && self.features.sheet.is_none();
        self.steps_frame(table_now);
        if (self.viewing.spin_due()).is_some_and(|until| self.now >= until) {
            self.viewing.coin = Coin::Landed;
            self.show_screen();
        }
        let views_shown = self.show.views_shown();
        if views_shown != self.viewing.views_shown {
            self.viewing.views_shown = views_shown;
            let played = self.show.shown().map(|update| &update.view);
            let was_opening = self.viewing.opening || self.show.table().hand_away();
            if was_opening || played.is_some_and(game_hud::screens::opening::in_opening) {
                self.show_screen();
            }
        }
        self.write_pill(visible);
    }

    /// Builds the game screen's step row again, and nothing else, when the turn step the table
    /// shows at `table_now` differs from the one the row shows: a script's `set` step began a
    /// step of the turn. Where the screen shows no step row, the next screen built shows it.
    fn steps_frame(&mut self, table_now: u64) {
        let turn = self.show.turn_at(table_now);
        if turn == self.viewing.turn {
            return;
        }
        // Until the table shows a view, the row shows the latest view's step, so a first turn
        // that only names that step again changes nothing on the screen.
        let latest = (self.show.latest()).map(|update| {
            let turn = &update.view.turn;
            (turn.active, turn.step)
        });
        let shows = turn.map(lit).or(latest);
        let changed = shows != self.viewing.turn.map(lit).or(latest);
        self.viewing.turn = turn;
        if !changed || self.screen != Screen::Game {
            return;
        }
        let ahead = &mut self.viewing.steps_ahead;
        let steps = match ahead.iter().position(|(turn, _)| Some(lit(*turn)) == shows) {
            Some(at) => Some(ahead.swap_remove(at).1),
            // A step no script to come named when the client last prepared.
            None => game_hud::screens::game::game_steps(&self.screen_in().hud()),
        };
        if let Some(steps) = steps {
            self.layer.replace(steps);
        }
    }

    /// Builds the game screen's step row ahead for each step of the turn the scripts to come
    /// will show (`Show::coming_turns`) that has none yet, and drops those built for a step
    /// none of them shows. `prepare` calls it, where allocating is allowed, so that a row the
    /// last frame used is built again before the next frame needs it.
    pub(super) fn prepare_steps(&mut self) {
        if self.screen != Screen::Game {
            self.viewing.steps_ahead.clear();
            return;
        }
        let from = {
            let input = self.screen_in();
            let opening = game_hud::screens::opening::shown(&input.hud());
            (input.stops, input.window, input.viewing, opening)
        };
        if self.viewing.steps_from != Some(from) {
            self.viewing.steps_from = Some(from);
            self.viewing.steps_ahead.clear();
        }
        // The step the row shows now needs no row ahead: a frame changes the row only to show
        // another step, and the next `prepare` builds this one again if a script comes back
        // to it.
        let latest = (self.show.latest()).map(|update| {
            let turn = &update.view.turn;
            (turn.active, turn.step)
        });
        let shown = self.viewing.turn.map(lit).or(latest);
        let mut coming = std::mem::take(&mut self.viewing.coming);
        coming.clear();
        for turn in self.show.coming_turns() {
            if Some(lit(turn)) != shown && !coming.iter().any(|&other| lit(other) == lit(turn)) {
                coming.push(turn);
            }
        }
        (self.viewing.steps_ahead)
            .retain(|&(turn, _)| coming.iter().any(|&shown| lit(shown) == lit(turn)));
        for &turn in &coming {
            let mut built = self.viewing.steps_ahead.iter();
            if built.any(|&(shown, _)| lit(shown) == lit(turn)) {
                continue;
            }
            let input = ScreenIn {
                turn: Some(turn),
                ..self.screen_in()
            };
            if let Some(steps) = game_hud::screens::game::game_steps(&input.hud()) {
                self.viewing.steps_ahead.push((turn, steps));
            }
        }
        self.viewing.coming = coming;
    }

    /// Shows the Revealed Cards pill over the pile while the pile holds a face, on the game
    /// screen with no sheet open, and closes the pile's viewer once its last face has left.
    /// Lays nothing out and allocates nothing.
    fn write_pill(&mut self, visible: bool) {
        let holds = (self.show.lit().open().iter()).any(|open| {
            matches!(open.span, Span::Revealed { .. })
                && open.ends.is_none_or(|ends| ends.to > self.now)
        });
        if !holds && self.viewing.open == Some(Viewed::Revealed) {
            self.view(None);
        }
        let Some(control) = &mut self.viewing.revealed_control else {
            return;
        };
        let pile = control.pile_slot;
        let key = if pile.is_some() { PILE_BUTTON } else { PILL };
        let shown = (visible && holds).then_some(key);
        if shown != control.shown {
            control.shown = shown;
            self.layer.show_prepared(floats::REVEALED, shown);
        }
        if shown.is_some() {
            let point = pile.map_or_else(
                || revealed::pill_corner(self.window.size),
                |area| Vec2::new(area.x, area.y),
            );
            let look = FloatLook {
                point,
                ..FloatLook::default()
            };
            self.layer.set_float_look(floats::REVEALED, look);
        }
    }

    /// Whether a press on a card resting at `place` opens a pile's viewer instead: a
    /// card in a library, graveyard or exile pile on the game screen, or in a graveyard or exile
    /// pile on the manual table (`manual::viewers::opens_on_press`), as the web interface's
    /// button over each pile opens it (`PileButton` in `ui/src/game/Piles.tsx`).
    pub(super) fn pile_at(&self, place: Place) -> Option<Viewed> {
        let Place::Card { zone, side, .. } = place else {
            return None;
        };
        let pile = match self.screen {
            Screen::Game => matches!(
                zone,
                ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile
            ),
            Screen::Table => game_manual::manual::viewers::opens_on_press(zone),
            _ => false,
        };
        (pile && self.features.sheet.is_none()).then_some(Viewed::Pile(side, zone))
    }

    /// Whether the open viewer shows the card in `slot` in one of its boxes.
    pub(super) fn viewer_shows(&self, slot: ObjectId) -> bool {
        (self.viewing.placements.iter()).any(|shown| shown.object.0 == u32::from(slot.0))
    }

    /// Whether the open prompt lists what it asks about over the table on a shade of its own
    /// (`prompts::covers_table`). The table then takes no press, and the piles' and the rows'
    /// counts, which would stand over its list and its pill, are hidden under the shade, as the
    /// web's `.stage-tray` covers them.
    pub(super) fn prompt_covers_table(&self) -> bool {
        self.table_open()
            && (self.prompt.as_ref())
                .is_some_and(|(_, prompt)| game_hud::prompts::covers_table(prompt))
    }

    /// Whether the rows' counts are shown, given whether the hud is `visible` and the table
    /// `covered` by a prompt: not under a prompt's shade (`prompt_covers_table`), nor while a
    /// manual table is frozen, whose banner and Lobby stand over the opponent's rows, which
    /// take no scroll until it thaws.
    pub(super) fn rows_counted(&self, visible: bool, covered: bool) -> bool {
        visible && !covered && !self.table_frozen()
    }
}
