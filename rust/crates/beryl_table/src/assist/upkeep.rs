use crate::action::Selection;
use crate::assist::{Assist, Line, Proposal, enabled};
use crate::helpers::untap_targets;
use crate::state::{Card, Face, Phase, Zone};
use crate::{Action, Seat, Table};

/// Offers the active seat the routine actions at the start of its turn.
///
/// [CR 502.3] says the active player determines “which permanents they control
/// will untap.” In [CR 504.1], “the active player draws a card.” The assist
/// follows [CR 103.8a], which says the player who plays first “skips the draw step”.
///
/// [CR 502.3]: https://yawgatog.com/resources/magic-rules/#R5023
/// [CR 504.1]: https://yawgatog.com/resources/magic-rules/#R5041
/// [CR 103.8a]: https://yawgatog.com/resources/magic-rules/#R1038a
#[must_use]
pub fn proposal(table: &Table, seat: Seat) -> Option<Proposal> {
    if !table.game_match.in_game
        || table.turn_plays != Some(0)
        || table.active != seat
        || table.phase != Phase::Untap
        || !enabled(table, seat, Assist::TurnUpkeep)
    {
        return None;
    }

    let line = |text: &str, action| Line {
        text: text.into(),
        whose: seat,
        on: true,
        actions: vec![action],
    };
    let mut lines = Vec::new();
    for card in untap_targets(table, seat) {
        let text = match &card.card {
            Card::Named(name) | Card::Custom { name, .. }
                if card.face != Face::Down && card.known_to.contains(&seat) =>
            {
                format!("Untap {name}")
            }
            _ => "Untap a card".into(),
        };
        lines.push(line(
            &text,
            Action::Tap {
                selection: Selection {
                    objects: vec![card.id],
                    zone: Zone::Battlefield,
                },
                tapped: false,
            },
        ));
    }
    if table.turn != 1 {
        lines.push(line("Draw a card", Action::Draw { seat, count: 1 }));
    }
    lines.push(line(
        "Go to first main",
        Action::Phase {
            phase: Phase::FirstMain,
        },
    ));

    Some(Proposal {
        assist: Assist::TurnUpkeep,
        lines,
    })
}
