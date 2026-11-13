//! Scenario actions from a game played some other way: the inverse of [`answer`](crate::answer),
//! and the actions as a card file's scenarios spell them, so that a game an agent or a fuzzer
//! played (a shrunk fuzz failure, an exhaustive search's failure) can be pasted into a card's
//! scenarios.

use beryl_engine::card::{Castings, FaceIx, ObjectAbility};
use beryl_engine::{Choice, Decision, DecisionKind, GameTable, Layout, Opt, Seat, State};

use std::fmt::Write;

use crate::{Action, Permanent, Pick, Scenario};

/// The action that takes candidate `c` of `d` in a scenario, where the scenario vocabulary
/// can name it: the opening's play-or-draw and mulligans, and spending floating mana, have no
/// action. A pick by name takes the first candidate of that name, which need not be `c`:
/// compare [`answer`](crate::answer) of the result with `c` to tell.
pub fn script<L: Layout>(
    t: &GameTable,
    s: &State<L>,
    d: &Decision<L>,
    c: Choice,
) -> Option<Action> {
    let player = d.player;
    if d.kind != DecisionKind::Priority {
        return pick(t, s, d, c).map(|pick| Action::answer(player, d.kind, pick));
    }
    let card = |slot| s.name(t, slot);
    Some(match d.candidates.option(c)? {
        Opt::Pass => Action::pass(player),
        Opt::PlayLand(slot) => Action::PlayLand {
            player,
            card: card(slot),
        },
        Opt::Cast { slot, face, costs } => Action::Cast {
            player,
            card: card(slot),
            face,
            costs,
        },
        Opt::Activate { slot, ability } => Action::Activate {
            player,
            card: card(slot),
            ability,
        },
        Opt::TurnFaceUp(slot) => Action::TurnFaceUp {
            player,
            card: card(slot),
        },
        _ => return None,
    })
}

/// Candidate `c` of `d`, a decision other than priority, as a pick, where the scenario
/// vocabulary can name it. The pick may name an earlier candidate too, of the same name.
pub(crate) fn pick<L: Layout>(
    t: &GameTable,
    s: &State<L>,
    d: &Decision<L>,
    c: Choice,
) -> Option<Pick> {
    let named = d
        .candidates
        .object(c)
        .or_else(|| d.candidates.option(c)?.object_named(s));
    if let Some(slot) = named {
        return Some(Pick::Card {
            card: s.name(t, slot),
        });
    }
    Some(match d.candidates.option(c)? {
        Opt::Pass => Pick::Pass,
        Opt::Yes => Pick::Yes,
        Opt::No => Pick::No,
        Opt::Done => Pick::Done,
        Opt::Number(n) => Pick::Number { n },
        Opt::Plan(i) => Pick::Plan {
            i: u16::try_from(Into::<usize>::into(i)).ok()?,
        },
        Opt::Player(player) => Pick::Player { player },
        Opt::Defender(None) => Pick::DefendingPlayer,
        _ => return None,
    })
}

