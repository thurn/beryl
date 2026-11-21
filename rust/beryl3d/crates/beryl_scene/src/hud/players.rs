//! Each player's life orb, beside their side of the table: the web interface's `LifeOrb`,
//! `Thinking` and `Incoming` (`ui/src/components/Player.tsx`, `ui/src/game/Life.tsx`) as the
//! web's `Hud` places them (`ui/src/stage/Chrome.tsx`).
//!
//! - The orb is the web's art (`art::orb`): a dark cushion in a bevelled grey bezel, a gold
//!   ring under it toward its player's edge, and the active player's flare. Its art's middle
//!   is the point where the table's layout puts the player's life total (`Layout::players`),
//!   which on a phone follows the table as it raises the viewer's total over a hand of several
//!   rows.
//! - The total itself is not part of the orb. It is the float `floats::LIFE`, drawn over the
//!   orb's middle (`hud::counts`), because a total changes as its `count` step begins, in the
//!   middle of a script, and a float can show a total laid out ahead without a frame laying
//!   anything out. The screen is rebuilt only when an update arrives, which would show the
//!   change late. The mana pool is a float for the same reason.
//! - While the opponent is deciding, a gold ring stands under their orb: a float of its own,
//!   hidden while the step banner is up (`hud::banner`).
//! - From the declaration of attackers until combat damage is dealt, the combat damage a
//!   player stands to take is a badge on the rim of their orb toward the table, solid red when
//!   it would be lethal: the web's `.life-incoming` (`ui/src/screens/styles/table.css`, and
//!   `.phone-orb .life-incoming` in `ui/src/mobile/phone.css` on a phone).

use beryl_protocol::protocol::{Combat, PlayerView, Side, Step};
use beryl_protocol::script::Subject;
use glam::Vec2;

use crate::art::orb;
use crate::data::space::hud::incoming as badge;
use crate::element::{Anchor, Element, Position, Shadow, Shown, TextAlign, holder, text};
use crate::hud::HudIn;
use crate::input::Rect;
use crate::table::phone::strip;
use crate::table::{DESKTOP, Window};
use crate::text::TypeStyle;
use crate::text::fonts::Font;

use super::counts::{index, points};

/// What the game screen shows for the players: both orbs, once a game has a view.
#[must_use]
pub fn players(input: &HudIn) -> Option<Element> {
    let view = input.view?;
    Some(
        holder()
            .name("players")
            .children([Side::You, Side::Opponent].map(|side| orb(view, side, input.window))),
    )
}

/// The middle of `side`'s orb in a window, in logical points: where the table's layout puts
/// their life total, which on the phone layout is the middle of their strip
/// (`table::phone::strip`) until the viewer's hand reaches into it. The table then raises the
/// viewer's total over the hand (`table::phone::your_life`), and their orb follows it.
#[must_use]
pub fn orb_centre(side: Side, window: Window) -> Vec2 {
    if window.phone() {
        return Vec2::new(window.size.width / 2.0, strip(window, side).middle());
    }
    points(DESKTOP.players[index(side)].life, window.size)
}

/// The name of `side`'s orb in the tree, by which the "marks on their things" check finds the
/// life total a mark on that player rings.
#[must_use]
pub fn orb_name(side: Side) -> &'static str {
    match side {
        Side::You => "your orb",
        Side::Opponent => "opponent's orb",
    }
}

/// The box on the window of the core of `side`'s orb, with its bezel, when the orb's art fills
/// `art`: the life total a mark on that player rings. The art's scale is read from the box, so
/// it holds on either layout.
#[must_use]
pub fn core_box(side: Side, art: Rect) -> Rect {
    let units = art.width / orb::ART[2];
    let half = (Vec2::new(orb::CORE_RX, orb::CORE_RY) + orb::BEZEL / 2.0) * units;
    let centre = Vec2::new(
        art.x + art.width / 2.0,
        art.y + art.height / 2.0 + orb::face_offset(side) * units,
    );
    Rect {
        x: centre.x - half.x,
        y: centre.y - half.y,
        width: 2.0 * half.x,
        height: 2.0 * half.y,
    }
}

/// `side`'s orb as `view` shows it: one element the size of the web's orb art, showing the art
/// (`Shown::Layers`).
fn orb(view: &PlayerView, side: Side, window: Window) -> Element {
    let active = view.turn.active == side;
    let life = match side {
        Side::You => view.players.you.life,
        Side::Opponent => view.players.opponent.life,
    };
    // The phone's strips are lower than the desktop's orb, so it draws the orb smaller, as the
    // web's `.phone-orb .life-orb` scales it.
    let scale = u8::from(window.phone());
    let units = orb::SCALES[usize::from(scale)];
    let [_, _, width, height] = orb::ART;
    let mut element = Element::new(crate::frame::Role::Region)
        .name(orb_name(side))
        .width(width * units)
        .height(height * units)
        .selected(active)
        .passes(true)
        .children(
            incoming(view, side)
                .map(|damage| incoming_text(damage, life, side, width * units, window.phone())),
        );
    element.shows = Some(Shown::Layers(orb::artwork(side, active, scale)));
    // On a phone the table raises the viewer's total over a hand of several rows
    // (`table::phone::your_life`), so the orb follows the player's anchor on every frame.
    let middle = Vec2::new(0.5, 0.5);
    element.anchor = Some(if window.phone() {
        Anchor::Object(Subject::Player { player: side }, middle)
    } else {
        Anchor::Point(orb_centre(side, window), middle)
    });
    element
}

