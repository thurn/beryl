use serde::{Deserialize, Serialize};

use crate::state::{Assist, Card, Face, ObjectId, Phase, Row, Seat, Zone};

/// The ids and the zone they occupied when the sender chose them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub objects: Vec<ObjectId>,
    #[serde(rename = "in")]
    pub zone: Zone,
}

/// A secondary target carries the same staleness information as the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenObject {
    pub id: ObjectId,
    #[serde(rename = "in")]
    pub zone: Zone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Object(SeenObject),
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
pub enum Action {
    Move {
        #[serde(flatten)]
        selection: Selection,
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
        selection: Selection,
        position: Position,
    },
    Pin {
        #[serde(flatten)]
        selection: Selection,
        row: Row,
    },
    Give {
        #[serde(flatten)]
        selection: Selection,
        to: Seat,
    },
    Tap {
        #[serde(flatten)]
        selection: Selection,
        tapped: bool,
    },
    Face {
        #[serde(flatten)]
        selection: Selection,
        face: Face,
    },
    Flag {
        #[serde(flatten)]
        selection: Selection,
        doesnt_untap: bool,
    },
    Peek {
        #[serde(flatten)]
        selection: Selection,
    },
    Counter {
        #[serde(flatten)]
        selection: Selection,
        name: String,
        #[serde(flatten)]
        adjustment: Adjustment,
    },
    Stats {
        #[serde(flatten)]
        selection: Selection,
        stats: Option<(i32, i32)>,
    },
    Damage {
        #[serde(flatten)]
        selection: Selection,
        damage: i32,
    },
    Note {
        #[serde(flatten)]
        selection: Selection,
        note: Option<String>,
    },
    Attach {
        #[serde(flatten)]
        selection: Selection,
        host: SeenObject,
    },
    Unattach {
        #[serde(flatten)]
        selection: Selection,
    },
    Clone {
        #[serde(flatten)]
        selection: Selection,
    },
    Token {
        card: Card,
        count: u16,
        attached_to: Option<SeenObject>,
        note: Option<String>,
    },
    Marker {
        #[serde(flatten)]
        selection: Selection,
        text: String,
    },
    Remove {
        #[serde(flatten)]
        selection: Selection,
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
    Reveal {
        #[serde(rename = "in")]
        zone: Zone,
        /// With neither objects nor a count, reveal the whole zone.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        objects: Option<Vec<ObjectId>>,
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
        selection: Selection,
        to: Target,
    },
    ClearArrows {
        from: Option<SeenObject>,
        to: Option<Target>,
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
    Start {
        first: Seat,
    },
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
    },
    Undo {},
    Rewind {
        to: u32,
    },
    Batch {
        actions: Vec<Action>,
    },
}
