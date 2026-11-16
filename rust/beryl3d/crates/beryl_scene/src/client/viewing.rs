//! The viewer of a pile's cards (`inspect::viewer`) and the Revealed Cards pill
//! (`inspect::revealed`): which pile's viewer is open, the cards shown in its boxes, which the
//! table moves there and back (`Table::show_in`), and the pill, shown whenever the Revealed Cards
//! pile holds a face.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{ObjectId, ZoneName};
use beryl_protocol::script::{Act, OwnStep};

use super::Client;
use crate::element::{Viewed, floats};
use crate::frame::Place;
use crate::input::Rect;
use crate::inspect::revealed::{self, RevealedFace};
use crate::layout::FloatLook;
use crate::light::Span;
use crate::pose::Curve;
use crate::screens::Screen;
use crate::table::Moves;
use crate::verbs::Interval;

/// Nanoseconds in a millisecond, as the session's step timings are written in milliseconds.
const NS_PER_MS: f64 = 1_000_000.0;

/// The key the Revealed Cards pill is laid out under: it never changes.
const PILL: u64 = 0;

/// The client's record of the viewer and the Revealed Cards pill.
#[derive(Debug, Default)]
pub(super) struct Viewing {
    /// The pile whose viewer is open, if any.
    pub(super) open: Option<Viewed>,
    /// The faces of the Revealed Cards pile as its viewer was last opened.
    pub(super) revealed: Vec<RevealedFace>,
    /// The boxes the screen shows cards in, kept so that finding them allocates nothing.
    boxes: Vec<(ObjectId, Rect, bool)>,
    /// Whether the pill is shown.
    pill: bool,
    /// The pile the primary pointer went down on, which a release on it opens.
    pub(super) down: Option<Viewed>,
}

impl Client {
    /// Opens the viewer of `viewed`, or closes the viewer with `None`. A pile with no card opens
    /// no viewer.
    pub(super) fn view(&mut self, viewed: Option<Viewed>) {
        if viewed == Some(Viewed::Revealed) {
            self.viewing.revealed = self.revealed_faces();
            if self.viewing.revealed.is_empty() {
                return;
            }
        }
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
        self.layer.shown_objects(&mut self.viewing.boxes);
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
                    (length, Curve::named(entry.timing.curve))
                })
        };
        let moves = Moves {
            into: timing(OwnStep::TrayLifts { sorted: false }),
            back: timing(OwnStep::TrayLetGo),
        };
        let covered =
            self.viewing.open.is_some() && matches!(self.screen, Screen::Game | Screen::Table);
        self.show.show_in(&self.viewing.boxes, moves, covered);
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
        }
    }

    /// Shows the Revealed Cards pill over the pile while the pile holds a face, on the game
    /// screen with no sheet open, and closes the pile's viewer once its last face has left.
    /// Lays nothing out and allocates nothing.
    pub(super) fn write_pill(&mut self, visible: bool) {
        let holds = (self.show.lit().open().iter()).any(|open| {
            matches!(open.span, Span::Revealed { .. })
                && open.ends.is_none_or(|ends| ends.to > self.now)
        });
        if !holds && self.viewing.open == Some(Viewed::Revealed) {
            self.view(None);
        }
        let shown = visible && holds;
        if shown != self.viewing.pill {
            self.viewing.pill = shown;
            self.layer
                .show_prepared(floats::REVEALED, shown.then_some(PILL));
        }
        if shown {
            let point = revealed::pill_corner(self.window.size);
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
            Screen::Table => crate::manual::viewers::opens_on_press(zone),
            _ => false,
        };
        (pile && self.sheet.is_none()).then_some(Viewed::Pile(side, zone))
    }

    /// Whether the open viewer shows the card in `slot` in one of its boxes.
    pub(super) fn viewer_shows(&self, slot: ObjectId) -> bool {
        (self.viewing.boxes.iter()).any(|(shown, ..)| *shown == slot)
    }
}
