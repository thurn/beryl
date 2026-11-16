//! The notices `announce` steps show, as floats of the flat layer (docs/beryl-3d.md, "Verbs"
//! and "Floats"): the turn banner and the other banners across the middle of the window
//! (`floats::BANNER`), the pill above a player's life total that says what their scry did
//! (`floats::SCRY_PILL`), and a coin's flip (`floats::COIN`). The words are the web
//! interface's (`ui/src/stage/notices.ts` and `ui/src/game/TurnBanner.tsx`).
//!
//! - `NoticeFloats::prepare` lays out the element of every notice of the playing and waiting
//!   scripts as each update arrives, under the notice's key (`notice::key`), so that a frame
//!   lays nothing out.
//! - `NoticeFloats::write` shows in each float the notice under way there that began last, as
//!   the web's `announced` picks one, and sets its look: it fades in over the first
//!   `FADE_IN` of its run and out over the last `FADE_OUT`.

use std::fmt::Write as _;

use beryl_protocol::protocol::{PlayerView, Side, TablePhase};
use beryl_protocol::script::Notice;
use glam::Vec2;

use super::counts::{HALF_LIFE, index, points};
use crate::element::{Align, Element, FloatId, column, floats, text};
use crate::input::Size;
use crate::layout::{FloatLook, Layer};
use crate::notice::{Announced, NoticeRun, Place};
use crate::table::Layout;
use crate::tokens::{color, space, type_style};

/// The share of a notice's run over which it fades in.
const FADE_IN: f32 = 0.15;
/// The share of a notice's run over which it fades out.
const FADE_OUT: f32 = 0.2;

/// Every float a notice is shown in, with the place that shows there.
const FLOATS: [(FloatId, Place); 4] = [
    (floats::BANNER, Place::Banner),
    (floats::SCRY_PILL[0], Place::Pill(Side::You)),
    (floats::SCRY_PILL[1], Place::Pill(Side::Opponent)),
    (floats::COIN, Place::Coin),
];

/// The floats that show notices, and the key each shows now.
#[derive(Debug, Default)]
pub struct NoticeFloats {
    shown: [Option<u64>; FLOATS.len()],
}

impl NoticeFloats {
    /// Lays out in `layer` the element of each notice in `coming` (`Show::coming_notices`),
    /// and forgets those laid out before that are neither coming nor in `held`
    /// (`Show::notices`). The client calls it as each update arrives, so that `write` lays
    /// nothing out and allocates nothing.
    pub fn prepare<'a>(
        layer: &mut Layer,
        coming: impl Iterator<Item = (usize, &'a [NoticeRun], &'a PlayerView)>,
        held: &[Announced],
    ) {
        let mut keys: Vec<u64> = held.iter().map(|announced| announced.key).collect();
        for (script, runs, view) in coming {
            for run in runs {
                let key = crate::notice::key(script, run.step);
                keys.push(key);
                let place = Place::of(&run.notice);
                layer.prepare(float_of(place), key, || {
                    element(&run.notice, view).name(name_of(place))
                });
            }
        }
        for (id, _) in FLOATS {
            layer.forget_prepared(id, |key| keys.contains(&key));
        }
    }

    /// Shows in `layer` the notices of `notices` under way at `now`, placed for `layout` in a
    /// window of `size`, or none of them unless `visible`. Lays nothing out and allocates
    /// nothing for what `prepare` laid out.
    pub fn write(
        &mut self,
        layer: &mut Layer,
        notices: &[Announced],
        now: u64,
        (layout, size): (&Layout, Size),
        visible: bool,
    ) {
        for (at, (id, place)) in FLOATS.into_iter().enumerate() {
            let showing = (notices.iter())
                .rev()
                .find(|announced| visible && announced.place == place && announced.under_way(now));
            let want = showing.map(|announced| announced.key);
            if want != self.shown[at] {
                let shown = layer.show_prepared(id, want);
                debug_assert!(shown, "nothing is prepared under {want:?} for float {id:?}");
                if !shown {
                    layer.show_prepared(id, None);
                }
                self.shown[at] = want.filter(|_| shown);
            }
            let Some(announced) = showing else {
                continue;
            };
            let (point, anchor) = match place {
                Place::Banner | Place::Coin => (
                    Vec2::new(size.width / 2.0, size.height / 2.0),
                    Vec2::new(0.5, 0.5),
                ),
                Place::Pill(player) => {
                    let life = points(layout.players[index(player)].life, size);
                    (
                        life - Vec2::new(0.0, HALF_LIFE + space::S),
                        Vec2::new(0.5, 1.0),
                    )
                }
            };
            layer.set_float_look(
                id,
                FloatLook {
                    point,
                    anchor,
                    opacity: opacity(announced, now),
                    tint: None,
                },
            );
        }
    }
}

/// The float `place` is shown in.
fn float_of(place: Place) -> FloatId {
    FLOATS
        .iter()
        .find(|(_, shows)| *shows == place)
        .map_or(floats::BANNER, |(id, _)| *id)
}

/// What the tree calls the float `place` is shown in: each its own name, since two notices may
/// stand at once.
const fn name_of(place: Place) -> &'static str {
    match place {
        Place::Banner => "banner",
        Place::Pill(Side::You) => "your pill",
        Place::Pill(Side::Opponent) => "opponent's pill",
        Place::Coin => "coin",
    }
}

