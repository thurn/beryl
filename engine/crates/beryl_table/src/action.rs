use serde::{Deserialize, Serialize};

use crate::state::{Assist, Card, Face, LobbySettings, ObjectId, Phase, Row, Seat, Zone};

mod references;

/// The ids and the zone they occupied when the sender chose them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection<Id = ObjectId> {
    pub objects: Vec<Id>,
    #[serde(rename = "in")]
    pub zone: Zone,
}

/// A secondary target carries the same staleness information as the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenObject<Id = ObjectId> {
    pub id: Id,
    #[serde(rename = "in")]
    pub zone: Zone,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target<Id = ObjectId> {
    Object(SeenObject<Id>),
    Player(Seat),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Destination {
    Zone(Zone),
    Computed(ComputedDestination),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputedDestination {
    Play,
    Resolve,
}

/// `Nth(0)` means the top. The index is measured after excluding the moving cards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    #[default]
    Top,
    Bottom,
    Nth(usize),
}

impl Position {
    pub fn is_top(&self) -> bool {
        *self == Self::Top
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Adjustment {
    Add(i32),
    Set(i32),
}

impl Adjustment {
    pub(crate) fn apply(self, current: i32) -> i32 {
        match self {
            Self::Add(delta) => current.saturating_add(delta),
            Self::Set(value) => value,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Portion {
    #[default]
    All,
    Top(u16),
    Bottom(u16),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    Name,
    Type,
    ManaValue,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flow {
    Scry,
    Surveil,
    LookAtTop,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(bound(deserialize = "Id: Deserialize<'de>"))]
pub enum Action<Id = ObjectId> {
    Move {
        #[serde(flatten)]
        selection: Selection<Id>,
        to: Destination,
        #[serde(default, skip_serializing_if = "Position::is_top")]
        position: Position,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<Face>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tapped: Option<bool>,
    },
    Reorder {
        #[serde(flatten)]
        selection: Selection<Id>,
        position: Position,
    },
    Pin {
        #[serde(flatten)]
        selection: Selection<Id>,
        row: Row,
    },
    Give {
        #[serde(flatten)]
        selection: Selection<Id>,
        to: Seat,
    },
    Tap {
        #[serde(flatten)]
        selection: Selection<Id>,
        tapped: bool,
    },
    Face {
        #[serde(flatten)]
        selection: Selection<Id>,
        face: Face,
    },
    Flag {
        #[serde(flatten)]
        selection: Selection<Id>,
        doesnt_untap: bool,
    },
    Peek {
        #[serde(flatten)]
        selection: Selection<Id>,
    },
    Counter {
        #[serde(flatten)]
        selection: Selection<Id>,
        name: String,
        #[serde(flatten)]
        adjustment: Adjustment,
    },
    Stats {
        #[serde(flatten)]
        selection: Selection<Id>,
        stats: Option<(i32, i32)>,
    },
    Damage {
        #[serde(flatten)]
        selection: Selection<Id>,
        damage: i32,
    },
    Note {
        #[serde(flatten)]
        selection: Selection<Id>,
        note: Option<String>,
    },
    Attach {
        #[serde(flatten)]
        selection: Selection<Id>,
        host: SeenObject<Id>,
    },
    Unattach {
        #[serde(flatten)]
        selection: Selection<Id>,
    },
    Clone {
        #[serde(flatten)]
        selection: Selection<Id>,
    },
    Token {
        card: Card,
        count: u16,
        attached_to: Option<SeenObject<Id>>,
        note: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        face_down: bool,
    },
    Marker {
        #[serde(flatten)]
        selection: Selection<Id>,
        text: String,
    },
    Remove {
        #[serde(flatten)]
        selection: Selection<Id>,
    },
    StackUntil {
        seat: Seat,
        query: String,
        hits: u16,
    },
    Draw {
        seat: Seat,
        count: u16,
    },
    Mulligan {
        seat: Seat,
        count: u16,
    },
    Shuffle {
        seat: Seat,
        #[serde(default)]
        portion: Portion,
    },
    Look {
        seat: Seat,
        portion: Portion,
    },
    CloseLook {},
    Reveal {
        #[serde(rename = "in")]
        zone: Zone,
        /// With neither objects nor a count, reveal the whole zone.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        objects: Option<Vec<Id>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        random: Option<u16>,
        to: Vec<Seat>,
    },
    Lend {
        #[serde(rename = "in")]
        zone: Zone,
        to: Seat,
    },
    AlwaysRevealTop {
        seat: Seat,
        on: bool,
    },
    AlwaysLookAtTop {
        seat: Seat,
        on: bool,
    },
    SortHand {
        seat: Seat,
        by: SortBy,
    },
    Life {
        seat: Seat,
        #[serde(flatten)]
        adjustment: Adjustment,
    },
    PlayerCounter {
        seat: Seat,
        name: String,
        #[serde(flatten)]
        adjustment: Adjustment,
    },
    Phase {
        phase: Phase,
    },
    NextTurn {},
    Roll {
        count: u16,
        sides: u32,
    },
    Arrow {
        #[serde(flatten)]
        selection: Selection<Id>,
        to: Target<Id>,
    },
    ClearArrows {
        from: Option<SeenObject<Id>>,
        to: Option<Target<Id>>,
    },
    LoadDeck {
        seat: Seat,
        main: Vec<(String, u16)>,
        side: Vec<(String, u16)>,
    },
    Sideboard {
        seat: Seat,
        into_main: Vec<(String, u16)>,
        into_side: Vec<(String, u16)>,
    },
    Ready {
        seat: Seat,
        ready: bool,
    },
    GameSettings {
        settings: LobbySettings,
    },
    Start {
        first: Seat,
        // Deal the opening hands and leave the first turn for a phase action.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        opening: bool,
    },
    /// The host has selected who answers play or draw.
    ChooseFirst {
        chooser: Seat,
    },
    Keep {},
    NextGame {
        winner: Option<Seat>,
    },
    Concede {},
    SetAssist {
        assist: Assist,
        on: bool,
    },
    Flow {
        seat: Seat,
        flow: Flow,
        count: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<SeenObject<Id>>,
    },
    /// The host's completed sort, applied and undone as one batch of moves.
    FlowResult {
        flow: Flow,
        seat: Seat,
        on_top: Vec<Id>,
        elsewhere: Vec<Id>,
    },
    Undo {
        /// A seat can name its own draw or a batch from either seat.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entry: Option<u32>,
    },
    Rewind {
        to: u32,
    },
    Batch {
        actions: Vec<Action<Id>>,
    },
}