/// The middle of `side`'s core in a window, in logical points: where the life total and the
/// step stops centre on the orb, a few points off the middle of its art (`orb::face_offset`).
#[must_use]
pub fn face_centre(side: Side, window: Window) -> Vec2 {
    let units = orb::SCALES[usize::from(window.phone())];
    orb_centre(side, window) + Vec2::new(0.0, orb::face_offset(side) * units)
}

/// The incoming badge's colors, from `.life-incoming` in `ui/src/screens/styles/table.css`:
/// a dark pill with a faint red ring and pale red numerals.
const INCOMING_FILL: [u8; 4] = [24, 21, 19, 217];
const INCOMING_RING: [u8; 4] = [255, 120, 90, 140];
const INCOMING_INK: [u8; 4] = [255, 154, 122, 255];
/// A lethal badge's, from `.life-incoming-lethal`: solid red with a pale ring, a red glow
/// around it and white numerals.
const LETHAL_FILL: [u8; 4] = [179, 38, 30, 255];
const LETHAL_RING: [u8; 4] = [255, 180, 168, 255];
const LETHAL_GLOW: [u8; 4] = [230, 40, 30, 191];
const LETHAL_INK: [u8; 4] = [255, 255, 255, 255];

/// The combat damage `damage` that `side`, at `life`, stands to take, as the web's `Incoming`
/// writes it ("−3", or "0") on a badge on the rim of their orb toward the table: the upper rim
/// for the viewer and the lower for the opponent, as `.life-incoming` places it, a fixed
/// distance right of the middle of the orb's art, which is `orb_width` wide. A phone puts it on
/// the upper rim for both players, as `.phone-orb .life-incoming` does.
fn incoming_text(damage: i32, life: i32, side: Side, orb_width: f32, phone: bool) -> Element {
    let lethal = damage > 0 && damage >= life;
    let words = if damage > 0 {
        format!("\u{2212}{damage}")
    } else {
        "0".to_owned()
    };
    let (left, top, min_width, padding_x, padding_y, text_size) = if phone {
        (
            badge::phone::LEFT,
            badge::phone::TOP,
            badge::phone::MIN_WIDTH,
            badge::phone::PADDING_X,
            badge::phone::PADDING_Y,
            badge::phone::TEXT,
        )
    } else {
        let top = match side {
            Side::You => badge::TOP,
            Side::Opponent => badge::OPPONENT_TOP,
        };
        (
            badge::LEFT,
            top,
            badge::MIN_WIDTH,
            badge::PADDING_X,
            badge::PADDING_Y,
            badge::TEXT,
        )
    };
    let (fill, ring, ink) = if lethal {
        (LETHAL_FILL, LETHAL_RING, LETHAL_INK)
    } else {
        (INCOMING_FILL, INCOMING_RING, INCOMING_INK)
    };
    let mut shadows = vec![Shadow::new(0.0, 0.0, 0.0, ring).spread(1.0).inset()];
    if lethal {
        shadows.push(Shadow::new(0.0, 0.0, badge::LETHAL_GLOW_BLUR, LETHAL_GLOW));
    }
    let mut element = text(&words)
        .name(&format!(
            "{damage} combat damage{}",
            if lethal { ", lethal" } else { "" }
        ))
        .style(TypeStyle {
            font: Font::OpenSans,
            weight: 700,
            italic: false,
            size: text_size,
            line_height: badge::LINE,
            letter_spacing: 0.0,
        })
        .text_align(TextAlign::Center)
        .position(Position::Absolute)
        .inset([Some(top), None, None, Some(orb_width / 2.0 + left)])
        .min_width(min_width)
        .background(fill)
        // `border-radius: 999px`: half the badge's height rounds its ends fully.
        .radius(text_size * badge::LINE / 2.0 + padding_y)
        .box_shadow(shadows)
        .color(ink);
    element.style.padding = [padding_y, padding_x, padding_y, padding_x];
    element
}

/// The combat damage `side` stands to take from the creatures attacking them that `view` shows
/// unblocked, or `None` outside the declare attackers and declare blockers steps or if none
/// attacks them, as the web's `incoming` (`ui/src/game/combat.ts`) counts it.
///
/// [CR 510.1b]: "An unblocked creature assigns its combat damage to the player, planeswalker,
/// or battle it's attacking." A blocked creature deals the player none, and each unblocked one
/// deals its power once. An attacker whose attack is still being declared
/// (`pending_attackers`) is not counted yet.
///
/// [CR 510.1b]: https://yawgatog.com/resources/magic-rules/#R5101b
#[must_use]
pub fn incoming(view: &PlayerView, side: Side) -> Option<i32> {
    if !matches!(
        view.turn.step,
        Step::DeclareAttackers | Step::DeclareBlockers
    ) {
        return None;
    }
    let pending = view.pending_attackers.as_deref().unwrap_or_default();
    let mut total = None;
    for object in &view.objects {
        let Some(Combat::Attacking {
            planeswalker: None,
            target_departed,
            blocked,
        }) = &object.combat
        else {
            continue;
        };
        if *target_departed == Some(true)
            || pending.contains(&object.slot)
            || object.controller == side
        {
            continue;
        }
        let power = if *blocked {
            0
        } else {
            object.pt.as_ref().map_or(0, |pt| pt.power.max(0))
        };
        total = Some(total.unwrap_or(0) + power);
    }
    total
}
