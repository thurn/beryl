//! Each player's life orb, beside their side of the table: the web interface's `LifeOrb`,
//! `Thinking` and `Incoming` (`ui/src/components/Player.tsx`, `ui/src/game/Life.tsx`) as the
//! web's `Hud` places them (`ui/src/stage/Chrome.tsx`).
//!
//! - The orb is a dark core with a grey bezel, centred on the point where the table's layout
//!   puts the player's life total (`Layout::players`). The active player's bezel is gold.
//! - The total itself is not part of the orb. It is the float `floats::LIFE`, drawn over the
//!   orb's middle (`hud::counts`), because a total changes as its `count` step begins, in the
//!   middle of a script, and a float can show a total laid out ahead without a frame laying
//!   anything out. The screen is rebuilt only when an update arrives, which would show the
//!   change late. The mana pool is a float for the same reason.
//! - While the opponent is deciding, a gold ring stands under their orb.
//! - From the declaration of attackers until combat damage is dealt, the combat damage a
//!   player stands to take stands beside their orb, red when it would be lethal.

use beryl_protocol::protocol::{Combat, PlayerView, Side, Status, Step};
use glam::Vec2;

use crate::data::space::hud::{INCOMING_GAP, THINKING_GAP};
use crate::element::{Align, Anchor, Element, Justify, Position, holder, text};
use crate::screens::ScreenIn;
use crate::table::{DESKTOP, Window};
use crate::tokens::{color, size, type_style};

use super::counts::{index, points};

/// What the game screen shows for the players: both orbs, once a game has a view.
#[must_use]
pub fn players(input: &ScreenIn) -> Option<Element> {
    let view = input.view?;
    Some(
        holder()
            .name("players")
            .children([Side::You, Side::Opponent].map(|side| orb(view, side, input.window))),
    )
}

/// The middle of `side`'s orb in a window, in logical points: where the table's layout puts
/// their life total.
#[must_use]
pub fn orb_centre(side: Side, window: Window) -> Vec2 {
    points(DESKTOP.players[index(side)].life, window.size)
}

/// `side`'s orb as `view` shows it.
fn orb(view: &PlayerView, side: Side, window: Window) -> Element {
    let active = view.turn.active == side;
    let rim = if active {
        color::ORB_ACTIVE
    } else {
        color::ORB_RIM
    };
    let thinking = side == Side::Opponent && matches!(view.status, Status::Thinking);
    let life = match side {
        Side::You => view.players.you.life,
        Side::Opponent => view.players.opponent.life,
    };
    let mut element = Element::new(crate::frame::Role::Region)
        .name(match side {
            Side::You => "your orb",
            Side::Opponent => "opponent's orb",
        })
        .width(size::ORB_WIDTH + 2.0 * size::ORB_BEZEL)
        .height(size::ORB_HEIGHT + 2.0 * size::ORB_BEZEL)
        .background(color::ORB_CORE)
        .border((rim, size::ORB_BEZEL))
        .radius(size::ORB_HEIGHT / 2.0 + size::ORB_BEZEL)
        .selected(active)
        .passes(true)
        .children(thinking.then(thinking_ring))
        .children(incoming(view, side).map(|damage| incoming_text(damage, life)));
    element.anchor = Some(Anchor::Point(orb_centre(side, window), Vec2::new(0.5, 0.5)));
    element
}

/// The ring under the opponent's orb while they decide, the web's `Thinking` spinner held
/// still: the flat layer draws no arcs, so the whole ring is gold.
fn thinking_ring() -> Element {
    let side = size::THINKING;
    let width = size::ORB_WIDTH + 2.0 * size::ORB_BEZEL;
    let height = size::ORB_HEIGHT + 2.0 * size::ORB_BEZEL;
    Element::new(crate::frame::Role::Region)
        .name("Opponent thinking")
        .position(Position::Absolute)
        .inset([
            Some(height + THINKING_GAP),
            None,
            None,
            Some((width - side) / 2.0),
        ])
        .width(side)
        .height(side)
        .border((color::THINKING, size::THINKING_TRACK))
        .radius(side / 2.0)
}

/// The combat damage `damage` a player at `life` stands to take, beside their orb as the web's
/// `Incoming` writes it: "−3", or "0", red when it is lethal.
fn incoming_text(damage: i32, life: i32) -> Element {
    let lethal = damage > 0 && damage >= life;
    let words = if damage > 0 {
        format!("\u{2212}{damage}")
    } else {
        "0".to_owned()
    };
    let mut element = text(&words)
        .name(&format!(
            "{damage} combat damage{}",
            if lethal { ", lethal" } else { "" }
        ))
        .style(type_style::TITLE)
        .position(Position::Absolute)
        .inset([
            Some(0.0),
            None,
            Some(0.0),
            Some(size::ORB_WIDTH + 2.0 * size::ORB_BEZEL + INCOMING_GAP),
        ])
        .align_items(Align::Center)
        .justify_content(Justify::Center);
    if lethal {
        element = element.color(color::DAMAGE);
    }
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
