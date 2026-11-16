//! Dropping cards on the manual table (docs/manual-multiplayer.md, "Drag"): while a card is
//! held, the place it would land is outlined, and letting it go there sends the actions the
//! web's `dropAction` (`ui/src/table/dropActions.ts`) makes. Where a card lands is found as the
//! web's `destinationAt` (`ui/src/table/dropZones.ts`) finds it: a pile under the pointer, then
//! a card on the battlefield (its row, beside it), in a hand or on the stack, and otherwise the
//! area of the table the pointer is over (`desktopDropAreas`): each side's hand, its creatures'
//! row, and the back row's lands and other permanents. Several cards dropped on the top of a
//! library open the order tray instead (`super::order`).

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{ObjectId, ObjectView, RowName, Side, TableAction, ZoneName};
use beryl_protocol::script::Subject;
use glam::{Vec2, Vec3};
use serde_json::Value;

use super::commands::seat_of;
use super::viewers::inside;
use crate::data::space::flat::table::drop as data;
use crate::element::{Align, Anchor, Element, Justify, column, text};
use crate::frame::{Frame, Place, Role, StateBits};
use crate::input::Rect;
use crate::table::{Camera, Layout};
use crate::tokens::{color, type_style};

/// What a dropped card lands in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Pile(ZoneName),
    Hand,
    Stack,
    Row(RowName),
}

/// Which end of a library a card dropped on it goes to: the upper half of the library's box is
/// its top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Top,
    Bottom,
}

/// Where a held card would land: what, whose, the box outlined on the screen, the end of a
/// library, and the permanent it was dropped on, if any.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Destination {
    pub area: Area,
    pub side: Side,
    pub rect: Rect,
    pub end: Option<End>,
    pub on: Option<ObjectId>,
}

/// The name of the outline of the place a held card would land.
pub const TARGET: &str = "drop-target";

/// Where cards `held` would land if let go at `at`, a point of the window, at `view`, as the
/// module describes. `None` over nothing a card can land in.
#[must_use]
pub fn destination_at(
    frame: &Frame,
    (layout, camera): (&Layout, &Camera),
    at: Vec2,
    held: &[ObjectId],
    view: &PlayerView,
) -> Option<Destination> {
    if let Some(found) = under(frame, at, held, view) {
        return Some(found);
    }
    Some(area_at(layout, camera, at))
}

/// The pile or card under `at` that a card lands in, the topmost first, leaving out the held
/// cards themselves.
fn under(frame: &Frame, at: Vec2, held: &[ObjectId], view: &PlayerView) -> Option<Destination> {
    for element in frame.resolved.iter().rev() {
        let Place::Card { zone, side, row } = element.place else {
            continue;
        };
        if !matches!(element.role, Role::Card | Role::Pile)
            || element.state.0 & StateBits::HELD != 0
            || !inside(&element.corners, at)
        {
            continue;
        }
        let rect = bounds(&element.corners);
        let slot = u16::try_from(element.id.0).ok().map(ObjectId);
        let found = |area| Destination {
            area,
            side,
            rect,
            end: None,
            on: None,
        };
        match zone {
            ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile => {
                return Some(located(found(Area::Pile(zone)), at));
            }
            _ if element.role == Role::Pile => {}
            _ if slot.is_some_and(|slot| held.contains(&slot)) => {}
            ZoneName::Battlefield => {
                let object = slot.and_then(|slot| object(view, slot));
                let row = row.unwrap_or(RowName::Creatures);
                return Some(Destination {
                    side: object.map_or(side, |object| object.controller),
                    on: slot,
                    ..found(Area::Row(row))
                });
            }
            ZoneName::Hand => return Some(found(Area::Hand)),
            ZoneName::Stack => {
                return Some(Destination {
                    side: Side::You,
                    ..found(Area::Stack)
                });
            }
            _ => {}
        }
    }
    None
}

