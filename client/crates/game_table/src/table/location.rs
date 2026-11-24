//! Maps game locations to the toolkit's labelled places.

use beryl_protocol::protocol::{RowName, Side, ZoneName};
use kit_frame::frame::Place as FramePlace;

const OPPONENT_STACK: u8 = 1;
const REVEALED: &str = "revealed";

/// A card's zone, side and row, or a location outside the game board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Layer,
    Card {
        zone: ZoneName,
        side: Side,
        row: Option<RowName>,
    },
    Revealed,
}

macro_rules! with_rows {
    ($prefix:expr) => {
        [
            $prefix,
            concat!($prefix, ".creatures"),
            concat!($prefix, ".others"),
            concat!($prefix, ".lands"),
        ]
    };
}

// The stack's labels are shared by both players.
macro_rules! zone_labels {
    (@sides owned $word:literal) => {
        [
            with_rows!(concat!($word, ".yours")),
            with_rows!(concat!($word, ".theirs")),
        ]
    };
    (@sides shared $word:literal) => {
        [with_rows!($word); 2]
    };
    ($($zone:ident => $scope:ident $word:literal),* $(,)?) => {
        const fn labels(zone: ZoneName) -> &'static [[&'static str; 4]; 2] {
            match zone {
                $(ZoneName::$zone => &zone_labels!(@sides $scope $word),)*
            }
        }

        fn zone_named(word: &str) -> Option<ZoneName> {
            match word {
                $($word => Some(ZoneName::$zone),)*
                _ => None,
            }
        }
    };
}

zone_labels! {
    Library => owned "library",
    Hand => owned "hand",
    Battlefield => owned "battlefield",
    Graveyard => owned "graveyard",
    Exile => owned "exile",
    Stack => shared "stack",
    Nowhere => owned "nowhere",
    Sideboard => owned "sideboard",
    Merged => owned "merged",
}

impl Place {
    /// The label printed for this place. Both players share one stack in the tree.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Layer => "",
            Self::Revealed => REVEALED,
            Self::Card { zone, side, row } => {
                let row = match row {
                    None => 0,
                    Some(RowName::Creatures) => 1,
                    Some(RowName::Others) => 2,
                    Some(RowName::Lands) => 3,
                };
                labels(zone)[usize::from(side == Side::Opponent)][row]
            }
        }
    }

    /// Reads a tree or story label. A stack label denotes the shared stack.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        if label == REVEALED {
            return Some(Self::Revealed);
        }
        let mut parts = label.split('.');
        let zone = zone_named(parts.next()?)?;
        let side = if zone == ZoneName::Stack {
            Side::You
        } else {
            match parts.next()? {
                "yours" => Side::You,
                "theirs" => Side::Opponent,
                _ => return None,
            }
        };
        let row = match parts.next() {
            None => None,
            Some("creatures") => Some(RowName::Creatures),
            Some("others") => Some(RowName::Others),
            Some("lands") => Some(RowName::Lands),
            _ => return None,
        };
        (parts.next().is_none()).then_some(Self::Card { zone, side, row })
    }

    /// Recovers a game location from a frame element without losing the owner of a stack object.
    #[must_use]
    pub fn in_frame(place: FramePlace) -> Option<Self> {
        if place == FramePlace::Layer {
            return Some(Self::Layer);
        }
        let mut location = Self::from_label(place.label())?;
        if let Self::Card {
            zone: ZoneName::Stack,
            side,
            ..
        } = &mut location
            && place.variant() == OPPONENT_STACK
        {
            *side = Side::Opponent;
        }
        Some(location)
    }
}

impl From<Place> for FramePlace {
    fn from(place: Place) -> Self {
        match place {
            Place::Layer => Self::Layer,
            Place::Card {
                zone: ZoneName::Stack,
                side: Side::Opponent,
                ..
            } => Self::with_variant(place.label(), OPPONENT_STACK),
            _ => Self::named(place.label()),
        }
    }
}
