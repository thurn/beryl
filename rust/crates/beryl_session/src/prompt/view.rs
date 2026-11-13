//! The wire types: a prompt as its seat is sent it, and the click command.

use beryl_engine::card::{Part, Place, Purpose, Shape};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::protocol::{DecisionId, ObjectId, Side, ZoneName};
use crate::script::{Arrow, Mark, Subject};

/// The open prompt as its seat sees it: what the decision's pick belongs to, and everything
/// the screen shows for it. The session makes one when a decision opens a prompt and a new one
/// after each click.
///
/// It is the decision's [`Prompt`](beryl_engine::card::Prompt) by public slot and relative to
/// the viewer, in words, with the prompt's state where the session holds it. No field is for
/// one card's question: a card that asks a known purpose changes nothing here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PromptView {
    /// Counts this prompt's states from 0. A click names the state it was made on.
    pub revision: u32,
    /// What the prompt is for. Its [`Shape`] is the purpose's ([`Purpose::shape`]). The wire
    /// writes it as its name, and TypeScript has the closed list of names.
    #[serde(with = "purpose_name")]
    #[ts(type = "Purpose")]
    pub purpose: Purpose,
    /// Which part of the shape the open decision's pick answers: a prompt is a run of picks.
    pub part: PartName,
    /// What the prompt asks, in words, from the wording table.
    pub title: String,
    /// The words of the buttons a UI that holds the prompt's state itself puts on it.
    pub words: Words,
    /// How much the prompt is about as a whole: the cards looked at on top of a library, a
    /// crew cost's power, the life to pay. None where its purpose has no such number.
    pub count: Option<u8>,
    /// The objects the question is about, in candidate order: what a yes then picks among,
    /// the creatures being aimed, the cards a yes or no or a mode asks about.
    pub about: Vec<ObjectId>,
    /// Whose cards the prompt is among and the zone they lie in: those of the first of
    /// `about`, or else of the first object the decision offers. None where it has neither.
    pub from: Option<Lies>,
    /// The mark on each card, ability and player that has one, at most one each, in no
    /// order.
    pub marks: Vec<Marked>,
    /// The buttons, in the order they are laid out.
    pub buttons: Vec<PromptButton>,
    /// What the player has put together so far, by the prompt's shape; or that the session
    /// holds no state for the prompt, which the UI then answers by candidate.
    pub body: ShapeBody,
}

impl PromptView {
    /// Whether the prompt is for `purpose`.
    pub fn is(&self, purpose: Purpose) -> bool {
        self.purpose == purpose
    }

    /// How the prompt is answered.
    pub fn shape(&self) -> Shape {
        self.purpose.shape()
    }
}

/// The name of `shape`, as the wire tags a [`ShapeBody`] of it.
pub fn shape_name(shape: Shape) -> &'static str {
    match shape {
        Shape::Pick => "pick",
        Shape::Order => "order",
        Shape::Split => "split",
        Shape::YesNo => "yesNo",
        Shape::Number => "number",
        Shape::Mode => "mode",
        Shape::Catalog => "catalog",
        Shape::Divide => "divide",
        Shape::Pay => "pay",
        Shape::Declare => "declare",
    }
}

/// A [`Purpose`] on the wire: its name.
mod purpose_name {
    use beryl_engine::card::Purpose;
    use serde::de::Error as _;
    use serde::{Deserialize as _, Deserializer, Serializer};

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's `with` passes a reference"
    )]
    pub fn serialize<S: Serializer>(purpose: &Purpose, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(purpose.name())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Purpose, D::Error> {
        let name = String::deserialize(d)?;
        (Purpose::ALL.iter().copied())
            .find(|purpose| purpose.name() == name)
            .ok_or_else(|| D::Error::custom(format!("no purpose is named {name:?}")))
    }
}

/// The TypeScript type of a [`Purpose`]: the union of every purpose's name.
pub(crate) fn purpose_decl() -> String {
    let names: Vec<String> = (Purpose::ALL.iter())
        .map(|purpose| format!("\"{}\"", purpose.name()))
        .collect();
    format!("type Purpose = {};", names.join(" | "))
}