/// How opaque `announced` is at `now`: rising from 0 to 1 over the first `FADE_IN` of its run,
/// and falling back to 0 over the last `FADE_OUT`.
fn opacity(announced: &Announced, now: u64) -> f32 {
    let over = announced.over;
    if over.duration() == 0 {
        return 1.0;
    }
    #[expect(clippy::cast_precision_loss, reason = "a notice lasts seconds at most")]
    let through = (now.saturating_sub(over.from)) as f32 / over.duration() as f32;
    (through / FADE_IN)
        .min((1.0 - through) / FADE_OUT)
        .clamp(0.0, 1.0)
}

/// What `notice` shows, where `view` is the table it is shown over.
fn element(notice: &Notice, view: &PlayerView) -> Element {
    match notice {
        Notice::Turn { active } => {
            let (words, tint) = match active {
                Side::You => ("Your Turn", color::CYAN),
                Side::Opponent => ("Opponent's Turn", color::EMBER),
            };
            banner(text(words).style(type_style::TITLE).color(tint), None)
        }
        Notice::Scry { top, bottom, .. } => pill(&format!("Scry: {top} top, {bottom} bottom.")),
        Notice::TopOrBottom { top, .. } => pill(if *top { "Top" } else { "Bottom" }),
        Notice::Coin {
            player,
            won,
            source,
        } => {
            let title = (view.objects.iter())
                .find(|object| object.slot == *source)
                .and_then(|object| object.name.as_deref())
                .unwrap_or("Coin flip");
            let who = who(*player);
            let result = if *won { "won" } else { "lost" };
            titled(title, Some(&format!("{who} {result} the flip")))
        }
        Notice::Roll { sides, outcomes } => roll(*sides, outcomes, view),
        Notice::Shuffle => titled("Shuffled", None),
        Notice::Rewind { to } => titled(&format!("Rewound to {to}"), None),
        Notice::Phase { phase } => titled(phase_name(*phase), None),
        Notice::TableTurn { name } => titled(&format!("{name}'s turn"), None),
        Notice::Game { number } => titled(&format!("Game {number}"), None),
    }
}

/// "You" or "Opponent", as a banner names a player.
const fn who(side: Side) -> &'static str {
    match side {
        Side::You => "You",
        Side::Opponent => "Opponent",
    }
}

/// The banner of a roll, as the web words it (`bannerWords` in `ui/src/stage/notices.ts`):
/// a two-sided roll's outcomes are heads and tails, and the roll that decides who chooses to go
/// first names each seat's result.
fn roll(sides: u32, outcomes: &[u32], view: &PlayerView) -> Element {
    let results: Vec<String> = (outcomes.iter())
        .map(|&outcome| match (sides, outcome) {
            (2, 0) => "Heads".to_owned(),
            (2, _) => "Tails".to_owned(),
            _ => (u64::from(outcome) + 1).to_string(),
        })
        .collect();
    if let Some(table) = view.table.as_ref().filter(|table| table.choosing_first)
        && results.len() == 2
    {
        let you = usize::from(table.seat.unwrap_or(0));
        let peer = table.peer_name.as_deref().unwrap_or("Opponent");
        let mut detail = String::new();
        for (seat, result) in results.iter().enumerate() {
            if seat > 0 {
                detail.push_str(" · ");
            }
            let name = if seat == you { "You" } else { peer };
            let _ = write!(detail, "{name}: {result}");
        }
        return titled("Rolled dice (d6)", Some(&detail));
    }
    if let [result] = results.as_slice() {
        return if sides == 2 {
            titled(result, None)
        } else {
            titled(&format!("Rolled {result} (d{sides})"), None)
        };
    }
    let title = if sides == 2 {
        "Flipped coins".to_owned()
    } else {
        format!("Rolled dice (d{sides})")
    };
    titled(&title, Some(&results.join(", ")))
}

/// The name of a manual table's phase, as the web's `PHASES` has it.
const fn phase_name(phase: TablePhase) -> &'static str {
    match phase {
        TablePhase::Untap => "Untap",
        TablePhase::Upkeep => "Upkeep",
        TablePhase::Draw => "Draw",
        TablePhase::FirstMain => "First main",
        TablePhase::BeginningOfCombat => "Beginning of combat",
        TablePhase::DeclareAttackers => "Declare attackers",
        TablePhase::DeclareBlockers => "Declare blockers",
        TablePhase::CombatDamage => "Combat damage",
        TablePhase::EndOfCombat => "End of combat",
        TablePhase::SecondMain => "Second main",
        TablePhase::End => "End step",
    }
}

/// A banner with `title` in the title style, and `detail` below it if any.
fn titled(title: &str, detail: Option<&str>) -> Element {
    banner(text(title).style(type_style::TITLE), detail)
}

/// A banner: `title` above `detail`, centred, on a panel.
fn banner(title: Element, detail: Option<&str>) -> Element {
    column()
        .align_items(Align::Center)
        .padding(space::L)
        .gap(space::S)
        .radius(space::M)
        .child(title)
        .children(detail.map(text))
}

/// A pill: `words` on a small rounded panel.
fn pill(words: &str) -> Element {
    column()
        .align_items(Align::Center)
        .padding(space::M)
        .radius(space::M)
        .child(text(words))
}