/// The area of the table under `at`: each side's hand along its edge of the window, and on the
/// table each side's creatures' row toward the middle and its back row beyond, the lands on the
/// left and the other permanents on the right, as `desktopDropAreas` divides the stage.
fn area_at(layout: &Layout, camera: &Camera, at: Vec2) -> Destination {
    let window = camera.window.size;
    let hand_top = f32::midpoint(1.0, -layout.hand.edge) * window.height;
    let strip = data::OPPONENT_HAND * window.height;
    let whole = |top: f32, bottom: f32| Rect {
        x: 0.0,
        y: top,
        width: window.width,
        height: bottom - top,
    };
    let hand = |side, rect| Destination {
        area: Area::Hand,
        side,
        rect,
        end: None,
        on: None,
    };
    if at.y >= hand_top {
        return hand(Side::You, whole(hand_top, window.height));
    }
    if at.y <= strip {
        return hand(Side::Opponent, whole(0.0, strip));
    }
    let point = camera.on_plane(at, 0.0);
    let side = if point.y < 0.0 {
        Side::You
    } else {
        Side::Opponent
    };
    // The creatures' row runs from the middle of the table to half way to the back row, and
    // the back row as far again beyond it. The lands' and the others' halves meet half way
    // between them.
    let (near, back) = (layout.creatures.y.abs(), layout.lands.y.abs());
    let split = f32::midpoint(near, back);
    let far = 2.0 * back - split;
    let middle = f32::midpoint(layout.lands.right, layout.others.left);
    let (left, right) = (layout.creatures.left, layout.creatures.right);
    let (row, band, across) = if point.y.abs() < split {
        (RowName::Creatures, (0.0, split), (left, right))
    } else if point.x < middle {
        (RowName::Lands, (split, far), (left, middle))
    } else {
        (RowName::Others, (split, far), (middle, right))
    };
    let sign = if side == Side::You { -1.0 } else { 1.0 };
    let corners = [
        (across.0, band.0),
        (across.1, band.0),
        (across.1, band.1),
        (across.0, band.1),
    ]
    .map(|(x, y)| camera.to_screen(Vec3::new(x, sign * y, 0.0)));
    let mut rect = bounds(&corners);
    // A row's box stops at the hands' edges of the window.
    let top = rect.y.max(strip);
    let bottom = (rect.y + rect.height).min(hand_top);
    rect.y = top;
    rect.height = (bottom - top).max(0.0);
    Destination {
        area: Area::Row(row),
        side,
        rect,
        end: None,
        on: None,
    }
}

/// A library's destination with the end of it that `at` is over.
fn located(found: Destination, at: Vec2) -> Destination {
    if found.area != Area::Pile(ZoneName::Library) {
        return found;
    }
    let end = if at.y < found.rect.y + found.rect.height / 2.0 {
        End::Top
    } else {
        End::Bottom
    };
    Destination {
        end: Some(end),
        ..found
    }
}

/// The box on the screen round `corners`.
fn bounds(corners: &[Vec2; 4]) -> Rect {
    let low = corners
        .iter()
        .fold(Vec2::INFINITY, |low, &corner| low.min(corner));
    let high = (corners.iter()).fold(Vec2::NEG_INFINITY, |high, &corner| high.max(corner));
    Rect {
        x: low.x,
        y: low.y,
        width: high.x - low.x,
        height: high.y - low.y,
    }
}

/// Whether `cards` may land at `to` at `view` (`available` in dropActions.ts): at a table of one
/// only the stack is open on the opponent's side, and a hidden zone, a library or a hand, takes
/// only cards its owner or a lent zone lets in (`canMoveInto` in `ui/src/table/zoneAccess.ts`).
#[must_use]
pub fn available(view: &PlayerView, to: &Destination, cards: &[ObjectId]) -> bool {
    let Some(table) = &view.table else {
        return false;
    };
    if to.side == Side::Opponent && table.seats != 2 {
        return to.area == Area::Stack;
    }
    let zone = match to.area {
        Area::Pile(ZoneName::Library) => ZoneName::Library,
        Area::Hand => ZoneName::Hand,
        _ => return true,
    };
    let lent = (table.acting.iter()).any(|grant| grant.side == to.side && grant.zone == zone);
    to.side == Side::You
        || lent
        || cards.iter().all(|&slot| {
            object(view, slot).is_some_and(|card| {
                card.owner == to.side || (card.zone == zone && card.zone_side == Some(to.side))
            })
        })
}

/// The actions that put `cards` where they land at `to`, let go at `at`, at `view`, whose
/// battlefield `frame` shows (`dropAction` in dropActions.ts). On the battlefield, cards from
/// elsewhere enter it; a permanent attached to something else comes off; one the other side
/// controls changes control; one from another row is pinned to this one; and one already in this
/// row moves to where it was let go among the row's other cards. Elsewhere each run of cards
/// from one zone moves as one action, to the top of a pile or library, the bottom of a hand, or
/// the end of the library it was let go over, in the order the cards were taken, the first on
/// top.
#[must_use]
pub fn actions(
    view: &PlayerView,
    frame: &Frame,
    cards: &[ObjectId],
    to: &Destination,
    at: Vec2,
) -> Vec<TableAction> {
    let Some(table) = &view.table else {
        return Vec::new();
    };
    let found: Vec<(ObjectId, Value, Value)> = (cards.iter())
        .filter_map(|&slot| {
            let action = (table.object_actions.iter())
                .find(|action| action.subject == Subject::Object { slot })?;
            let reference = serde_json::to_value(&action.reference).ok()?;
            let zone = serde_json::to_value(action.zone).ok()?;
            Some((slot, reference, zone))
        })
        .collect();
    let zone = match to.area {
        Area::Stack => Value::from("stack"),
        Area::Hand => serde_json::json!({ "hand": seat_of(view, to.side) }),
        Area::Pile(pile) => keyed(word(pile), seat_of(view, to.side).into()),
        Area::Row(row) => return battlefield(view, frame, &found, to, (row, at)),
    };
    let end = to.end.unwrap_or(if to.area == Area::Hand {
        End::Bottom
    } else {
        End::Top
    });
    let mut groups = runs(&found);
    // Each move puts its run at the same end of the pile, so for the top the last run moves
    // first and the first run ends on top.
    if end == End::Top {
        groups.reverse();
    }
    let position = if end == End::Top { "top" } else { "bottom" };
    groups
        .into_iter()
        .filter_map(|(zone_of, objects)| {
            let mut fields = serde_json::json!({ "objects": objects, "in": zone_of, "to": zone });
            if end == End::Bottom {
                fields["position"] = position.into();
            }
            action(serde_json::json!({ "move": fields }))
        })
        .collect()
}