/// Which part of a prompt a pick answers ([`Part`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PartName {
    Pick,
    Whether,
    Instead,
    Count,
    First,
    Second,
    Aim,
    Plan,
}

impl From<Part> for PartName {
    fn from(part: Part) -> Self {
        match part {
            Part::Pick => PartName::Pick,
            Part::Whether => PartName::Whether,
            Part::Instead => PartName::Instead,
            Part::Count => PartName::Count,
            Part::First => PartName::First,
            Part::Second => PartName::Second,
            Part::Aim => PartName::Aim,
            Part::Plan => PartName::Plan,
        }
    }
}

/// Whose cards and where they lie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Lies {
    pub player: Side,
    pub zone: ZoneName,
}

/// The words of a prompt's buttons, from the wording table, for a UI that holds the prompt's
/// state itself: the session's own [`PromptButton`]s say theirs. A button the purpose does not
/// have is empty. `{picked}` stands for how many are picked, `{of}` for how many the prompt
/// takes and `{s}` for the plural's letter unless one is picked, which only the holder of the
/// state knows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Words {
    /// The button that answers with what is picked.
    pub confirm: String,
    /// That button where the prompt takes several and counts them.
    pub counted: String,
    /// The button that answers with nothing.
    pub decline: String,
    /// The button that answers yes, and the one that answers no.
    pub yes: String,
    pub no: String,
    /// The button that answers as the session suggests: its payment, every attacker.
    pub auto: String,
    /// The button that takes the prompt back, where it can be.
    pub cancel: String,
    /// What each of `places` is called over its pile; or of an order, what its two ends are
    /// called, the last and then the first.
    pub piles: [String; 2],
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
    /// What it says under its label, where it says more: where a pass of priority leads.
    /// Empty otherwise.
    pub caption: String,
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
    /// Answers with nothing: pass priority, find nothing, no attackers. Where the prompt asks
    /// about a pick before it answers ([`ShapeBody::Pick`]'s `warned`), it takes the pick back;
    /// on the choice among a card's ways, open over a priority, it goes back to the priority;
    /// with attackers or blockers declared, it takes the declaration back.
    Decline,
    /// Takes the session's default: Auto Pay answers with the payment the marks suggest, and
    /// All Attack declares every creature that can attack, still to be confirmed.
    Auto,
    /// Passes priority on to the viewer's next stop in a later step.
    PassOn,
    /// One of the answers a mode, a yes or no or a number offers as buttons: a candidate of
    /// the open decision, by index.
    Option { index: u16 },
    /// On the choice among a card's ways, an activated ability of the card that the priority
    /// does not offer now, by how many activated abilities its face lists before it
    /// ([`CandidateView::nth`](crate::protocol::CandidateView::nth)): shown in the card's
    /// order, never enabled.
    Ability { nth: u8 },
}

