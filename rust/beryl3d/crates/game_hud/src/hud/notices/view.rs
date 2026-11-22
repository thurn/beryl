use std::fmt::Write as _;

use super::Inputs;
use beryl_protocol::protocol::{Side, TablePhase};
use beryl_protocol::script::Notice;
use kit_base::tokens::{color, space, type_style};
use kit_ui::element::{Align, Element, column, text};

/// What the notice shows, with names from the table it is shown over.
pub(super) fn element(input: &Inputs<'_>) -> Element {
    match input.notice {
        Notice::Turn { active } => {
            // The web tints only its rosettes and glow by player; the words are cream for both
            // (`.turn-banner-title`).
            let words = match active {
                Side::You => "Your Turn",
                Side::Opponent => "Opponent's Turn",
            };
            banner(
                text(words)
                    .style(type_style::TURN_BANNER_TITLE)
                    .color(color::TURN_BANNER_TITLE),
                None,
            )
        }
        Notice::Scry { top, bottom, .. } => pill(&format!("Scry: {top} top, {bottom} bottom.")),
        Notice::TopOrBottom { top, .. } => pill(if *top { "Top" } else { "Bottom" }),
        Notice::Coin { player, won, .. } => {
            let title = input.coin_source().unwrap_or("Coin flip");
            let who = who(*player);
            let result = if *won { "won" } else { "lost" };
            step_banner(title, Some(&format!("{who} {result} the flip")))
        }
        Notice::Roll { sides, outcomes } => roll(*sides, outcomes, input.roll_seats()),
        Notice::Shuffle => step_banner("Shuffled", None),
        Notice::Rewind { to } => step_banner(&format!("Rewound to {to}"), None),
        Notice::Phase { phase } => step_banner(phase_name(*phase), None),
        Notice::TableTurn { name } => step_banner(&format!("{name}'s turn"), None),
        Notice::Game { number } => step_banner(&format!("Game {number}"), None),
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
fn roll(sides: u32, outcomes: &[u32], seats: Option<(usize, Option<&str>)>) -> Element {
    let results: Vec<String> = (outcomes.iter())
        .map(|&outcome| match (sides, outcome) {
            (2, 0) => "Heads".to_owned(),
            (2, _) => "Tails".to_owned(),
            _ => (u64::from(outcome) + 1).to_string(),
        })
        .collect();
    if let Some((you, peer)) = seats {
        let peer = peer.unwrap_or("Opponent");
        let mut detail = String::new();
        for (seat, result) in results.iter().enumerate() {
            if seat > 0 {
                detail.push_str(" · ");
            }
            let name = if seat == you { "You" } else { peer };
            let _ = write!(detail, "{name}: {result}");
        }
        return step_banner("Rolled dice (d6)", Some(&detail));
    }
    if let [result] = results.as_slice() {
        return if sides == 2 {
            step_banner(result, None)
        } else {
            step_banner(&format!("Rolled {result} (d{sides})"), None)
        };
    }
    let title = if sides == 2 {
        "Flipped coins".to_owned()
    } else {
        format!("Rolled dice (d{sides})")
    };
    step_banner(&title, Some(&results.join(", ")))
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
pub fn step_banner(title: &str, detail: Option<&str>) -> Element {
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
