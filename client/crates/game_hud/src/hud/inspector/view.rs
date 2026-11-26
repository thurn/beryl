//! The inspector, which shows the card under the pointer at full size beside the table, with
//! what its face cannot say beneath it: a port of the web interface's `InspectAt` and
//! `inspectorPlace` (`ui/src/game/Inspector.tsx`) and its `Inspect` panel
//! (`ui/src/game/Piles.tsx`), with the state the web's `Look` (`ui/src/game/Look.tsx`) draws on
//! a card on the stack.
//!
//! The client shows it as a float of the flat layer (`floats::INSPECTOR`) rather than as a piece
//! of the game screen, so that it stands over the table whether a prompt is open or not and
//! never moves the prompt bar. It lets the pointer through, as the web's `.inspect` has
//! `pointer-events: none`, so the card under it stays hovered.
//!
//! Its geometry comes from `table.inspector` in the layout profile. The stage here is as tall as the window and as wide as the
//! window's shape makes it, as the tray's is (`table::tray`), and a place on it is turned into
//! logical points of the window.

use std::fmt::Write as _;

use beryl_protocol::protocol::{
    ObjectId, ObjectView, PaidCost, PlayerView, RefusalView, StackItem, ZoneName,
};
use glam::Vec2;

use game_table::{faces::FaceList, profiles::Profile};
use kit_base::data::Card;
use kit_base::data::profiles::Inspector;
use kit_base::tokens::{color, size, space, type_style};
use kit_frame::frame::{Anchor, ElementId, Role};
use kit_ui::element::{Element, Shown, column, row, text};
use kit_ui::input::{FaceId, FaceSize, Rect};

/// The card the pointer is over, which the inspector shows, and what decides where it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct Inspected {
    pub slot: ObjectId,
    /// The face it shows.
    pub face: FaceId,
    /// The middle of the card across the window, in logical points.
    pub x: f32,
    /// The top left of the projected box used when choosing the inspector slot.
    pub origin: Vec2,
    /// The boxes of the cards the open prompt marks, which the player may be about to pick and
    /// which the inspector keeps clear of where it can (the web's `keptClear`).
    pub targets: Vec<Rect>,
    /// Why the open decision refused the click on it, while the pointer has stayed on it
    /// since (`Client::refuse_click`).
    pub refusal: Option<RefusalView>,
}

/// The word at the permanent that holds its controller's opponents to sorcery speed: the web's
/// `TimingRefusal`.
pub(super) fn notice() -> Element {
    let mut word = row()
        .name("Sorcery speed only")
        .background(color::TIMING_REFUSAL)
        .border((color::STAT_DOWN, 1.0))
        .radius(size::TIMING_REFUSAL_RADIUS)
        .passes(true)
        .child(text("Sorcery speed only").style(type_style::TIMING_REFUSAL));
    let (across, down) = (
        size::TIMING_REFUSAL_PADDING_X,
        size::TIMING_REFUSAL_PADDING_Y,
    );
    word.style.padding = [down, across, down, across];
    word
}

/// The inspector's corner radius, `.inspect`'s `border-radius`.
const RADIUS: f32 = size::INSPECTOR_RADIUS;

/// The card details and geometry used to build the inspector.
pub struct CardInputs<'a> {
    inspected: &'a Inspected,
    object: &'a ObjectView,
    name: &'a str,
    data: Inspector,
    area: Rect,
    card: Card,
    place: Placed,
    costs: Option<(u8, &'a [PaidCost])>,
    printed: Option<[i32; 2]>,
    refused_by: Option<&'a str>,
}

/// Inputs that affect the inspector's card panel.
#[derive(PartialEq)]
pub struct CardKey {
    inspected: Inspected,
    object: ObjectView,
    data: Inspector,
    area: Rect,
    card: Card,
    place: Placed,
    costs: Option<(u8, Vec<PaidCost>)>,
    printed: Option<[i32; 2]>,
    refused_by: Option<String>,
}