/// What a prompt's player has put together so far, tagged by its [`Shape`]; or that the
/// session holds no state for the prompt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "shape", rename_all = "camelCase")]
pub enum ShapeBody {
    /// Objects picked one click at a time.
    Pick {
        /// The cards laid out to pick from when they are not on the table as it lies: the
        /// top of a library, a hand, a graveyard. Empty when they are picked where they lie.
        tray: Vec<ObjectId>,
        /// How few and how many may be picked.
        least: u8,
        most: u8,
        /// Those picked, in the order they were.
        picked: Vec<Subject>,
        /// Those the prompt asks about before it answers with them: the viewer's own
        /// permanents a removal would target (UI Conventions, exception 7). A click on one
        /// picks it and puts the question; the confirm button then answers.
        warned: Vec<Subject>,
    },
    /// Things in an order the player changes, first to last: waiting triggered abilities as
    /// they will resolve. A waiting trigger is not on the stack yet, so each is named as an
    /// ability whose number is the index of its candidate in the open decision, which says
    /// its source and which of the source's abilities it is.
    Order { items: Vec<Subject> },
    /// Cards dealt out between two piles, the first destination's and then the second's. A
    /// click on a card sends it to the other pile, at its end.
    Split { piles: Vec<Pile> },
    /// A question with two answers, each a button.
    YesNo {
        /// The cards the question is about.
        about: Vec<ObjectId>,
        /// Where the card the question is about goes on a yes, and on a no; none where the
        /// question sends no card anywhere.
        places: Option<[PlaceName; 2]>,
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
        about: Vec<ObjectId>,
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
    /// A cost paid with permanents: the marks say which are picked to pay (`paying`), which
    /// the default would tap for the rest (`suggested`) and which else a click can add
    /// (`eligible`). A click on a permanent picks it or takes it back.
    Pay {
        /// The whole cost, and what of it is still to pay, each as the screen writes it: a
        /// mana cost as Oracle text does, a crew cost as its power.
        cost: String,
        left: String,
    },
    /// Objects each aimed at something: the marks say which can be declared (`eligible`) and
    /// which are (`selected`), and, while some await their aim, what they can be aimed at
    /// (`eligible`). A click on a creature declares it or takes it back; one on an aim aims
    /// those awaiting it.
    Declare {
        /// The line drawn from each object to what it is aimed at.
        arrow: Arrow,
        pairs: Vec<Pair>,
        /// Whether those lines are drawn: a block's are, and an attack's only where more
        /// than one thing can be attacked, as the script draws them once it is declared.
        lined: bool,
        /// The objects picked whose aim the next click gives.
        aiming: Vec<ObjectId>,
    },
    /// The session holds no state for this prompt (a payment whose plans tap the same
    /// permanents, a pick no shape here takes): the UI answers it by the decision's candidates,
    /// with buttons it words from the prompt's [`Words`].
    Candidates,
}

impl ShapeBody {
    /// The shape this is the state of; none where the session holds no state.
    pub fn shape(&self) -> Option<Shape> {
        Some(match self {
            ShapeBody::Pick { .. } => Shape::Pick,
            ShapeBody::Order { .. } => Shape::Order,
            ShapeBody::Split { .. } => Shape::Split,
            ShapeBody::YesNo { .. } => Shape::YesNo,
            ShapeBody::Number { .. } => Shape::Number,
            ShapeBody::Mode { .. } => Shape::Mode,
            ShapeBody::Catalog { .. } => Shape::Catalog,
            ShapeBody::Divide { .. } => Shape::Divide,
            ShapeBody::Pay { .. } => Shape::Pay,
            ShapeBody::Declare { .. } => Shape::Declare,
            ShapeBody::Candidates => return None,
        })
    }
}

/// One pile of a split.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Pile {
    /// Where its cards go; none where they stay where they lie, as the cards a put-back
    /// leaves in the hand do.
    pub place: Option<PlaceName>,
    pub label: String,
    /// The cards in it: where the place keeps an order, as they will lie, the top first.
    pub cards: Vec<ObjectId>,
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
    pub from: ObjectId,
    pub to: Subject,
}

/// Where a pile's cards go ([`Place`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PlaceName {
    Top,
    OnTop,
    Bottom,
    Hand,
    Graveyard,
    Exile,
    Battlefield,
}

impl PlaceName {
    /// The place `place` names, if any: [`Place::None`] names none.
    pub fn of(place: Place) -> Option<PlaceName> {
        Some(match place {
            Place::None => return None,
            Place::Top => PlaceName::Top,
            Place::OnTop => PlaceName::OnTop,
            Place::Bottom => PlaceName::Bottom,
            Place::Hand => PlaceName::Hand,
            Place::Graveyard => PlaceName::Graveyard,
            Place::Exile => PlaceName::Exile,
            Place::Battlefield => PlaceName::Battlefield,
        })
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
    /// made on. A click that opens another prompt over this one returns that prompt, as the
    /// next state: a card with several ways to use it, clicked at a priority, opens the choice
    /// among them, and the button that closes it returns the priority's prompt.
    Open { prompt: Box<PromptView> },
    /// The click answered the prompt: the session has sent the engine its picks, and the
    /// game's next views arrive as updates, as after any answer.
    Answered,
}
