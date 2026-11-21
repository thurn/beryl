//! The manual table's pile viewers (docs/manual-multiplayer.md, "Viewers"): the view commands
//! of a graveyard and an exile pile open its viewer, the card browser of `inspect::viewer`, as
//! the web's `TableViewers` (`ui/src/table/viewers.ts`) opens a `TablePileViewer`, and the
//! table's look commands open a library's once the table takes the look (`Look`). Each card in
//! the viewer is its scene object shown in a box (`Shown::Object`), and a right click on one
//! opens that card's context menu, with the commands of a card in a pile, as the web's viewer
//! cards carry the table's commands.
//!
//! The viewer can be searched, grouped by card type and sorted by name or mana value
//! (`PileViewer`, `sections`), and closing a search of a whole library asks whether to shuffle
//! it first (UI Conventions, "Manual table pile viewers").

use crate::inspect::Viewed;
use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{ObjectId, Side, TableAction, ZoneName};
use glam::Vec2;
use serde_json::{Value, json};

use super::Target;
use super::commands::seat_of;

use crate::faces::FaceList;
use crate::frame::{Frame, Role};
use crate::hud::piles::pile_cards;

/// The viewer's search field, by its name, which the web gives its search box as its label.
pub const SEARCH: &str = "Search cards";

/// The order the viewer lists its cards in: the pile's own, top first, or by name or by mana
/// value (the web's `Sort`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Pile,
    Name,
    ManaValue,
}

impl Sort {
    /// What the sort control says for this order, as the web's options say it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pile => "Pile order",
            Self::Name => "Sort by name",
            Self::ManaValue => "Sort by mana value",
        }
    }

    /// The order after this one, which a press on the sort control chooses.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Pile => Self::Name,
            Self::Name => Self::ManaValue,
            Self::ManaValue => Self::Pile,
        }
    }
}

/// The part of a library a look takes: all of it, or its top or bottom cards (the table's
/// `portion`, which the client reads from the action's JSON as `commands` writes actions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Portion {
    All,
    Top(u16),
    Bottom(u16),
}

impl Portion {
    /// The portion a table action's JSON writes as `portion`.
    fn read(value: &Value) -> Option<Self> {
        if value.as_str() == Some("all") {
            return Some(Self::All);
        }
        let count = |key| u16::try_from(value.get(key)?.as_u64()?).ok();
        count("top")
            .map(Self::Top)
            .or_else(|| count("bottom").map(Self::Bottom))
    }
}

/// A look at a library that the table took (`TableAction::Look`): whose library, which part of
/// it, and for a part, the cards that part held when the look began, so that a card that later
/// arrives in it is not listed, as the web's `PileView` keeps `slots`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    pub side: Side,
    pub portion: Portion,
    pub slots: Option<Vec<ObjectId>>,
}

impl Look {
    /// The look `action` asks for at `view`, if it is one.
    #[must_use]
    pub fn of(action: &TableAction, view: &PlayerView) -> Option<Self> {
        let value = serde_json::to_value(action).ok()?;
        let look = value.get("look")?;
        let seat = look.get("seat")?.as_u64()?;
        let portion = Portion::read(look.get("portion")?)?;
        let side = if seat == u64::from(seat_of(view, Side::You)) {
            Side::You
        } else {
            Side::Opponent
        };
        let library = pile_cards(view, side, ZoneName::Library);
        let slots = (portion != Portion::All).then(|| portion_slots(&library, portion).to_vec());
        Some(Self {
            side,
            portion,
            slots,
        })
    }

    /// The action that ends the look at `view`: `close_look`, after shuffling the whole
    /// library if `shuffle`, in one batch, as the web's `TableViewers.close` sends it.
    ///
    /// # Panics
    ///
    /// Never: both actions are ones the table reads.
    #[must_use]
    pub fn close(&self, view: &PlayerView, shuffle: bool) -> TableAction {
        let close = json!({ "close_look": {} });
        let action = if shuffle {
            let seat = seat_of(view, self.side);
            json!({ "batch": { "actions": [
                { "shuffle": { "seat": seat, "portion": "all" } },
                close,
            ] } })
        } else {
            close
        };
        serde_json::from_value(action).expect("the table reads close_look and shuffle")
    }