/// The one action a drop sends for its `actions`: the only one, or a batch of several, as the
/// web's `dropAction` sends a drop (`batch` in dropActions.ts), so that the table applies or
/// refuses the whole drop at once and its log shows one line that one undo takes back.
#[must_use]
pub fn batch(mut actions: Vec<TableAction>) -> Option<TableAction> {
    if actions.len() > 1 {
        Some(TableAction::Batch { actions })
    } else {
        actions.pop()
    }
}

/// The actions that put `found` cards on the battlefield in `row`, let go at `at`
/// (`placement` in dropActions.ts, without attaching a card to the permanent it was dropped on,
/// which the table's Attach command does).
fn battlefield(
    view: &PlayerView,
    frame: &Frame,
    found: &[(ObjectId, Value, Value)],
    to: &Destination,
    (row, at): (RowName, Vec2),
) -> Vec<TableAction> {
    let on_battlefield = |zone: &Value| zone == "battlefield";
    let entering: Vec<_> = found
        .iter()
        .filter(|(.., zone)| !on_battlefield(zone))
        .collect();
    let arrivals: Vec<TableAction> = runs_of(&entering)
        .into_iter()
        .filter_map(|(zone, objects)| {
            action(serde_json::json!({ "move": {
                "objects": objects, "in": zone, "to": "battlefield",
            }}))
        })
        .collect();
    let permanents: Vec<(ObjectId, &Value, &ObjectView)> = (found.iter())
        .filter(|(.., zone)| on_battlefield(zone))
        .filter_map(|(slot, reference, _)| Some((*slot, reference, object(view, *slot)?)))
        .collect();
    let moving = |pick: &dyn Fn(&ObjectView) -> bool| -> Vec<Value> {
        (permanents.iter())
            .filter(|(.., object)| pick(object))
            .map(|(_, reference, _)| (*reference).clone())
            .collect()
    };
    let mut actions = Vec::new();
    let mut push = |kind: &str, objects: Vec<Value>, extra: Option<(&str, Value)>| {
        if objects.is_empty() {
            return;
        }
        let mut fields = serde_json::json!({ "objects": objects, "in": "battlefield" });
        if let Some((name, value)) = extra {
            fields[name] = value;
        }
        actions.extend(action(keyed(kind, fields)));
    };
    push(
        "unattach",
        moving(&|object| object.attached_to.is_some_and(|host| Some(host) != to.on)),
        None,
    );
    push(
        "give",
        moving(&|object| object.controller != to.side),
        Some(("to", seat_of(view, to.side).into())),
    );
    let local = |object: &ObjectView| object.controller == to.side && object.attached_to.is_none();
    let row_of = |slot: ObjectId| row_in(frame, slot);
    let pinned: Vec<Value> = (permanents.iter())
        .filter(|(slot, _, object)| local(object) && row_of(*slot) != Some(row))
        .map(|(_, reference, _)| (*reference).clone())
        .collect();
    push("pin", pinned, Some(("row", row_word(row).into())));
    let reordered: Vec<ObjectId> = (permanents.iter())
        .filter(|(slot, _, object)| local(object) && row_of(*slot) == Some(row))
        .map(|(slot, ..)| *slot)
        .collect();
    if let Some(nth) = place_in_row(view, frame, (&reordered, row, to.side), at) {
        let objects: Vec<Value> = (permanents.iter())
            .filter(|(slot, ..)| reordered.contains(slot))
            .map(|(_, reference, _)| (*reference).clone())
            .collect();
        push(
            "reorder",
            objects,
            Some(("position", serde_json::json!({ "nth": nth }))),
        );
    }
    // A reorder counts places in the battlefield as it stands, so the cards from elsewhere
    // arrive after it.
    actions.extend(arrivals);
    actions
}