impl<'a> CardInputs<'a> {
    #[must_use]
    pub fn new(
        inspected: &'a Inspected,
        profile: Profile,
        view: &'a PlayerView,
        faces: &FaceList,
    ) -> Option<Self> {
        let object = view
            .objects
            .iter()
            .find(|object| object.slot == inspected.slot)?;
        let name = object.name.as_deref()?;
        let costs = view.stack.iter().find_map(|item| match item {
            StackItem::Spell { slot, x, paid, .. } if *slot == object.slot => {
                Some((*x, paid.as_slice()))
            }
            _ => None,
        });
        let refused_by = match &inspected.refusal {
            Some(RefusalView::Timing { by: Some(by) }) => view
                .objects
                .iter()
                .find(|object| object.slot == *by)
                .and_then(|object| object.name.as_deref()),
            _ => None,
        };
        let data = profile.data().table.inspector?;
        let area = profile.slot("inspector")?.rect;
        let card = profile.card();
        let place = stage_place(
            area,
            (data, card),
            (inspected.x, inspected.origin),
            !view.stack.is_empty() || !inspected.targets.is_empty(),
            &inspected.targets,
        );
        Some(Self {
            inspected,
            object,
            name,
            data,
            area,
            card,
            place,
            costs,
            printed: faces.find(name).and_then(|face| faces.get(face)?.printed),
            refused_by,
        })
    }

    pub(super) fn key(&self) -> CardKey {
        CardKey {
            inspected: self.inspected.clone(),
            object: self.object.clone(),
            data: self.data,
            area: self.area,
            card: self.card,
            place: self.place,
            costs: self.costs.map(|(x, paid)| (x, paid.to_vec())),
            printed: self.printed,
            refused_by: self.refused_by.map(str::to_owned),
        }
    }

    pub(super) fn anchor(&self) -> Anchor {
        Anchor::new(
            ElementId(u32::from(self.inspected.slot.0)),
            Vec2::ZERO,
            Vec2::ZERO,
            self.place.offset,
        )
    }
}

/// The full-size face and its current state.
pub(super) fn inspector(input: &CardInputs<'_>) -> Element {
    let inspected = input.inspected;
    let name = input.name;
    let place = input.place;
    let data = input.data;
    let per_point = data.stage_height / input.area.height;
    let pad = data.pad / per_point;
    let inner = place.width - 2.0 * pad;
    let face = Element {
        shows: Some(Shown::Face(inspected.face, FaceSize::Full)),
        ..Element::new(Role::Region)
    }
    .name("face")
    .width(inner)
    .height(inner * input.card.height / input.card.width);
    let lines = state(input).into_iter().map(|line| {
        text(&line)
            .style(type_style::BODY)
            .color(color::TEXT_DIM)
            .max_width(inner)
    });
    column()
        .name(&format!("{name}, inspected"))
        .width(place.width)
        .padding(pad)
        .gap(space::S)
        .background(color::SURFACE_SUNK)
        .radius(RADIUS)
        .passes(true)
        .child(face)
        .children(lines)
        .children(
            inspected
                .refusal
                .as_ref()
                .map(|reason| refusal(reason, input.refused_by, inner)),
        )
}

/// The line under the state that says why a click on the card was refused.
fn refusal(reason: &RefusalView, by: Option<&str>, inner: f32) -> Element {
    let words = refusal_words(reason, by);
    let kind = ["Can't play", "Can't attack"][usize::from(reason.is_attack())];
    let mut line = row()
        .name(&format!("{kind}: {words}"))
        .justify_content(kit_ui::element::Justify::Center)
        .max_width(inner)
        .background(color::REFUSAL)
        .radius(size::REFUSAL_RADIUS)
        .child(
            text(&words)
                .style(type_style::REFUSAL)
                .color(color::STAT_DOWN),
        );
    let (x, y) = (size::REFUSAL_PADDING_X, size::REFUSAL_PADDING_Y);
    line.style.padding = [y, x, y, x];
    line
}

/// Why a card can't be played or a creature can't attack.
#[must_use]
pub fn refusal_words(reason: &RefusalView, by: Option<&str>) -> String {
    use RefusalView as R;
    match (reason, by) {
        (R::Timing { .. }, Some(by)) => {
            format!("Sorcery speed: {}", by.split(',').next().unwrap_or(by))
        }
        (R::Timing { .. }, None) => "Sorcery speed".to_owned(),
        (R::LandPlayed, _) => "Land played".to_owned(),
        (R::Mana, _) => "No mana".to_owned(),
        (R::Cost, _) => "Can't pay".to_owned(),
        (R::Target, _) => "No target".to_owned(),
        (R::Tapped, _) => "Tapped".to_owned(),
        (R::SummoningSickness, _) => "Summoning sickness".to_owned(),
        (R::CantAttack, _) => "Can't attack".to_owned(),
        (R::DefenderNeeds { subtype }, _) => format!("Defender needs {subtype}"),
    }
}

