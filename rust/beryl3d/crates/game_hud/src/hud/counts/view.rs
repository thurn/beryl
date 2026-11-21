use std::fmt::Write as _;

use beryl_protocol::prompt::Click;
use beryl_protocol::protocol::{ManaPool, Side};
use beryl_protocol::script::Subject;
use kit_base::data::profiles::LifeText;
use kit_base::tokens::{color, size, type_style};
use kit_frame::frame::Role;
use kit_ui::element::{Align, Element, Span, TextAlign, holder, text};

/// A player's life total, if any: a player element that shows the number, named for whose it
/// is and the number, as "You 20", so that the tree's line for it says both. As a `target`, a
/// press on it clicks the open prompt on the player, as the web interface's `PlayerTarget`
/// (`ui/src/stage/PromptBar.tsx`) does.
///
/// On the desktop it is the web's `.life-orb-total`, large and bold in the cream of
/// `color::LIFE`; the phone's smaller orb keeps the prompt's title size. A float's top element
/// is drawn on a panel unless it says otherwise, and the total stands on its orb with none.
pub(super) fn life_total(
    (side, name): (Side, &str),
    life: Option<i32>,
    (target, passes, style): (bool, bool, LifeText),
) -> Option<Element> {
    let life = life?;
    let mut element = Element::new(Role::Player)
        .name(&format!("{name} {life}"))
        .style(style.style())
        .color(color::LIFE)
        .background([0; 4])
        .border(([0; 4], 0.0));
    // A total the prompt can pick is a touch target as large as an icon button, the phone's
    // least, centred on the orb as the total is, so the orb around it still takes no press.
    // Any other total on the game screen lets the pointer through, as the orb does, to a card
    // of the phone's hand that stands behind it.
    if target {
        element = element
            .on_press(Click::On {
                on: Subject::Player { player: side },
            })
            .min_width(size::ICON_BUTTON)
            .min_height(size::ICON_BUTTON)
            .text_align(TextAlign::Center);
    } else {
        element = element.passes(passes);
    }
    element.text = Some(life.to_string());
    Some(element)
}

/// A pool of unspent mana, as the web interface's `ManaPool` (`ui/src/components/Player.tsx`)
/// shows it: large mana symbols 6 points apart, each followed by its count in small bold type
/// when there is more than one (`.mana-pool` in
/// `ui/src/design/components/mana-pool-and-coin.css`), in the order W, U, B, R, G and then
/// colorless. The flat layer draws each `{U}` as an inline mana symbol, a disc the size of the
/// type's em.
pub(super) fn pool_text(pool: ManaPool) -> Element {
    holder()
        .name(&mana(pool))
        .gap(size::POOL_GAP)
        .align_items(Align::Center)
        .passes(true)
        .children(entries(pool).map(|(count, symbol)| {
            let mut words = format!("{{{symbol}}}");
            let symbol_end = words.len();
            if count > 1 {
                let _ = write!(words, "{count}");
            }
            let mut entry = text(&words).style(type_style::POOL);
            #[expect(clippy::cast_possible_truncation, reason = "a short text")]
            let range = symbol_end as u32..words.len() as u32;
            if !range.is_empty() {
                entry.spans.push(Span {
                    range,
                    style: type_style::POOL_COUNT,
                    color: None,
                });
            }
            entry
        }))
}

/// Each kind of mana in `pool`, with how much of it there is, in the order W, U, B, R, G and
/// then colorless, leaving out the kinds there is none of.
fn entries(pool: ManaPool) -> impl Iterator<Item = (u32, char)> {
    [
        (pool.white, 'W'),
        (pool.blue, 'U'),
        (pool.black, 'B'),
        (pool.red, 'R'),
        (pool.green, 'G'),
        (pool.colorless, 'C'),
    ]
    .into_iter()
    .filter(|&(count, _)| count > 0)
}

/// By how much a life total changed, with its sign.
pub(super) fn life_change(by: i32) -> Element {
    text(&format!("{by:+}"))
        .style(type_style::TITLE)
        .passes(true)
}

/// Unspent mana as text, the pool's name in the tree: one mana symbol for each kind there is,
/// followed by its count when that is more than one, as the web's `ManaPool` shows it, as
/// "{U}2 {G}".
fn mana(pool: ManaPool) -> String {
    let mut out = String::new();
    for (count, symbol) in entries(pool) {
        if !out.is_empty() {
            out.push(' ');
        }
        let _ = write!(out, "{{{symbol}}}");
        if count > 1 {
            let _ = write!(out, "{count}");
        }
    }
    out
}
