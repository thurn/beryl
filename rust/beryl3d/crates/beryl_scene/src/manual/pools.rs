//! Each player's mana and player counters on the manual table (docs/ui-conventions.md, "Manual
//! table: a click acts" and "Manual counters and untap flags"), as the web's `ManaPool` stands
//! them beside a life orb when it is given `adjust` (`ui/src/components/Player.tsx`): all six
//! mana symbols, each with its amount once it has any, then each named counter the player has,
//! as its name and count. A click on one adds one (`TablePress::Counter`) and a right click
//! removes one, as the web's `registerPlayers` (`ui/src/table/players.ts`) sends them. The
//! opponent's row takes clicks only at a table of two seats, as the web's `Chrome` gives it
//! `adjust`; at a solo table it shows only the mana and counters they have, as a game does. They
//! stand where a game shows the player's unspent mana, which the table does not show
//! separately (`hud::counts`).
//!
//! On the phone the web opens these in a sheet from the mana total beside the life orb; Beryl
//! 3D does not show them on the phone layout yet.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{ManaPool, PlayerState, Side};

use super::TablePress;
use crate::element::{Align, Anchor, Element, Press, holder, row, text};
use crate::hud::counts::{index, points};
use crate::input::Size;
use crate::table::DESKTOP;
use crate::tokens::{color, size, space};

/// The mana symbols, in the order the web's `POOL_SYMBOLS` shows them, with each one's amount.
fn mana(pool: ManaPool) -> [(&'static str, u32); 6] {
    [
        ("W", pool.white),
        ("U", pool.blue),
        ("B", pool.black),
        ("R", pool.red),
        ("G", pool.green),
        ("C", pool.colorless),
    ]
}

/// Both players' rows, in a window of `size`, or none on the phone layout.
#[must_use]
pub fn pools(view: &PlayerView, window: Size, phone: bool) -> Option<Element> {
    if phone {
        return None;
    }
    let seats = view.table.as_ref().map_or(1, |table| table.seats);
    Some(
        holder()
            .name("pools")
            .children([Side::You, Side::Opponent].map(|side| {
                let adjust = side == Side::You || seats == 2;
                pool(player(view, side), side, window, adjust)
            })),
    )
}

/// `side`'s state in `view`.
fn player(view: &PlayerView, side: Side) -> &PlayerState {
    match side {
        Side::You => &view.players.you,
        Side::Opponent => &view.players.opponent,
    }
}

/// `player`'s row, for `side`, at the point the desktop layout gives their pool: every mana
/// symbol, each taking clicks, if `adjust`, and otherwise only the mana they have, taking none.
/// Only a row that takes clicks numbers its counters as `counter_name` reads them.
fn pool(player: &PlayerState, side: Side, window: Size, adjust: bool) -> Element {
    let whose = match side {
        Side::You => "Your",
        Side::Opponent => "Opponent's",
    };
    let mana = (mana(player.mana).into_iter())
        .filter(|&(_, count)| adjust || count > 0)
        .map(|(symbol, count)| {
            let words = if count > 0 {
                format!("{{{symbol}}}{count}")
            } else {
                format!("{{{symbol}}}")
            };
            (symbol.to_owned(), count, words)
        });
    let named = (player.named_counters.iter())
        .filter(|counter| counter.count != 0)
        .map(|counter| {
            let words = format!("{} {}", counter.name, counter.count);
            (
                counter.name.clone(),
                u32::try_from(counter.count).unwrap_or(0),
                words,
            )
        });
    let mut element = row()
        .name(&format!("{whose} counters"))
        .gap(space::S)
        .align_items(Align::Center)
        .children(
            mana.chain(named)
                .enumerate()
                .map(|(at, (name, count, words))| {
                    let at = u8::try_from(at).unwrap_or(u8::MAX);
                    let button = counter(&format!("{name} counter: {count}"), &words, adjust);
                    if adjust {
                        button.on_press(Press::Table(TablePress::Counter { side, at }))
                    } else {
                        button
                    }
                }),
        );
    element.anchor = Some(Anchor::Point(
        points(DESKTOP.players[index(side)].pool, window),
        glam::Vec2::new(0.0, 0.5),
    ));
    element
}

/// One counter, named `name`, showing `words`: a button if it takes clicks.
fn counter(name: &str, words: &str, button: bool) -> Element {
    let role = if button {
        crate::frame::Role::Button
    } else {
        crate::frame::Role::Region
    };
    Element::new(role)
        .name(name)
        .padding(space::S)
        .radius(size::PANEL_RADIUS)
        .background(color::PANEL)
        .child(text(words).passes(true))
}

/// The name a table action gives the counter at `at` of `side`'s row in `view`: a mana
/// symbol's letter, or a named counter's name.
#[must_use]
pub fn counter_name(view: &PlayerView, side: Side, at: u8) -> Option<String> {
    let player = player(view, side);
    let at = usize::from(at);
    if let Some((symbol, _)) = mana(player.mana).get(at) {
        return Some((*symbol).to_owned());
    }
    (player.named_counters.iter())
        .filter(|counter| counter.count != 0)
        .nth(at - 6)
        .map(|counter| counter.name.clone())
}

/// The id of the command that adds one to the counter at `at` of a row, or removes one unless
/// `add`: mana for the six symbols, a player counter after them.
#[must_use]
pub const fn command(at: u8, add: bool) -> &'static str {
    match (at < 6, add) {
        (true, true) => "player.mana_add",
        (true, false) => "player.mana_remove",
        (false, true) => "player.counter_add",
        (false, false) => "player.counter_remove",
    }
}
