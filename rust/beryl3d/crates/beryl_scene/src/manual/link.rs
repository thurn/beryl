//! The other seat's connection to a hosted or joined table (docs/manual-multiplayer.md), as the
//! web's `LinkBanner` and `useTableLink` (`ui/src/table/LinkBanner.tsx` and
//! `ui/src/table/link.ts`) show it. While the connection is down (`LinkState::Reconnecting`)
//! or the other player has left (`LinkState::Left`), every input to the table is frozen and the
//! step banner names the other player under the window's top. A table the other player left
//! also offers Lobby, which leaves it. The guest redials by itself, and the table's next view,
//! which carries the restored link, unfreezes the input.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::LinkState;
use glam::Vec2;

use super::TablePress;
use crate::data::space::flat::table::link::{PHONE_TOP, TOP, TOP_SHARE, WIDTH};
use crate::element::{Align, Anchor, Element, column, text};
use crate::hud::notices::step_banner;
use crate::input::Size;
use crate::tokens::{color, size, space, type_style};
use crate::widgets::button;

/// Whether `link` freezes the table's input.
#[must_use]
pub const fn blocks(link: LinkState) -> bool {
    matches!(link, LinkState::Reconnecting | LinkState::Left)
}

/// Whether the table in `view` is frozen because its connection is down or the other player
/// left.
#[must_use]
pub fn frozen(view: &PlayerView) -> bool {
    view.table.as_ref().is_some_and(|table| blocks(table.link))
}

/// The banner the table shows while its input is frozen, with the failure the table reported
/// (`error`, as the web's table toast shows it) and Lobby under it once the other player has
/// left, standing `top` points below the top of a window of `window`'s size; none while the
/// connection is up.
#[must_use]
pub fn banner(view: &PlayerView, window: Size, top: f32, error: Option<&str>) -> Option<Element> {
    let table = view.table.as_ref()?;
    let peer = table.peer_name.as_deref().unwrap_or("Opponent");
    let title = match table.link {
        LinkState::Left => format!("{peer} left the table"),
        LinkState::Reconnecting => format!("{peer} is reconnecting"),
        LinkState::Alone | LinkState::Connected { .. } => return None,
    };
    let width = WIDTH.min(window.width - 2.0 * space::M);
    let notice = step_banner(&title, None)
        .name("link")
        .width(width)
        .background(color::PANEL)
        .border((color::METAL, size::BORDER));
    let lobby =
        (table.link == LinkState::Left).then(|| button("Lobby").on_press(TablePress::Leave));
    let mut shown = column()
        .name("connection")
        .align_items(Align::Center)
        .gap(space::S)
        .child(notice)
        .children(error.map(|error| {
            text(error)
                .style(type_style::SCREEN_BODY)
                .color(color::EMBER)
        }))
        .children(lobby);
    shown.anchor = Some(Anchor::Point(
        Vec2::new(window.width / 2.0, top),
        Vec2::new(0.5, 0.0),
    ));
    Some(shown)
}

/// How far below the window's top the banner stands: under the safe area on the phone, and at
/// `.table-link`'s padding (`flat.table.link` in `data/beryl3d/space.json`), at most a share of
/// the window's height, on the desktop. The banner is at most `flat.table.link.width` wide.
#[must_use]
pub fn top(window: Size, phone: bool, safe_top: f32) -> f32 {
    if phone {
        safe_top + PHONE_TOP
    } else {
        TOP.min(TOP_SHARE * window.height)
    }
}
