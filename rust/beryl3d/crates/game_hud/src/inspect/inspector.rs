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
//! Its numbers are the web's, on its stage 900 pixels tall (`layout.desktop.inspector` in
//! `data/beryl3d/space-constants.json`). The stage here is as tall as the window and as wide as the
//! window's shape makes it, as the tray's is (`table::tray`), and a place on it is turned into
//! logical points of the window.

use std::fmt::Write as _;

use beryl_protocol::protocol::{ObjectId, ObjectView, PaidCost, RefusalView, StackItem, ZoneName};
use glam::Vec2;

use crate::hud::HudIn;
use game_table::faces::FaceList;
use game_table::table::{CARD_HEIGHT, CARD_WIDTH, Window};
use kit_base::data::space::layout::desktop::inspector as data;
use kit_base::tokens::{color, size, space, type_style};
use kit_frame::frame::Role;
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
    /// The boxes of the cards the open prompt marks, which the player may be about to pick and
    /// which the inspector keeps clear of where it can (the web's `keptClear`).
    pub targets: Vec<Rect>,
    /// Why the open decision refused the click on it, while the pointer has stayed on it
    /// since (`Client::refuse_click`).
    pub refusal: Option<RefusalView>,
}

/// The inspector's corner radius, `.inspect`'s `border-radius`.
const RADIUS: f32 = size::INSPECTOR_RADIUS;

/// What the game screen shows for it: the full-size face of the card the pointer is over, at
/// the place `place` gives, with its state beneath it. Nothing when no card is inspected, on the
/// phone layout, which reads a card another way, or when the view has no such card.
#[must_use]
pub fn inspector(input: &HudIn) -> Option<Element> {
    let inspected = input.inspected?;
    let place = place(input)?;
    let object = input
        .view?
        .objects
        .iter()
        .find(|object| object.slot == inspected.slot)?;
    let name = object.name.as_deref()?;
    let per_point = data::STAGE_HEIGHT / input.window.size.height;
    let pad = data::PAD / per_point;
    let inner = place.width - 2.0 * pad;
    let face = Element {
        shows: Some(Shown::Face(inspected.face, FaceSize::Full)),
        ..Element::new(Role::Region)
    }
    .name("face")
    .width(inner)
    .height(inner * CARD_HEIGHT / CARD_WIDTH);
    let lines = state(object, input).into_iter().map(|line| {
        text(&line)
            .style(type_style::BODY)
            .color(color::TEXT_DIM)
            .max_width(inner)
    });
    Some(
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
                    .map(|reason| refusal(reason, input, inner)),
            ),
    )
}

/// The line under the state that says why a click on the card was refused, as the web's
/// `InspectRefusal` draws it (`ui/src/game/Piles.tsx`), in the words `refusal_words` gives.
/// The web's glyph before the words, from the Mana font, is not drawn: the flat layer has no
/// such icons.
fn refusal(reason: &RefusalView, input: &HudIn, inner: f32) -> Element {
    let by = match reason {
        RefusalView::Timing { by: Some(by) } => (input.view)
            .and_then(|view| view.objects.iter().find(|object| object.slot == *by))
            .and_then(|object| object.name.as_deref()),
        _ => None,
    };
    let words = refusal_words(reason, by);
    let mut line = row()
        .name(&format!("Can't play: {words}"))
        .justify_content(kit_ui::element::Justify::Center)
        .max_width(inner)
        .background(color::REFUSAL)
        .radius(size::REFUSAL_RADIUS)
        .child(
            text(&words)
                .style(type_style::REFUSAL)
                .color(color::STAT_DOWN),
        );
    let (across, down) = (size::REFUSAL_PADDING_X, size::REFUSAL_PADDING_Y);
    line.style.padding = [down, across, down, across];
    line
}

/// Why a card can't be played, in the web's words (`REFUSAL_WORDS` in `ui/src/game/Piles.tsx`):
/// a timing refusal names the permanent `by` that holds its controller to sorcery speed, by the
/// part of its name before a comma.
#[must_use]
pub fn refusal_words(reason: &RefusalView, by: Option<&str>) -> String {
    match (reason, by) {
        (RefusalView::Timing { .. }, Some(by)) => {
            format!("Sorcery speed: {}", by.split(',').next().unwrap_or(by))
        }
        (RefusalView::Timing { .. }, None) => "Sorcery speed".to_owned(),
        (RefusalView::LandPlayed, _) => "Land played".to_owned(),
        (RefusalView::Mana, _) => "No mana".to_owned(),
        (RefusalView::Cost, _) => "Can't pay".to_owned(),
        (RefusalView::Target, _) => "No target".to_owned(),
    }
}

