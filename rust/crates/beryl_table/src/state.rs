use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Action;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Seat(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ObjectId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Card {
    Named(String),
    Custom {
        name: String,
        types: String,
        #[serde(default)]
        colors: Vec<String>,
        stats: Option<(i32, i32)>,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Face {
    #[default]
    Front,
    Back,
    Down,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Row {
    #[default]
    Auto,
    Creatures,
    Others,
    Lands,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Zone {
    Library(Seat),
    Hand(Seat),
    Graveyard(Seat),
    Exile(Seat),
    Sideboard(Seat),
    Battlefield,
    Stack,
}

impl Zone {
    pub fn is_public(self) -> bool {
        matches!(
            self,
            Self::Graveyard(_) | Self::Exile(_) | Self::Battlefield | Self::Stack
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Object {
    pub id: ObjectId,
    pub card: Card,
    pub owner: Seat,
    pub controller: Seat,
    pub zone: Zone,
    pub face: Face,
    pub tapped: bool,
    pub token: bool,
    pub counters: Vec<(String, i32)>,
    pub stats: Option<(i32, i32)>,
    pub damage: i32,
    pub note: Option<String>,
    pub attached_to: Option<ObjectId>,
    pub doesnt_untap: bool,
    pub row: Row,
    pub known_to: Vec<Seat>,
}

impl Object {
    pub fn new(id: ObjectId, card: Card, owner: Seat, zone: Zone) -> Self {
        Self {
            id,
            card,
            owner,
            controller: owner,
            zone,
            face: Face::Front,
            tapped: false,
            token: false,
            counters: Vec::new(),
            stats: None,
            damage: 0,
            note: None,
            attached_to: None,
            doesnt_untap: false,
            row: Row::Auto,
            known_to: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbilityMarker {
    pub source: ObjectId,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub life: i32,
    pub counters: Vec<(String, i32)>,
}

impl Player {
    pub fn new(life: i32) -> Self {
        Self {
            life,
            counters: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Untap,
    Upkeep,
    Draw,
    FirstMain,
    BeginningOfCombat,
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage,
    EndOfCombat,
    SecondMain,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrowTarget {
    Object(ObjectId),
    Player(Seat),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrowKind {
    Attack,
    Block,
    FriendlyTarget,
    HostileTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arrow {
    pub from: ObjectId,
    pub to: ArrowTarget,
    pub by: Seat,
    pub kind: ArrowKind,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchSeat {
    pub deck: Vec<(String, u16)>,
    pub sideboard: Vec<(String, u16)>,
    pub ready: bool,
    pub wins: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Match {
    pub seats: Vec<MatchSeat>,
    pub starting_life: i32,
    pub starting_hand_size: u16,
    pub in_game: bool,
    /// The latest game's result, used to offer the losing seat the next choice.
    #[serde(default)]
    pub last_winner: Option<Seat>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Assist {
    TurnUpkeep,
    Combat,
    StateNudges,
    LibraryFlows,
    AbilityMenus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub assist: Assist,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub text: String,
    pub whose: Seat,
    pub on: bool,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatZones {
    pub library: Vec<ObjectId>,
    pub hand: Vec<ObjectId>,
    pub graveyard: Vec<ObjectId>,
    pub exile: Vec<ObjectId>,
    pub sideboard: Vec<ObjectId>,
}

/// Zone vectors hold object ids in top-to-bottom order. On the stack an id can
/// refer to a card in `objects` or to a marker in `markers`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub players: Vec<Player>,
    pub zones: Vec<SeatZones>,
    pub battlefield: Vec<ObjectId>,
    pub stack: Vec<ObjectId>,
    pub objects: BTreeMap<ObjectId, Object>,
    pub markers: BTreeMap<ObjectId, AbilityMarker>,
    pub active: Seat,
    pub turn: u32,
    pub phase: Phase,
    pub arrows: Vec<Arrow>,
    pub game_match: Match,
    pub proposals: Vec<Proposal>,
    pub assists: Vec<Vec<Assist>>,
    pub lent_zones: Vec<(Zone, Seat)>,
    pub always_reveal_top: Vec<Seat>,
    pub always_look_at_top: Vec<Seat>,
    /// Monotonic across undo so a view never confuses two cards with the same id.
    pub(crate) next_id: u64,
    /// Counts committed actions, including actions which produced no changes.
    pub(crate) entry_number: u32,
}

impl Table {
    pub fn new(seat_count: u8) -> Self {
        let count = usize::from(seat_count);
        let starting_life = 20;
        Self {
            players: vec![Player::new(starting_life); count],
            zones: vec![SeatZones::default(); count],
            battlefield: Vec::new(),
            stack: Vec::new(),
            objects: BTreeMap::new(),
            markers: BTreeMap::new(),
            active: Seat(0),
            turn: 0,
            phase: Phase::Untap,
            arrows: Vec::new(),
            game_match: Match {
                seats: vec![MatchSeat::default(); count],
                starting_life,
                starting_hand_size: 7,
                in_game: false,
                last_winner: None,
            },
            proposals: Vec::new(),
            assists: vec![Vec::new(); count],
            lent_zones: Vec::new(),
            always_reveal_top: Vec::new(),
            always_look_at_top: Vec::new(),
            next_id: 1,
            entry_number: 0,
        }
    }

    pub fn seats(&self) -> impl Iterator<Item = Seat> + '_ {
        (0..self.players.len()).map(|index| Seat(index as u8))
    }

    pub fn zone(&self, zone: Zone) -> Option<&[ObjectId]> {
        Some(match zone {
            Zone::Battlefield => &self.battlefield,
            Zone::Stack => &self.stack,
            Zone::Library(seat) => &self.zones.get(usize::from(seat.0))?.library,
            Zone::Hand(seat) => &self.zones.get(usize::from(seat.0))?.hand,
            Zone::Graveyard(seat) => &self.zones.get(usize::from(seat.0))?.graveyard,
            Zone::Exile(seat) => &self.zones.get(usize::from(seat.0))?.exile,
            Zone::Sideboard(seat) => &self.zones.get(usize::from(seat.0))?.sideboard,
        })
    }

    pub(crate) fn zone_mut(&mut self, zone: Zone) -> Option<&mut Vec<ObjectId>> {
        Some(match zone {
            Zone::Battlefield => &mut self.battlefield,
            Zone::Stack => &mut self.stack,
            Zone::Library(seat) => &mut self.zones.get_mut(usize::from(seat.0))?.library,
            Zone::Hand(seat) => &mut self.zones.get_mut(usize::from(seat.0))?.hand,
            Zone::Graveyard(seat) => &mut self.zones.get_mut(usize::from(seat.0))?.graveyard,
            Zone::Exile(seat) => &mut self.zones.get_mut(usize::from(seat.0))?.exile,
            Zone::Sideboard(seat) => &mut self.zones.get_mut(usize::from(seat.0))?.sideboard,
        })
    }

    pub fn position(&self, id: ObjectId) -> Option<(Zone, usize)> {
        let zone = if let Some(object) = self.objects.get(&id) {
            object.zone
        } else if self.markers.contains_key(&id) {
            Zone::Stack
        } else {
            return None;
        };
        Some((
            zone,
            self.zone(zone)?
                .iter()
                .position(|&candidate| candidate == id)?,
        ))
    }

    /// Returns an unused id without consuming it. An action assembling several
    /// creations or replacements requests successive offsets.
    pub fn fresh_id(&self, offset: usize) -> Option<ObjectId> {
        let highest = self
            .objects
            .last_key_value()
            .map_or(0, |(id, _)| id.0)
            .max(self.markers.last_key_value().map_or(0, |(id, _)| id.0));
        let start = self.next_id.max(u64::from(highest) + 1);
        let next = start.checked_add(u64::try_from(offset).ok()?)?;
        Some(ObjectId(u32::try_from(next).ok()?))
    }
}