/// The lines under the face that say what the face cannot, in the web's order: its note, the
/// value it chose, the card a copy really is, that it is phased out, its owner when another
/// player controls it, its power and toughness against the printed ones, its counters, its
/// damage and summoning sickness, and on the stack, its X and the costs it was cast with.
fn state(input: &CardInputs<'_>) -> Vec<String> {
    let object = input.object;
    let mut lines = Vec::new();
    lines.extend(object.note.clone());
    lines.extend(
        object
            .chosen
            .as_ref()
            .map(|chosen| format!("Chosen: {chosen}")),
    );
    if let Some(type_line) = &object.type_line {
        lines.push(type_line.clone());
    }
    if let Some(rules) = &object.text {
        for line in rules.lines().filter(|line| !line.is_empty()) {
            lines.push(line.to_owned());
        }
    }
    lines.extend((object.really.as_ref()).map(|really| format!("Copy. Card: {really}")));
    if object.phased_out {
        lines.push("Phased out".to_owned());
    }
    if object.zone == ZoneName::Battlefield {
        if object.owner != object.controller {
            let owner = match object.owner {
                beryl_protocol::protocol::Side::You => "You",
                beryl_protocol::protocol::Side::Opponent => "Opponent",
            };
            lines.push(format!("Owner: {owner}"));
        }
        if let (Some(pt), Some([power, toughness])) = (&object.pt, input.printed)
            && (pt.power, pt.toughness) != (power, toughness)
        {
            lines.push(format!(
                "{power}/{toughness} \u{2192} {}/{}",
                pt.power, pt.toughness
            ));
        }
        for counter in object.counters.iter().filter(|counter| counter.count != 0) {
            let kind = format!("{:?}", counter.kind);
            let kind = match kind.as_str() {
                "PlusOne" => "+1/+1",
                "MinusOne" => "\u{2212}1/\u{2212}1",
                other => other,
            };
            lines.push(format!("{kind}: {}", counter.count));
        }
        if object.damage > 0 {
            lines.push(format!("Damage: {}", object.damage));
        }
        if object.summoning_sick {
            lines.push("Summoning sick".to_owned());
        }
    }
    if object.zone == ZoneName::Stack
        && let Some((x, paid)) = input.costs
    {
        if x > 0 {
            lines.push(format!("X = {x}"));
        }
        if !paid.is_empty() {
            let mut words = String::from("Cast with ");
            for (at, cost) in paid.iter().enumerate() {
                if at > 0 {
                    words.push_str(", ");
                }
                let _ = write!(words, "{}", paid_word(*cost));
            }
            lines.push(words);
        }
    }
    lines
}

/// The name of an alternative or additional cost, as its keyword is written.
fn paid_word(cost: PaidCost) -> &'static str {
    match cost {
        PaidCost::Kicker => "kicker",
        PaidCost::Offspring => "offspring",
        PaidCost::Impending => "impending",
        PaidCost::Overload => "overload",
        PaidCost::Evoke => "evoke",
        PaidCost::Flashback => "flashback",
    }
}

/// An inspector slot described relative to its card.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placed {
    offset: Vec2,
    width: f32,
}

/// A place on the web's stage: its left edge, top and width in stage pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Side {
    left: f32,
    top: f32,
    width: f32,
}

/// `inspectorPlace`, on a stage as tall as `data.stage_height` and as wide as `window`'s shape
/// makes it, turned into logical points of `window`.
fn stage_place(
    area: Rect,
    (data, card): (Inspector, Card),
    (card_x, card_origin): (f32, Vec2),
    taken: bool,
    targets: &[Rect],
) -> Placed {
    let per_point = data.stage_height / area.height;
    let stage_width = area.width * per_point;
    let x = card_x - area.x;
    // The web's `fromMiddle` and `fromRight`: a place drawn for the base stage, held to the
    // middle or to the right edge of a wider or narrower one.
    let from_middle = |at: f32| at + (stage_width - data.base_width) / 2.0;
    let from_right = |at: f32| at + stage_width - data.base_width;
    let left = Side {
        left: data.edge,
        top: data.top,
        width: data.width,
    };
    let right = if taken {
        // Right of the life orbs, left of the stack, below the opponent's orb and above yours.
        let start = from_middle(data.orbs_right);
        Side {
            left: start,
            top: data.between_top,
            width: from_right(data.stack_left) - data.stack_gap - start,
        }
    } else {
        Side {
            left: stage_width - data.width - data.edge,
            top: data.top,
            width: data.width,
        }
    };
    let sides = if x * per_point >= stage_width / 2.0 {
        [left, right]
    } else {
        [right, left]
    };
    let targets: Vec<Rect> = targets
        .iter()
        .map(|target| Rect {
            x: (target.x - area.x) * per_point,
            y: (target.y - area.y) * per_point,
            width: target.width * per_point,
            height: target.height * per_point,
        })
        .collect();
    let mut chosen = None;
    'sides: for side in sides {
        let mut width = side.width;
        while width >= data.least_width {
            // The left side keeps its left edge; the others their right one, by the stack or
            // the stage's edge.
            let place = Side {
                left: if side == left {
                    side.left
                } else {
                    side.left + side.width - width
                },
                width,
                ..side
            };
            if covered(place, &targets, (data, card)) <= 0.0 {
                chosen = Some(place);
                break 'sides;
            }
            width -= data.narrow_step;
        }
    }
    let place = chosen.unwrap_or_else(|| {
        if covered(sides[1], &targets, (data, card)) < covered(sides[0], &targets, (data, card)) {
            sides[1]
        } else {
            sides[0]
        }
    });
    Placed {
        offset: Vec2::new(place.left, place.top) / per_point
            - (card_origin - Vec2::new(area.x, area.y)),
        width: place.width / per_point,
    }
}