    /// The cards the look shows at `view`, the bottom one first as a library lists them.
    #[must_use]
    pub fn cards(&self, view: &PlayerView) -> Vec<ObjectId> {
        let library = pile_cards(view, self.side, ZoneName::Library);
        (portion_slots(&library, self.portion).iter().copied())
            .filter(|slot| (self.slots.as_ref()).is_none_or(|slots| slots.contains(slot)))
            .collect()
    }
}

/// The cards of `portion` of a library listed bottom first in `slots`, as the web's
/// `portionSlots` takes them.
#[must_use]
pub fn portion_slots(slots: &[ObjectId], portion: Portion) -> &[ObjectId] {
    match portion {
        Portion::All => slots,
        Portion::Top(count) => &slots[slots.len().saturating_sub(usize::from(count))..],
        Portion::Bottom(count) => &slots[..usize::from(count).min(slots.len())],
    }
}

/// What the client keeps of the open pile viewer beyond which pile it shows: the look it shows
/// if it is a library's, how it sorts and groups its cards, and whether it is asking whether
/// to shuffle the library before it closes. A viewer that opens starts again from the
/// default: the pile's order, ungrouped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PileViewer {
    pub look: Option<Look>,
    pub sort: Sort,
    pub grouped: bool,
    pub confirming: bool,
}

impl PileViewer {
    /// A viewer in the pile's order, ungrouped, showing no look.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            look: None,
            sort: Sort::Pile,
            grouped: false,
            confirming: false,
        }
    }

    /// Whether the viewer shows a search of a whole library whose player may shuffle it at
    /// `view` (the web's `canShuffle`), which closing it then offers.
    #[must_use]
    pub fn can_shuffle(&self, view: &PlayerView) -> bool {
        (self.look.as_ref())
            .is_some_and(|look| look.portion == Portion::All && uses_library(view, look.side))
    }
}

/// Whether the viewer may act on `side`'s library at `view`: their own, or the opponent's at a
/// table of two that lent it to them, as the web's `canUseZone` says.
fn uses_library(view: &PlayerView, side: Side) -> bool {
    let Some(table) = &view.table else {
        return false;
    };
    side == Side::You
        || (table.seats == 2
            && (table.acting.iter())
                .any(|grant| grant.side == side && grant.zone == ZoneName::Library))
}

/// What a client command does to the open viewer, as the web's `ViewerControls` and
/// `#finishLibrary` do: close it (asking first whether to shuffle a whole library), focus its
/// search field, turn grouping by type on or off, sort it, or close a whole library's search
/// with or without shuffling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerControl {
    Close,
    Search,
    Group,
    Sort(Sort),
    Finish { shuffle: bool },
}

/// The viewer control the client command `client` gives, if it is one.
#[must_use]
pub fn control(client: &str) -> Option<ViewerControl> {
    Some(match client {
        "close_viewer" => ViewerControl::Close,
        "search_viewer" => ViewerControl::Search,
        "group_viewer_by_type" => ViewerControl::Group,
        "sort_viewer_by_name" => ViewerControl::Sort(Sort::Name),
        "sort_viewer_by_mana_value" => ViewerControl::Sort(Sort::ManaValue),
        "close_and_shuffle" => ViewerControl::Finish { shuffle: true },
        "close_without_shuffle" => ViewerControl::Finish { shuffle: false },
        _ => return None,
    })
}

/// A run of the viewer's cards, under the card type they share while it groups them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub label: Option<String>,
    pub cards: Vec<ObjectId>,
}

/// The viewer's cards as it lists them, as the web's `sections` does: `cards` (listed bottom
/// first, as a pile lists them) top first, only those whose name holds `query` (in lower case),
/// sorted by `sort` (by name, or by mana value and then name), and with `grouped`, in a section
/// for each card type line's types, the sections in order of their labels.
#[must_use]
pub fn sections(
    view: &PlayerView,
    faces: Option<&FaceList>,
    cards: &[ObjectId],
    query: &str,
    sort: Sort,
    grouped: bool,
) -> Vec<Section> {
    let name = |slot: &ObjectId| {
        (view.objects.iter())
            .find(|object| object.slot == *slot)
            .and_then(|object| object.name.as_deref())
            .unwrap_or("")
    };
    let mut visible: Vec<ObjectId> = (cards.iter().rev())
        .filter(|slot| query.is_empty() || name(slot).to_lowercase().contains(query))
        .copied()
        .collect();
    let mana = |slot: &ObjectId| faces.and_then(|faces| faces.mana_value(name(slot)));
    match sort {
        Sort::Pile => {}
        Sort::Name => visible.sort_by(|a, b| name(a).cmp(name(b))),
        Sort::ManaValue => {
            visible.sort_by(|a, b| {
                (mana(a).unwrap_or(0), name(a)).cmp(&(mana(b).unwrap_or(0), name(b)))
            });
        }
    }
    if !grouped {
        return vec![Section {
            label: None,
            cards: visible,
        }];
    }
    let mut groups: Vec<Section> = Vec::new();
    for slot in visible {
        let label = type_group(view, faces, slot);
        match groups
            .iter_mut()
            .find(|group| group.label.as_ref() == Some(&label))
        {
            Some(group) => group.cards.push(slot),
            None => groups.push(Section {
                label: Some(label),
                cards: vec![slot],
            }),
        }
    }
    groups.sort_by(|a, b| a.label.cmp(&b.label));
    groups
}

