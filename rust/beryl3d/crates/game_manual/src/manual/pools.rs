//! Each player's mana and player counters on the manual table (docs/ui-conventions.md, "Manual
//! table: a click acts" and "Manual counters and untap flags"), as the web's `ManaPool` stands
//! them beside a life orb when it is given `adjust` (`ui/src/components/Player.tsx`): all six
//! mana symbols, each with its amount once it has any, then each named counter the player has,
//! as its name and count. A click on one adds one (`TableMessage::Counter`) and a right click
//! removes one, as the web's `registerPlayers` (`ui/src/table/players.ts`) sends them. The
//! opponent's row takes clicks only at a table of two seats, as the web's `Chrome` gives it
//! `adjust`; at a solo table it shows only the mana and counters they have, as a game does. They
//! stand where a game shows the player's unspent mana, which the table does not show
//! separately (`hud::counts`).
//!
//! On the phone each player's strip shows a Mana button with their total mana instead, between
//! the piles and the life orb, as the web's `ManaButton` (`ui/src/stage/PhoneStageBoard.tsx`)
//! stands it. It opens a sheet (`sheet`, the web's `ManaSheet` in
//! `ui/src/stage/PhoneChrome.tsx`), where each mana symbol and counter is a button that adds one
//! beside a minus button that removes one.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{ManaPool, PlayerState, Side};
use glam::Vec2;

use crate::features::table::Message as TableMessage;
use game_play::hud::counts::{index, points};
use game_table::profiles::Profile;
use game_table::table::{REFERENCE, Window};
use kit_base::data::profiles::ManualControls;
use kit_base::tokens::{color, size, space, type_style};
use kit_ui::element::{Align, Anchor, Display, Element, Justify, Track, column, holder, row, text};
use kit_ui::input::Size;
use kit_ui::widgets::button;

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

/// Both players' rows in `window`, or on the phone layout their Mana buttons.
#[must_use]
pub fn pools(view: &PlayerView, window: Window, profile: Profile<'_>) -> Element {
    if let Some((bands, metrics)) = profile
        .data()
        .manual
        .and_then(|metrics| Some((profile.bands()?, metrics)))
    {
        let strips = [
            (Side::You, bands.your_strip),
            (Side::Opponent, bands.opponent_strip),
        ];
        let buttons = strips.into_iter().filter_map(|(side, strip)| {
            let at = Vec2::new(window.size.width / 2.0 - metrics.mana_right, strip.middle());
            mana_button(
                player(view, side),
                side,
                adjustable(view, side),
                at,
                metrics,
                profile.data().checks.target_points,
            )
        });
        return holder().name("pools").children(buttons);
    }
    holder()
        .name("pools")
        .children([Side::You, Side::Opponent].map(|side| {
            pool(
                player(view, side),
                side,
                window.size,
                adjustable(view, side),
            )
        }))
}

/// Whether the viewer may change `side`'s mana and counters in `view`: their own always, and
/// the opponent's at a table of two seats, as the web's `Chrome` gives a row `adjust`.
fn adjustable(view: &PlayerView, side: Side) -> bool {
    side == Side::You || view.table.as_ref().is_some_and(|table| table.seats == 2)
}

/// `player`'s Mana button on the phone, for `side`, its right edge's middle at `at`: the word
/// and their total mana under it, which opens their sheet. A player whose mana and counters the
/// viewer may not change and who has none shows no button, as the web's `ManaButton` hides.
fn mana_button(
    player: &PlayerState,
    side: Side,
    adjust: bool,
    at: Vec2,
    metrics: ManualControls,
    target: f32,
) -> Option<Element> {
    let total: u32 = mana(player.mana).iter().map(|&(_, count)| count).sum();
    let counters = player
        .named_counters
        .iter()
        .any(|counter| counter.count != 0);
    if !adjust && total == 0 && !counters {
        return None;
    }
    let whose = match side {
        Side::You => "Your",
        Side::Opponent => "Opponent's",
    };
    let mut element = column();
    element.role = kit_frame::frame::Role::Button;
    let mut element = element
        .name(&format!("{whose} mana: {total}"))
        .width(metrics.mana_width)
        .min_height(size::ICON_BUTTON.max(target))
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .radius(size::PANEL_RADIUS)
        .background(color::PANEL)
        .border((color::METAL, size::BORDER))
        .child(text("Mana").passes(true))
        .child(
            text(&total.to_string())
                .style(type_style::BUTTON)
                .passes(true),
        )
        .on_press(TableMessage::ManaSheet(Some(side)));
    element.anchor = Some(Anchor::Point(at, Vec2::new(1.0, 0.5)));
    Some(element)
}