/// How much of `targets` the inspector at `place` covers, in square stage pixels: the web's
/// `inspectorBox`, for a card with no lines under its face, against each target's box.
fn covered(place: Side, targets: &[Rect], (data, card): (Inspector, Card)) -> f32 {
    let inner = place.width - 2.0 * data.pad;
    let right = place.left + place.width;
    let bottom = place.top + inner * card.height / card.width + 2.0 * data.pad + data.artist;
    targets
        .iter()
        .map(|target| {
            let width = right.min(target.x + target.width) - place.left.max(target.x);
            let height = bottom.min(target.y + target.height) - place.top.max(target.y);
            if width > 0.0 && height > 0.0 {
                width * height
            } else {
                0.0
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{Placed, stage_place as place_in};
    use crate::test_profiles::SHIPPED;
    use glam::Vec2;
    use kit_base::data::Card;
    use kit_base::data::profiles::Profiles;
    use kit_ui::input::{Rect, Size};

    fn window(width: f32, height: f32) -> Rect {
        Rect {
            width,
            height,
            ..Rect::default()
        }
    }

    fn stage_place(area: Rect, x: f32, taken: bool, targets: &[Rect]) -> Placed {
        let card = include_str!("../../../../../../data/client/card.json");
        let card = Card::parse(card).expect("repository card");
        let profiles = Profiles::parse(&SHIPPED, card).expect("repository profiles");
        let data = profiles.for_size(Size {
            width: area.width,
            height: area.height,
        });
        place_in(
            area,
            (data.table.inspector.expect("inspector metrics"), card),
            (x, Vec2::ZERO),
            taken,
            targets,
        )
    }

    #[test]
    fn a_card_on_the_right_puts_it_on_the_left_and_the_left_on_the_right() {
        let window = window(1600.0, 900.0);
        let on_left = stage_place(window, 1200.0, false, &[]);
        assert_eq!(
            on_left,
            Placed {
                offset: Vec2::new(24.0, 118.0),
                width: 330.0
            }
        );
        let on_right = stage_place(window, 300.0, false, &[]);
        assert_eq!(on_right.offset, Vec2::new(1600.0 - 330.0 - 24.0, 118.0));
    }

    #[test]
    fn a_taken_right_puts_it_between_the_orbs_and_the_stack() {
        let placed = stage_place(window(1600.0, 900.0), 300.0, true, &[]);
        assert_eq!(placed.offset, Vec2::new(940.0, 172.0));
        assert!((placed.width - (1284.0 - 54.0 - 940.0)).abs() < 1e-3);
    }

    #[test]
    fn a_target_under_one_side_sends_it_to_the_other() {
        let target = Rect {
            x: 1300.0,
            y: 200.0,
            width: 100.0,
            height: 140.0,
        };
        let placed = stage_place(window(1600.0, 900.0), 300.0, false, &[target]);
        assert!((placed.offset.x - 24.0).abs() < 1e-3, "{placed:?}");
    }

    #[test]
    fn a_window_twice_the_size_doubles_every_number() {
        let small = stage_place(window(1600.0, 900.0), 300.0, true, &[]);
        let large = stage_place(window(3200.0, 1800.0), 600.0, true, &[]);
        assert!((large.offset - small.offset * 2.0).length() < 1e-3);
        assert!((large.width - small.width * 2.0).abs() < 1e-3);
    }
}