/// The card types a card's section is named for, as the web's `typeGroup` finds them: the card
/// types its printed type line names before any dash, or else the types the view gives the
/// object, or "Other".
fn type_group(view: &PlayerView, faces: Option<&FaceList>, slot: ObjectId) -> String {
    // In lower case, as the words of a type line are compared, and so that no name here reads
    // as the clock types that the scene-purity check keeps out of the scene.
    const TYPES: [&str; 9] = [
        "artifact",
        "battle",
        "creature",
        "enchantment",
        "instant",
        "kindred",
        "land",
        "planeswalker",
        "sorcery",
    ];
    let object = view.objects.iter().find(|object| object.slot == slot);
    let printed = (object.and_then(|object| object.name.as_deref()))
        .and_then(|name| faces?.type_line(name))
        .map(|line| line.split('—').next().unwrap_or(line));
    let mut words: Vec<String> = (printed.into_iter().flat_map(str::split_whitespace))
        .filter(|word| TYPES.contains(&word.to_lowercase().as_str()))
        .map(str::to_owned)
        .collect();
    if words.is_empty() {
        words = (object.iter().flat_map(|object| &object.types))
            .map(|kind| format!("{kind:?}"))
            .collect();
    }
    if words.is_empty() {
        "Other".to_owned()
    } else {
        words.join(" ")
    }
}

/// The viewer the client command `client` opens for `target`, if it is a view command and the
/// target is a pile of the kind it views. A library is looked at through its own commands,
/// which the table answers, and a sideboard has no pile on the table to view.
#[must_use]
pub fn opens(client: &str, target: Target) -> Option<Viewed> {
    let zone = match client {
        "view_graveyard" => ZoneName::Graveyard,
        "view_exile" => ZoneName::Exile,
        _ => return None,
    };
    match target {
        Target::Pile(side, pile) if pile == zone => Some(Viewed::Pile(side, zone)),
        _ => None,
    }
}

/// Whether a press on `zone`'s pile on the table opens its viewer: a graveyard and an exile
/// pile, whose cards everyone may see. A library's cards are hidden, and the table shows them
/// only through its commands.
#[must_use]
pub fn opens_on_press(zone: ZoneName) -> bool {
    matches!(zone, ZoneName::Graveyard | ZoneName::Exile)
}

/// The card of the open viewer at `point` in `frame`, as a target, if the pointer is on one:
/// a card element whose slot `shown` says the viewer holds, the topmost first. A card in the
/// viewer lies in its pile as far as the view goes, so `target_at` would name the pile.
#[must_use]
pub fn card_at(frame: &Frame, point: Vec2, shown: impl Fn(ObjectId) -> bool) -> Option<Target> {
    (frame.resolved.iter().rev())
        .filter(|element| element.role == Role::Card && inside(&element.corners, point))
        .find_map(|element| {
            let slot = ObjectId(u16::try_from(element.id.0).ok()?);
            shown(slot).then_some(Target::Card(slot))
        })
}

/// Whether `point` lies inside the quadrilateral `corners`, given in order round its edge.
#[must_use]
pub fn inside(corners: &[Vec2; 4], point: Vec2) -> bool {
    // The point is inside when it lies on the same side of every edge, or on an edge.
    let sides = (0..4).map(|index| {
        let (a, b) = (corners[index], corners[(index + 1) % 4]);
        (b - a).perp_dot(point - a)
    });
    let (mut left, mut right) = (false, false);
    for cross in sides {
        left |= cross > 0.0;
        right |= cross < 0.0;
    }
    !(left && right)
}