/// `action` as a card file's scenarios write it, or as a comment where no builder spells it
/// (several casting options at once, an assertion).
pub fn source(action: &Action) -> String {
    let seat = |p: Seat| format!("P{}", p.get());
    match *action {
        Action::PlayLand { player, card } => {
            format!(
                "Action::PlayLand {{ player: {}, card: {card:?} }}",
                seat(player)
            )
        }
        Action::Cast {
            player,
            card,
            face: FaceIx::FRONT,
            costs: Castings::NONE,
        } => format!("Action::cast({}, {card:?})", seat(player)),
        Action::Cast {
            player,
            card,
            face,
            costs,
        } if costs.bits().count_ones() <= 1 && matches!(face, FaceIx::FRONT | FaceIx::BACK) => {
            let face = if face == FaceIx::FRONT {
                "FaceIx::FRONT"
            } else {
                "FaceIx::BACK"
            };
            let costs = match costs.bits() {
                0 => "Castings::NONE".to_owned(),
                bits => format!("Castings::only({})", bits.trailing_zeros()),
            };
            format!(
                "Action::Cast {{ player: {}, card: {card:?}, face: {face}, costs: {costs} }}",
                seat(player)
            )
        }
        Action::Activate {
            player,
            card,
            ability: ObjectAbility::Own(i),
        } => format!("Action::activate({}, {card:?}, {})", seat(player), i.get()),
        Action::Activate {
            player,
            card,
            ability: ObjectAbility::Granted(k),
        } => format!("Action::activate_granted({}, {card:?}, {k})", seat(player)),
        Action::TurnFaceUp { player, card } => {
            format!(
                "Action::TurnFaceUp {{ player: {}, card: {card:?} }}",
                seat(player)
            )
        }
        Action::Resolve => "Action::Resolve".to_owned(),
        Action::AdvanceTo { turn, step } => {
            format!("Action::AdvanceTo {{ turn: {turn}, step: TurnStep::{step:?} }}")
        }
        Action::Answer {
            player,
            pick: Pick::Pass,
            kind: DecisionKind::Priority,
        } => format!("Action::pass({})", seat(player)),
        Action::Answer {
            player,
            kind,
            pick: Pick::Card { card },
        } => format!(
            "Action::choose({}, DecisionKind::{kind:?}, {card:?})",
            seat(player)
        ),
        Action::Answer { player, kind, pick } => {
            let pick = match pick {
                Pick::Player { player } => format!("Pick::Player {{ player: {} }}", seat(player)),
                pick => format!("Pick::{pick:?}"),
            };
            format!(
                "Action::answer({}, DecisionKind::{kind:?}, {pick})",
                seat(player)
            )
        }
        action => format!("// {action:?}"),
    }
}

/// The setup of `scenario` as a card file's scenarios write it: `Scenario::new` and its
/// builder calls, each zone that holds a card, but not its actions.
pub fn setup_source(scenario: &Scenario) -> String {
    let setup = &scenario.setup;
    let mut text = format!("Scenario::new({:?})\n", scenario.name);
    let _ = writeln!(
        text,
        "    .seed({})\n    .at({}, P{}, TurnStep::{:?})",
        setup.seed,
        setup.turn,
        setup.active.get(),
        setup.step
    );
    for p in Seat::BOTH {
        let player = &setup.players[p.index()];
        let p = p.get();
        if player.life != 20 {
            let _ = writeln!(text, "    .life(P{p}, {})", player.life);
        }
        if !player.battlefield.is_empty() {
            let _ = writeln!(text, "    .battlefield(P{p}, &{:?})", player.battlefield);
        }
        if !player.permanents.is_empty() {
            let permanents: Vec<String> = player.permanents.iter().map(permanent_source).collect();
            let _ = writeln!(text, "    .permanents(P{p}, &[{}])", permanents.join(", "));
        }
        for (zone, cards) in [
            ("hand", player.hand),
            ("graveyard", player.graveyard),
            ("exile", player.exile),
            ("library", player.library),
        ] {
            if !cards.is_empty() {
                let _ = writeln!(text, "    .{zone}(P{p}, &{cards:?})");
            }
        }
    }
    text
}

/// A permanent of a setup as its builder writes it.
fn permanent_source(b: &Permanent) -> String {
    let mut text = if b.token {
        format!("Permanent::token({:?})", b.card)
    } else {
        format!("Permanent::new({:?})", b.card)
    };
    match b.face {
        FaceIx::FRONT => {}
        FaceIx::BACK => text.push_str(".transformed()"),
        FaceIx::DOWN => text.push_str(".face_down()"),
        face => {
            let _ = write!(text, " /* face {} */", face.get());
        }
    }
    if b.tapped {
        text.push_str(".tapped()");
    }
    if b.sick {
        text.push_str(".sick()");
    }
    if let Some(c) = b.counters {
        let _ = write!(text, ".counters(CounterKind::{:?}, {})", c.kind, c.count);
    }
    if !b.exiled_with.is_empty() {
        let _ = write!(text, ".exiled_with(&{:?})", b.exiled_with);
    }
    text
}