/// The sheet of `side`'s mana and counters on the phone, as the web's `ManaSheet` shows them:
/// its heading and a Done button that closes it,
/// then each mana symbol and counter in three columns, each a button that adds one beside a
/// minus button that removes one if the viewer may change them. None in the desktop layout.
/// The web stands it at the bottom of the window; here it stands below the opponent's strip, as
/// the log sheet does (`hud::phone::log_sheet`), since the life totals are floats drawn over
/// every screen element and would show through a sheet over the viewer's strip.
#[must_use]
pub fn sheet(view: &PlayerView, side: Side, profile: Profile<'_>) -> Option<Element> {
    let rect = profile.slot("manual.mana")?.rect;
    let metrics = profile.data().manual?;
    let adjust = adjustable(view, side);
    let heading = row()
        .align_items(Align::Center)
        .justify_content(Justify::SpaceBetween)
        .child(text("Mana").style(type_style::TITLE))
        .child(
            button("Done")
                .min_width(metrics.mana_done_width)
                .on_press(TableMessage::ManaSheet(None)),
        );
    let mut grid = Element::new(kit_frame::frame::Role::Region)
        .gap(space::S)
        .grid_template_columns(vec![Track::Fraction(1.0); 3]);
    grid.style.display = Display::Grid;
    let grid = grid.children(entries(player(view, side), adjust).enumerate().map(
        |(at, (name, count, words, _))| {
            let at = u8::try_from(at).unwrap_or(u8::MAX);
            let add = counter(&format!("{name} counter: {count}"), &words, adjust)
                .grow(1.0)
                .min_height(size::ICON_BUTTON);
            if !adjust {
                return add;
            }
            let remove = counter(&format!("Remove {name}"), "\u{2212}", true)
                .min_width(size::ICON_BUTTON)
                .min_height(size::ICON_BUTTON)
                .on_press(TableMessage::CounterRemove { side, at });
            row()
                .gap(space::S)
                .child(add.on_press(TableMessage::Counter { side, at }))
                .child(remove)
        },
    ));
    let label = match side {
        Side::You => "Your mana and counters",
        Side::Opponent => "Opponent's mana and counters",
    };
    let mut sheet = column()
        .name(label)
        .width(rect.width)
        .padding(space::M)
        .gap(space::S)
        .background(color::PANEL)
        .border((color::METAL, size::BORDER))
        .radius(size::PANEL_RADIUS)
        .child(heading)
        .child(grid);
    sheet.anchor = Some(Anchor::Point(Vec2::new(rect.x, rect.y), Vec2::ZERO));
    Some(sheet)
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
    let mut element =
        row()
            .name(&format!("{whose} counters"))
            .gap(size::POOL_GAP)
            .align_items(Align::Center)
            .children(entries(player, adjust).enumerate().map(
                |(at, (name, count, words, mana))| {
                    let at = u8::try_from(at).unwrap_or(u8::MAX);
                    let button =
                        counter_cell(&format!("{name} counter: {count}"), &words, mana, adjust);
                    if adjust {
                        button.on_press(TableMessage::Counter { side, at })
                    } else {
                        button
                    }
                },
            ));
    // The point is the row's top left corner, as the web's `MANA_POOLS` places the `.hud` that
    // holds it (`ui/src/game/tableRows.ts`).
    element.anchor = Some(Anchor::Point(
        points(REFERENCE.players[index(side)].pool, window),
        Vec2::ZERO,
    ));
    element
}

/// `player`'s mana symbols and named counters as a row or a sheet shows them, each its name, its
/// count, the words it shows and whether it is a mana symbol: every mana symbol if `adjust`, and otherwise only those with
/// mana, then each named counter that is not zero. Their order is the one `counter_name`
/// numbers them in.
fn entries(
    player: &PlayerState,
    adjust: bool,
) -> impl Iterator<Item = (String, u32, String, bool)> {
    let mana = (mana(player.mana).into_iter())
        .filter(move |&(_, count)| adjust || count > 0)
        .map(|(symbol, count)| {
            let words = if count > 0 {
                format!("{{{symbol}}}{count}")
            } else {
                format!("{{{symbol}}}")
            };
            (symbol.to_owned(), count, words, true)
        });
    let named = (player.named_counters.iter())
        .filter(|counter| counter.count != 0)
        .map(|counter| {
            let words = format!("{} {}", counter.name, counter.count);
            (
                counter.name.clone(),
                u32::try_from(counter.count).unwrap_or(0),
                words,
                false,
            )
        });
    mana.chain(named)
}

/// One counter, named `name`, showing `words`: a button if it takes clicks.
fn counter(name: &str, words: &str, button: bool) -> Element {
    let role = if button {
        kit_frame::frame::Role::Button
    } else {
        kit_frame::frame::Role::Region
    };
    Element::new(role)
        .name(name)
        .padding(space::S)
        .radius(size::PANEL_RADIUS)
        .background(color::PANEL)
        .child(text(words).passes(true))
}

/// One counter of a desktop row, named `name`, showing `words`, a button if it takes clicks, as
/// the web's `.manual-counter-button` draws it: no face or border of its own, two points of
/// padding round what it holds. A mana symbol (`mana`) is set as the web's `.mana-pool` sets
/// it, a disc of `type_style::POOL`'s size with its amount after it in `POOL_COUNT`.
fn counter_cell(name: &str, words: &str, mana: bool, button: bool) -> Element {
    let role = if button {
        kit_frame::frame::Role::Button
    } else {
        kit_frame::frame::Role::Region
    };
    let mut shown = text(words).passes(true);
    if mana {
        shown = shown.style(type_style::POOL);
        let symbol_end = words.find('}').map_or(words.len(), |end| end + 1);
        #[expect(clippy::cast_possible_truncation, reason = "a short text")]
        let range = symbol_end as u32..words.len() as u32;
        if !range.is_empty() {
            shown.spans.push(kit_ui::element::Span {
                range,
                style: type_style::POOL_COUNT,
                color: None,
            });
        }
    }
    let mut element = Element::new(role)
        .name(name)
        .padding(COUNTER_PADDING)
        .background([0; 4])
        .child(shown);
    element.style.border = Some(kit_ui::element::Border::uniform([0; 4], 0.0));
    element
}

/// The padding of a desktop counter: `.manual-counter-button`'s `padding: 2px`.
const COUNTER_PADDING: f32 = 2.0;

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
