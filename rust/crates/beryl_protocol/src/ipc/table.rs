//! Commands for opening and configuring linked manual tables.

use beryl_table::state::Seat;
// The assists and settings a lobby names, for clients that do not depend on beryl_table.
pub use beryl_table::state::{Assist, LobbySettings};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const TABLE_HOST: &str = "table_host";
pub const TABLE_JOIN: &str = "table_join";
pub const TABLE_RESUME: &str = "table_resume";
pub const TABLE_SAVED: &str = "table_saved";
pub const TABLE_SETTINGS_GET: &str = "table_settings_get";
pub const TABLE_SETTINGS_SET: &str = "table_settings_set";
pub const TABLE_LINK: &str = "table-link";
pub const TABLE_LOBBY_COMMAND: &str = "table_lobby";
pub const TABLE_LOBBY: &str = "table-lobby";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TableLink {
    Iroh,
    Local,
    /// Available only in the local HTTP test server.
    Memory,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TableHostSettings {
    pub name: String,
    #[ts(optional)]
    pub starting_life: Option<i32>,
    #[ts(optional)]
    pub starting_hand_size: Option<u16>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TableHostRequest {
    pub link: TableLink,
    pub settings: TableHostSettings,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TableHostReply {
    pub invite: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TableJoinRequest {
    pub invite: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TableResumeRequest {
    pub id: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SavedTable {
    pub id: String,
    pub host: String,
    pub guest: Option<String>,
}

/// Profile settings for linked tables. A null relay selects iroh's public relays.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TableSettings {
    pub relay: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TableConnectionError {
    pub reason: String,
}

impl TableConnectionError {
    pub fn from_error(error: impl std::fmt::Display) -> Self {
        Self {
            reason: error.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LobbySeat {
    pub seat: Seat,
    pub name: Option<String>,
    pub loaded: bool,
    pub main_count: u32,
    pub side_count: u32,
    pub ready: bool,
    pub wins: u32,
}

/// A seat's own deck and the public information about the table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LobbyView {
    pub you: Seat,
    pub seats: Vec<LobbySeat>,
    pub main: Vec<(String, u16)>,
    pub side: Vec<(String, u16)>,
    pub unknown: Vec<String>,
    pub assists: Vec<Assist>,
    pub settings: LobbySettings,
    pub game_number: u32,
    pub last_winner: Option<Seat>,
    pub invite: Option<String>,
    pub can_start: bool,
}

/// A null view means the current table has started playing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LobbyUpdate {
    pub view: Option<LobbyView>,
    // The lobby shown as information while the game is in progress.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub game: Option<Box<LobbyView>>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TableLobbyRequest {
    pub change: LobbyChange,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LobbyChange {
    View,
    Solo,
    LoadDeck {
        main: Vec<(String, u16)>,
        side: Vec<(String, u16)>,
    },
    Sideboard {
        into_main: Vec<(String, u16)>,
        into_side: Vec<(String, u16)>,
    },
    Ready {
        ready: bool,
    },
    SetAssist {
        assist: Assist,
        on: bool,
    },
    Settings {
        settings: LobbySettings,
    },
    Start,
}
