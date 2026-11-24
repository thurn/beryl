//! The pile viewer and the prepared Revealed Cards controls.

use kit_base::data::Card;
use std::convert::Infallible;

use beryl_protocol::PlayerView;
use beryl_protocol::script::Act;
use game_table::profiles::Profile;
use glam::Vec2;
use kit_base::data::presentation::Revealed;
use kit_scene::{Context, Feature, StateUpdate};
use kit_ui::element::{Element, floats, row};
use kit_ui::input::{Rect, Size};
use kit_ui::layout::{FloatLook, Layer};

use crate::hud::HudIn;
use crate::inspect::Viewed;

pub mod revealed;
pub use revealed::RevealedFace;
mod view;
pub use view::{cards, count_words, grid, object_box, pile_title, show_grid, viewer_dialog};

/// Requests from the viewer and the Revealed Cards pill.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Show(Option<Viewed>, Vec<RevealedFace>),
    Open(Viewed),
    Close,
    DismissRevealed,
}

pub enum Request {
    Open(Viewed),
    Close,
    DismissRevealed,
}

/// The selected pile, the revealed faces captured when its viewer opened, and
/// the prepared Revealed Cards control showing over the table.
#[derive(Debug, Default)]
pub struct State {
    open: Option<Viewed>,
    revealed: Vec<RevealedFace>,
    control: Option<RevealedControl>,
}

/// Inputs to the dialog. A new view may change which cards its slots name.
pub struct Inputs<'a> {
    pub window: Size,
    pub centre: Vec2,
    /// The card whose shape the grid's boxes take.
    pub card: Card,
    pub view: Option<&'a PlayerView>,
    pub revision: usize,
}

/// The pile viewer.
pub struct Viewer;

impl Feature for Viewer {
    const SOURCE: &'static str = file!();

    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = (Size, Vec2, Card, Option<Viewed>, Option<usize>);
    type Command = Infallible;
    type Request = Request;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Infallible, Request>,
    ) -> bool {
        let request = match message {
            Message::Show(open, revealed) => {
                if state.open == *open && state.revealed == *revealed {
                    return false;
                }
                state.open = *open;
                state.revealed.clone_from(revealed);
                return true;
            }
            Message::Open(pile) => Request::Open(*pile),
            Message::Close => Request::Close,
            Message::DismissRevealed => Request::DismissRevealed,
        };
        context.request(request);
        false
    }

    fn view_key(state: &State, input: &Inputs<'_>) -> Self::ViewKey {
        let revision = matches!(state.open, Some(Viewed::Pile(..))).then_some(input.revision);
        (input.window, input.centre, input.card, state.open, revision)
    }

    fn view(state: &State, input: Inputs<'_>) -> Element {
        view::dialog(
            (input.window, input.centre, input.card),
            input.view,
            state.open,
            &state.revealed,
        )
        .unwrap_or_else(row)
    }
}

impl<'a> Inputs<'a> {
    #[must_use]
    pub fn new(
        profile: Profile,
        window: Size,
        view: Option<&'a PlayerView>,
        revision: usize,
    ) -> Option<Self> {
        let area = profile.slot("viewer")?.rect;
        Some(Self {
            window,
            centre: Vec2::new(area.x + area.width / 2.0, area.y + area.height / 2.0),
            card: profile.card(),
            view,
            revision,
        })
    }
}

#[derive(Debug, Default)]
struct RevealedControl {
    pile_slot: Option<Rect>,
    shown: Option<u64>,
}

/// The float's prepared keys for the desktop pill and phone pile button.
const PILL: u64 = 0;
const PILE_BUTTON: u64 = 1;

/// Updates the revealed-cards control as views arrive, the window resizes and playback advances.
pub enum Playback<'a> {
    Prepare {
        layer: &'a mut Layer,
        view: &'a PlayerView,
        profile: Profile<'a>,
    },
    Resize {
        layer: &'a mut Layer,
        profile: Profile<'a>,
    },
    Draw {
        layer: &'a mut Layer,
        window: Size,
        tuning: Revealed,
        card: Card,
        visible: bool,
    },
}

impl StateUpdate<Viewer> for Playback<'_> {
    type Output = ();

    fn apply(self, state: &mut State) -> (bool, ()) {
        match self {
            Self::Prepare {
                layer,
                view,
                profile,
            } => state.prepare_pill(layer, view, profile),
            Self::Resize { layer, profile } => state.prepare_pile(layer, profile),
            Self::Draw {
                layer,
                window,
                tuning,
                card,
                visible,
            } => {
                state.write_pill(layer, window, (tuning, card), visible);
            }
        }
        (false, ())
    }
}

impl State {
    #[must_use]
    pub fn open(&self) -> Option<Viewed> {
        self.open
    }

    #[must_use]
    pub fn revealed(&self) -> &[RevealedFace] {
        &self.revealed
    }

    /// Lays the Revealed Cards pill out ahead, so that a frame only shows it, once `view`'s
    /// script holds a card up to be read (`Act::Showcase`), adding its face to the pile. It is
    /// laid out once, for the first script that reveals, and never for a game with none.
    fn prepare_pill(&mut self, layer: &mut Layer, view: &PlayerView, profile: Profile) {
        let reveals = (view.script.iter())
            .flat_map(|script| script.beats.iter().flatten())
            .any(|step| matches!(step.act, Act::Showcase { .. }));
        if reveals {
            layer.prepare(floats::REVEALED, PILL, revealed::revealed);
            self.control.get_or_insert_with(RevealedControl::default);
            self.prepare_pile(layer, profile);
        }
    }

    /// Sizes the transparent pile button before playback and after a resize.
    fn prepare_pile(&mut self, layer: &mut Layer, profile: Profile) {
        let Some(control) = &mut self.control else {
            return;
        };
        let slot = profile.slot("revealed.pile").map(|slot| slot.rect);
        if control.pile_slot.map(|area| (area.width, area.height))
            != slot.map(|area| (area.width, area.height))
        {
            if control.shown == Some(PILE_BUTTON) {
                layer.show_prepared(floats::REVEALED, None);
                control.shown = None;
            }
            layer.forget_prepared(floats::REVEALED, |key| key != PILE_BUTTON);
            if let Some(area) = slot {
                layer.prepare(floats::REVEALED, PILE_BUTTON, || {
                    revealed::pile_button(area)
                });
            }
        }
        control.pile_slot = slot;
    }

    /// Shows the prepared control while a revealed face is visible.
    fn write_pill(
        &mut self,
        layer: &mut Layer,
        window: Size,
        (tuning, card): (Revealed, Card),
        visible: bool,
    ) {
        let Some(control) = &mut self.control else {
            return;
        };
        let pile = control.pile_slot;
        let key = if pile.is_some() { PILE_BUTTON } else { PILL };
        let shown = visible.then_some(key);
        if shown != control.shown {
            control.shown = shown;
            layer.show_prepared(floats::REVEALED, shown);
        }
        if shown.is_some() {
            let point = pile.map_or_else(
                || revealed::pill_corner(window, tuning, card),
                |area| Vec2::new(area.x, area.y),
            );
            let look = FloatLook {
                point,
                ..FloatLook::default()
            };
            layer.set_float_look(floats::REVEALED, look);
        }
    }
}

/// Builds a viewer directly from supplied screen inputs.
#[must_use]
pub fn viewer(input: &HudIn<'_>) -> Option<Element> {
    let layout = Inputs::new(input.profile, input.window.size, input.view, 0)?;
    view::dialog(
        (layout.window, layout.centre, layout.card),
        layout.view,
        input.viewing,
        input.revealed,
    )
}
