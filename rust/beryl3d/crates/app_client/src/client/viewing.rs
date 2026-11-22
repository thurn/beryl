//! Routes the pile viewer and the opening's playback to the session and the table.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{Declaration, ObjectId, Side, ZoneName};
use beryl_protocol::script::OwnStep;
use game_hud::inspect::Viewed;

use super::Client;
use app_screens::screens::Screen;
use game_hud::hud::viewer::RevealedFace;
use game_play::light::Span;
use game_play::verbs::Interval;
use game_table::table::{Moves, Relaid, Window};
use kit_base::pose::Curve;
use kit_frame::frame::{Place, ShowPlacement};

/// Nanoseconds in a millisecond, as the session's step timings are written in milliseconds.
const NS_PER_MS: f64 = 1_000_000.0;

/// Playback of the opening screen and its coin flip.
#[derive(Debug, Default)]
pub(super) struct Opening {
    pub shown: bool,
    coin: Coin,
    views_shown: usize,
}

/// Card boxes from the entire screen, including the opening, prompts and pile viewers.
#[derive(Debug, Default)]
pub(super) struct ScreenCards {
    placements: Vec<ShowPlacement>,
    was_opening: bool,
    laid_in: Option<Window>,
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

impl Opening {
    /// Notes whether `view`, the latest view whose script has played out if the game screen is
    /// shown, is in its opening. If the opponent won the flip and nobody has declared, the coin
    /// spins for `flip_ms` from `now`, as it does when the web's opening first appears.
    pub(super) fn spin(&mut self, view: Option<&PlayerView>, now: u64, flip_ms: f32) {
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
                until: now + ns(flip_ms),
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
    fn land(&mut self, now: u64) -> bool {
        if self.spin_due().is_some_and(|until| now >= until) {
            self.coin = Coin::Landed;
            return true;
        }
        false
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

/// Converts milliseconds to nanoseconds.
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
        let mut revealed = Vec::new();
        if viewed == Some(Viewed::Revealed) {
            revealed = self.revealed_faces();
            if revealed.is_empty() {
                return;
            }
        }
        self.features.table_mut().viewer = game_manual::manual::viewers::PileViewer::new();
        self.layer.set_field(
            game_manual::manual::viewers::SEARCH,
            kit_ui::widgets::FieldState::default(),
        );
        self.features.show_viewer(viewed, revealed);
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
        if self.features.viewer().open() == Some(Viewed::Revealed) {
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
        let placements = &mut self.features.screen_cards.placements;
        placements.clear();
        self.layer
            .visit_shows(|placement| placements.push(placement));
        let timing = |step: OwnStep| {
            (self.step_timings.entries.iter())
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
        let closed = self.features.screen_cards.was_opening && !self.features.opening.shown;
        let resized = self
            .features
            .screen_cards
            .laid_in
            .is_some_and(|laid| laid != self.window);
        self.features.screen_cards.was_opening = self.features.opening.shown;
        self.features.screen_cards.laid_in = Some(self.window);
        let opening = self.data().shown.opening;
        let back = if closed {
            (ns(opening.kept_ms), Curve::EASE_OUT)
        } else {
            timing(OwnStep::TrayLetGo)
        };
        let relaid = (self.features.opening.shown && !resized).then_some(Relaid {
            cross: (ns(opening.cross_ms), GLIDE),
            shift: (ns(opening.shift_ms), GLIDE),
        });
        // A card the opening shows stands in its box at once, as the web's opening draws its own
        // copy of each card in place rather than moving the table's card there.
        let into = if self.features.opening.shown {
            (0, Curve::EASE_OUT)
        } else {
            timing(OwnStep::TrayLifts { sorted: false })
        };
        let moves = Moves { into, back, relaid };
        // A pile's viewer covers the table, and so, on a manual table, do the guided start's
        // opening and the banner of a frozen table, whose cards take no press.
        let covered = (self.features.viewer().open().is_some()
            && matches!(self.features.screen, Screen::Game | Screen::Table))
            || self.guided_start()
            || self.table_frozen();
        self.show
            .place_shows(&self.features.screen_cards.placements, moves, covered);
    }

    pub(super) fn prepare_pill(&mut self, view: &PlayerView) {
        self.features
            .viewer_mut()
            .prepare_pill(&mut self.layer, view, self.show.table().profile());
    }

    pub(super) fn prepare_revealed_pile(&mut self) {
        self.features
            .viewer_mut()
            .prepare_pile(&mut self.layer, self.show.table().profile());
    }

    /// Advances the opening and viewer after the table has reached this frame.
    pub(super) fn viewing_frame(&mut self, table_now: u64) {
        let visible = self.features.screen == Screen::Game && self.features.sheet.is_none();
        let input = self.step_inputs();
        let turn = self.show.turn_at(table_now);
        let in_game = self.features.screen == Screen::Game;
        self.features
            .steps_mut()
            .write(&mut self.layer, turn, &input, in_game);
        if self.features.opening.land(self.now) {
            self.show_screen();
        }
        let views_shown = self.show.views_shown();
        if views_shown != self.features.opening.views_shown {
            self.features.opening.views_shown = views_shown;
            let played = self.show.shown().map(|update| &update.view);
            let was_opening = self.features.opening.shown || self.show.table().hand_away();
            if was_opening || played.is_some_and(game_hud::screens::opening::in_opening) {
                self.show_screen();
            }
        }
        let holds = self.show.lit().open().iter().any(|open| {
            matches!(open.span, Span::Revealed { .. })
                && open.ends.is_none_or(|ends| ends.to > self.now)
        });
        if !holds && self.features.viewer().open() == Some(Viewed::Revealed) {
            self.view(None);
        }
        self.features.viewer_mut().write_pill(
            &mut self.layer,
            self.window.size,
            (
                self.show.table().data().revealed,
                self.show.table().data().card,
            ),
            visible && holds,
        );
    }

    pub(super) fn prepare_steps(&mut self) {
        if self.features.screen == Screen::Game {
            let input = self.step_inputs();
            self.features
                .steps_mut()
                .prepare(&input, self.show.coming_turns());
        } else {
            self.features.steps_mut().clear_prepared();
        }
    }

    pub(super) fn step_inputs(&self) -> game_hud::hud::steps::Inputs {
        game_hud::hud::steps::Inputs::new(&self.screen_in().hud())
    }

    /// Whether the open viewer shows the card in `slot` in one of its boxes.
    pub(super) fn viewer_shows(&self, slot: ObjectId) -> bool {
        self.features
            .screen_cards
            .placements
            .iter()
            .any(|shown| shown.object.0 == u32::from(slot.0))
    }

    /// Whether the open prompt lists what it asks about over the table on a shade of its own
    /// (`prompts::covers_table`). The table then takes no press, and the piles' and the rows'
    /// counts, which would stand over its list and its pill, are hidden under the shade, as the
    /// web's `.stage-tray` covers them.
    pub(super) fn prompt_covers_table(&self) -> bool {
        self.table_open()
            && (self.features.prompts.current()).is_some_and(game_hud::prompts::covers_table)
    }

    /// Whether the rows' counts are shown, given whether the hud is `visible` and the table
    /// `covered` by a prompt: not under a prompt's shade (`prompt_covers_table`), nor while a
    /// manual table is frozen, whose banner and Lobby stand over the opponent's rows, which
    /// take no scroll until it thaws.
    pub(super) fn rows_counted(&self, visible: bool, covered: bool) -> bool {
        visible && !covered && !self.table_frozen()
    }
}

/// Maps a card or count to the pile whose action owns it.
pub(super) fn pointer_pile(
    place: Place,
    screen: Screen,
    no_sheet: bool,
    manual_library: bool,
) -> Option<Viewed> {
    let Place::Card { zone, side, .. } = place else {
        return None;
    };
    let opens = match screen {
        Screen::Game => matches!(
            zone,
            ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile
        ),
        Screen::Table => {
            game_manual::manual::viewers::opens_on_press(zone)
                || (manual_library && zone == ZoneName::Library)
        }
        _ => false,
    };
    (opens && no_sheet).then_some(Viewed::Pile(side, zone))
}