/// Where among `side`'s other cards in `row` the `moved` cards go, let go at `at`: before the
/// first card whose middle lies right of the pointer, or after the last, counted as places in
/// the view's battlefield with the moved cards left out. `None` when none moves or the row has
/// no other card.
fn place_in_row(
    view: &PlayerView,
    frame: &Frame,
    (moved, row, side): (&[ObjectId], RowName, Side),
    at: Vec2,
) -> Option<usize> {
    if moved.is_empty() {
        return None;
    }
    let order: Vec<ObjectId> = (view.battlefield.iter().copied())
        .filter(|slot| !moved.contains(slot))
        .collect();
    let mut neighbors: Vec<(usize, f32)> = (order.iter().enumerate())
        .filter_map(|(index, &slot)| {
            let object = object(view, slot)?;
            if object.attached_to.is_some() || object.controller != side {
                return None;
            }
            let element = card_element(frame, slot)?;
            let Place::Card {
                row: Some(found), ..
            } = element.place
            else {
                return None;
            };
            let middle = element.corners.iter().map(|corner| corner.x).sum::<f32>() / 4.0;
            (found == row).then_some((index, middle))
        })
        .collect();
    neighbors.sort_by(|a, b| a.1.total_cmp(&b.1));
    let before = neighbors.iter().find(|(_, x)| at.x < *x);
    match (before, neighbors.last()) {
        (Some((index, _)), _) => Some(*index),
        (None, Some((index, _))) => Some(index + 1),
        (None, None) => None,
    }
}

/// The row the card in `slot` lies in, as `frame` drew it.
fn row_in(frame: &Frame, slot: ObjectId) -> Option<RowName> {
    match card_element(frame, slot)?.place {
        Place::Card { row, .. } => row,
        Place::Layer => None,
    }
}

/// The card element of `slot` in `frame`.
fn card_element(frame: &Frame, slot: ObjectId) -> Option<&crate::frame::Resolved> {
    (frame.resolved.iter())
        .find(|element| element.role == Role::Card && element.id.0 == u32::from(slot.0))
}

/// The cards in runs of one zone each, in order, each run's zone and references.
fn runs(found: &[(ObjectId, Value, Value)]) -> Vec<(Value, Vec<Value>)> {
    runs_of(&found.iter().collect::<Vec<_>>())
}

fn runs_of(found: &[&(ObjectId, Value, Value)]) -> Vec<(Value, Vec<Value>)> {
    let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
    for (_, reference, zone) in found {
        match groups.last_mut() {
            Some((last, objects)) if last == zone => objects.push(reference.clone()),
            _ => groups.push((zone.clone(), vec![reference.clone()])),
        }
    }
    groups
}

/// The object in `slot` at `view`.
fn object(view: &PlayerView, slot: ObjectId) -> Option<&ObjectView> {
    view.objects.iter().find(|object| object.slot == slot)
}

/// The JSON object whose one field is `key`, holding `value`.
fn keyed(key: &str, value: Value) -> Value {
    let mut object = serde_json::Map::new();
    object.insert(key.to_owned(), value);
    Value::Object(object)
}

/// `json` read as a table action, if it is one.
fn action(json: Value) -> Option<TableAction> {
    serde_json::from_value(json).ok()
}

/// The word an action names a pile by.
fn word(zone: ZoneName) -> &'static str {
    match zone {
        ZoneName::Library => "library",
        ZoneName::Exile => "exile",
        _ => "graveyard",
    }
}

/// The word an action names a battlefield row by.
fn row_word(row: RowName) -> &'static str {
    match row {
        RowName::Creatures => "creatures",
        RowName::Lands => "lands",
        RowName::Others => "others",
    }
}

/// The outline of where a held card would land, as the web's `.table-drop-target`: a bright
/// metal line round a faint glow, and over a library its two halves, Top and Bottom, the one
/// under the pointer lit. The pointer goes through it.
#[must_use]
pub fn target(to: &Destination) -> Element {
    let mut outline = column()
        .name(TARGET)
        .width(to.rect.width)
        .height(to.rect.height)
        .border((color::METAL_BRIGHT, data::RIM))
        .radius(data::RADIUS)
        .passes(true);
    outline.anchor = Some(Anchor::Point(Vec2::new(to.rect.x, to.rect.y), Vec2::ZERO));
    let Some(end) = to.end else {
        return outline;
    };
    let half = |label: &str, over: bool| {
        let (face, ink) = if over {
            (color::METAL_BRIGHT, color::DROP_HALF_INK)
        } else {
            (color::DROP_HALF, color::METAL_BRIGHT)
        };
        column()
            .name(label)
            .height(to.rect.height / 2.0)
            .align_items(Align::Center)
            .justify_content(Justify::Center)
            .background(face)
            .passes(true)
            .child(text(label).style(type_style::BODY).color(ink))
    };
    outline
        .child(half("Top", end == End::Top))
        .child(half("Bottom", end == End::Bottom))
}