/// The lines under the face that say what the face cannot, in the web's order: its note, the
/// value it chose, the card a copy really is, that it is phased out, its owner when another
/// player controls it, its power and toughness against the printed ones, its counters, its
/// damage and summoning sickness, and on the stack, its X and the costs it was cast with.
fn state(object: &ObjectView, input: &HudIn) -> Vec<String> {
    let mut lines = Vec::new();
    lines.extend(object.note.clone());
    lines.extend(
        object
            .chosen
            .as_ref()
            .map(|chosen| format!("Chosen: {chosen}")),
    );
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
        let printed = object.name.as_deref().and_then(|name| printed(input, name));
        if let (Some(pt), Some([power, toughness])) = (&object.pt, printed)
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
        && let Some((x, paid)) = input.view.and_then(|view| {
            view.stack.iter().find_map(|item| match item {
                StackItem::Spell { slot, x, paid, .. } if *slot == object.slot => Some((*x, paid)),
                _ => None,
            })
        })
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

/// The power and toughness printed on the face of the card named `name`, when both are numbers.
fn printed(input: &HudIn, name: &str) -> Option<[i32; 2]> {
    let list: &FaceList = input.faces?;
    list.get(list.find(name)?)?.printed
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

/// Where the inspector stands, in logical points of the window: its top left corner and its
/// width. As the web's `inspectorPlace` puts it: beside the table on the side away from the
/// card, never over a life orb, which lies between the two sides. While the stack or a prompt
/// holds the table's right (the stack is not empty or the prompt marks cards), it stands smaller
/// in the column between the life orbs and the stack instead. It never covers a card the prompt
/// marks: where its side would, it takes the other, and where both would, it narrows toward a
/// side's outer edge, to no less than `least_width`, until it clears them. Failing that, it
/// takes the side that covers the least of them.
///
/// `None` with no card inspected and on the phone layout.
#[must_use]
pub fn place(input: &HudIn) -> Option<Placed> {
    let inspected = input.inspected?;
    if input.window.phone() {
        return None;
    }
    let taken =
        input.view.is_some_and(|view| !view.stack.is_empty()) || !inspected.targets.is_empty();
    Some(stage_place(
        input.window,
        inspected.x,
        taken,
        &inspected.targets,
    ))
}

/// Where the inspector stands and how wide it is, in logical points of the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub corner: Vec2,
    pub width: f32,
}

/// A place on the web's stage: its left edge, top and width in stage pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Side {
    left: f32,
    top: f32,
    width: f32,
}

/// `inspectorPlace`, on a stage as tall as `data::STAGE_HEIGHT` and as wide as `window`'s shape
/// makes it, turned into logical points of `window`.
fn stage_place(window: Window, x: f32, taken: bool, targets: &[Rect]) -> Placed {
    let per_point = data::STAGE_HEIGHT / window.size.height;
    let stage_width = window.size.width * per_point;
    // The web's `fromMiddle` and `fromRight`: a place drawn for the base stage, held to the
    // middle or to the right edge of a wider or narrower one.
    let from_middle = |at: f32| at + (stage_width - data::BASE_WIDTH) / 2.0;
    let from_right = |at: f32| at + stage_width - data::BASE_WIDTH;
    let left = Side {
        left: data::EDGE,
        top: data::TOP,
        width: data::WIDTH,
    };
    let right = if taken {
        // Right of the life orbs, left of the stack, below the opponent's orb and above yours.
        let start = from_middle(data::ORBS_RIGHT);
        Side {
            left: start,
            top: data::BETWEEN_TOP,
            width: from_right(data::STACK_LEFT) - data::STACK_GAP - start,
        }
    } else {
        Side {
            left: stage_width - data::WIDTH - data::EDGE,
            top: data::TOP,
            width: data::WIDTH,
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
            x: target.x * per_point,
            y: target.y * per_point,
            width: target.width * per_point,
            height: target.height * per_point,
        })
        .collect();
    let mut chosen = None;
    'sides: for side in sides {
        let mut width = side.width;
        while width >= data::LEAST_WIDTH {
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
            if covered(place, &targets) <= 0.0 {
                chosen = Some(place);
                break 'sides;
            }
            width -= data::NARROW_STEP;
        }
    }
    let place = chosen.unwrap_or_else(|| {
        if covered(sides[1], &targets) < covered(sides[0], &targets) {
            sides[1]
        } else {
            sides[0]
        }
    });
    Placed {
        corner: Vec2::new(place.left, place.top) / per_point,
        width: place.width / per_point,
    }
}

/// How much of `targets` the inspector at `place` covers, in square stage pixels: the web's
/// `inspectorBox`, for a card with no lines under its face, against each target's box.
fn covered(place: Side, targets: &[Rect]) -> f32 {
    let inner = place.width - 2.0 * data::PAD;
    let right = place.left + place.width;
    let bottom = place.top + inner * CARD_HEIGHT / CARD_WIDTH + 2.0 * data::PAD + data::ARTIST;
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
    use super::{Placed, stage_place};
    use game_table::table::Window;
    use glam::Vec2;
    use kit_ui::input::{Insets, Rect, Size};

    fn window(width: f32, height: f32) -> Window {
        Window {
            size: Size { width, height },
            scale: 1.0,
            safe_area: Insets::default(),
        }
    }

    #[test]
    fn a_card_on_the_right_puts_it_on_the_left_and_the_left_on_the_right() {
        let window = window(1600.0, 900.0);
        let on_left = stage_place(window, 1200.0, false, &[]);
        assert_eq!(
            on_left,
            Placed {
                corner: Vec2::new(24.0, 118.0),
                width: 330.0
            }
        );
        let on_right = stage_place(window, 300.0, false, &[]);
        assert_eq!(on_right.corner, Vec2::new(1600.0 - 330.0 - 24.0, 118.0));
    }

    #[test]
    fn a_taken_right_puts_it_between_the_orbs_and_the_stack() {
        let placed = stage_place(window(1600.0, 900.0), 300.0, true, &[]);
        assert_eq!(placed.corner, Vec2::new(940.0, 172.0));
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
        assert!((placed.corner.x - 24.0).abs() < 1e-3, "{placed:?}");
    }

    #[test]
    fn a_window_twice_the_size_doubles_every_number() {
        let small = stage_place(window(1600.0, 900.0), 300.0, true, &[]);
        let large = stage_place(window(3200.0, 1800.0), 600.0, true, &[]);
        assert!((large.corner - small.corner * 2.0).length() < 1e-3);
        assert!((large.width - small.width * 2.0).abs() < 1e-3);
    }
}
