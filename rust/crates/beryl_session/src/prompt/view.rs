//! The wire types: a prompt as its seat is sent it, and the click command.

use beryl_engine::card::{Place, Shape};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::protocol::DecisionId;
use crate::script::{Arrow, Mark, Subject};

/// The open prompt as its seat sees it: everything the screen shows for it. The session makes
/// one when a decision opens a prompt and a new one after each click; the UI draws the latest
/// and keeps nothing between them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PromptView {
    /// Counts this prompt's states from 0. A click names the state it was made on.
    pub revision: u32,
    /// What the prompt is for: a [`Purpose`](beryl_engine::card::Purpose)'s name. The UI
    /// reads no meaning from it; it keys sound and motion.
    pub purpose: String,
    /// What the prompt asks, in words, from the wording table.
    pub title: String,
    /// The mark on each card, ability and player that has one, at most one each, in no
    /// order. What has none is not part of the prompt as it stands.
    pub marks: Vec<Marked>,
    /// The buttons, in the order they are laid out.
    pub buttons: Vec<PromptButton>,
    /// How the prompt is answered, with what the player has put together so far.
    pub shape: ShapeView,
}

/// A mark on one thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Marked {
    pub on: Subject,
    pub mark: Mark,
}

/// A button of a prompt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PromptButton {
    pub id: ButtonId,
    pub label: String,
    /// Whether a click does anything now.
    pub enabled: bool,
    /// The one the prompt puts forward: at most one button of a prompt.
    pub primary: bool,
}

/// Which button of a prompt: what a click on it names, and where the stage puts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ButtonId {
    /// Answers with what the player has put together.
    Confirm,
    /// Answers with nothing: pass priority, find nothing, no attackers.
    Decline,
    /// Answers with what the marks suggest, as Auto Pay does.
    Auto,
    /// Passes priority on to the viewer's next stop in a later step.
    PassOn,
    /// One of the answers a mode, a yes or no or a number offers as buttons: a candidate of
    /// the open decision, by index.
    Option { index: u16 },
}

/// A prompt's shape ([`Shape`]) and that shape's own state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "shape", rename_all = "camelCase")]
pub enum ShapeView {
    /// Objects picked one click at a time.
    Pick {
        /// The cards laid out to pick from when they are not on the table as it lies: the
        /// top of a library, a hand, a graveyard. Empty when they are picked where they lie.
        tray: Vec<u16>,
        /// How few and how many may be picked.
        least: u8,
        most: u8,
        /// Those picked, in the order they were.
        picked: Vec<Subject>,
    },
    /// Things in an order the player changes, first to last.
    Order { items: Vec<Subject> },
    /// Cards dealt out between piles.
    Split { piles: Vec<Pile> },
    /// A question with two answers, each a button.
    YesNo {
        /// The cards the question is about.
        about: Vec<u16>,
    },
    /// A number among `values`, ascending.
    Number {
        values: Vec<u8>,
        value: u8,
        /// What the value means, where it means more than itself: the total cost an X makes.
        detail: Option<String>,
    },
    /// One of a few answers, each a button.
    Mode {
        /// The cards the choice is about.
        about: Vec<u16>,
    },
    /// One entry of a list.
    Catalog {
        entries: Vec<String>,
        /// The entry chosen, by index.
        chosen: Option<u16>,
    },
    /// An amount divided among recipients.
    Divide {
        total: u8,
        /// How much of it is not yet given out.
        left: u8,
        shares: Vec<Share>,
    },
    /// A cost paid with permanents: the marks say which pay and which the default would use.
    Pay {
        /// The whole cost, and what of it is still to pay, each as the screen writes it: a
        /// mana cost as Oracle text does, a crew cost as its power.
        cost: String,
        left: String,
    },
    /// Objects each aimed at something.
    Declare {
        /// The line drawn from each object to what it is aimed at.
        arrow: Arrow,
        pairs: Vec<Pair>,
        /// The objects picked whose aim the next click gives.
        aiming: Vec<u16>,
    },
}

impl ShapeView {
    /// The shape this is the state of.
    pub fn shape(&self) -> Shape {
        match self {
            ShapeView::Pick { .. } => Shape::Pick,
            ShapeView::Order { .. } => Shape::Order,
            ShapeView::Split { .. } => Shape::Split,
            ShapeView::YesNo { .. } => Shape::YesNo,
            ShapeView::Number { .. } => Shape::Number,
            ShapeView::Mode { .. } => Shape::Mode,
            ShapeView::Catalog { .. } => Shape::Catalog,
            ShapeView::Divide { .. } => Shape::Divide,
            ShapeView::Pay { .. } => Shape::Pay,
            ShapeView::Declare { .. } => Shape::Declare,
        }
    }
}

/// One pile of a split.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Pile {
    pub place: PlaceName,
    pub label: String,
    /// The cards in it: where the place keeps an order, as they will lie, the top first.
    pub cards: Vec<u16>,
    /// How many it must hold for the prompt to confirm, if a number.
    pub exactly: Option<u8>,
    /// It is settled: no card can be dropped into it or taken out.
    pub closed: bool,
}

/// One recipient's share of a division.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    pub on: Subject,
    pub amount: u8,
}

/// An object and what it is aimed at: an attacker and the player or planeswalker it attacks,
/// a blocker and the attacker it blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Pair {
    pub from: u16,
    pub to: Subject,
}

/// Where a pile's cards go ([`Place`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PlaceName {
    None,
    Top,
    OnTop,
    Bottom,
    Hand,
    Graveyard,
    Exile,
    Battlefield,
}

impl From<Place> for PlaceName {
    fn from(place: Place) -> Self {
        match place {
            Place::None => PlaceName::None,
            Place::Top => PlaceName::Top,
            Place::OnTop => PlaceName::OnTop,
            Place::Bottom => PlaceName::Bottom,
            Place::Hand => PlaceName::Hand,
            Place::Graveyard => PlaceName::Graveyard,
            Place::Exile => PlaceName::Exile,
            Place::Battlefield => PlaceName::Battlefield,
        }
    }
}

/// What a click on an open prompt names. The UI sends what was clicked and decides nothing:
/// whether the click picks, unpicks, aims or does nothing is the session's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Click {
    /// A card, an ability on the stack or a player.
    On { on: Subject },
    /// One of the prompt's buttons.
    Button { button: ButtonId },
    /// A card dropped into pile `pile` of a split, or an item of an order moved (`pile` 0):
    /// to before what is now at `at`, counting from 0, or to the end when `at` is the length.
    Drop { on: Subject, pile: u8, at: u16 },
    /// A number: the value of a number prompt, or with `on` that recipient's share of a
    /// division.
    Number { n: u8, on: Option<Subject> },
    /// An entry of a catalog, by index.
    Entry { index: u16 },
}

/// The click command's request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ClickRequest {
    /// The open decision the UI received.
    pub decision: DecisionId,
    /// The state of its prompt the click was made on ([`PromptView::revision`]): a click on
    /// an older state is refused, as an answer to an older decision is.
    pub revision: u32,
    pub click: Click,
}

/// The click command's reply. A click changes no game state until it answers the prompt, so
/// the new state is the reply, not a view: nothing else on the screen has changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ClickReply {
    /// The prompt goes on, in this state. A click that does nothing returns the state it was
    /// made on.
    Open { prompt: PromptView },
    /// The click answered the prompt: the session has sent the engine its picks, and the
    /// game's next views arrive as updates, as after any answer.
    Answered,
}
