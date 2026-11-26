//! Scenario actions from a game played some other way: the inverse of [`answer`](crate::answer),
//! and the actions as a card file's scenarios spell them, so that a game an agent or a fuzzer
//! played (a shrunk fuzz failure, an exhaustive search's failure) can be pasted into a card's
//! scenarios.

use beryl_card::{ChosenKind, Purpose};
use beryl_engine::card::{Castings, FaceIx, ObjectAbility};
use beryl_engine::chosen::ChosenValue;
use beryl_engine::{Choice, Decision, DecisionKind, GameTable, Layout, Opt, Seat, State};

use std::fmt::Write;

use crate::{Action, Permanent, Pick, Scenario};

/// The action that takes candidate `c` of `d` in a scenario, where the scenario vocabulary
/// can name it: choosing play or draw and spending floating mana have no
/// action. A pick by name takes the first candidate of that name, which need not be `c`:
/// compare [`answer`](crate::answer) of the result with `c` to tell.
pub fn script<L: Layout>(
    t: &GameTable,
    s: &State<L>,
    d: &Decision<L>,
    c: Choice,
) -> Option<Action> {
    let player = d.decider;
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
        Opt::Companion => Action::Companion { player },
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
    if let Some(Opt::Cast { slot, face, costs }) = d.candidates.option(c) {
        return Some(Pick::Cast {
            card: s.name(t, slot),
            face,
            costs,
        });
    }
    if let Some(Opt::Attack {
        slot,
        band: Some(first),
    }) = d.candidates.option(c)
    {
        return Some(Pick::Band {
            card: s.name(t, slot),
            first: (first != slot).then(|| s.name(t, first)),
        });
    }
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
        Opt::Keep => Pick::Keep,
        Opt::Mulligan => Pick::Mulligan,
        Opt::Done => Pick::Done,
        Opt::Number(n) if d.prompt.purpose == Purpose::Wish => Pick::Card {
            card: t.def(t.sideboard_card::<L::Card>(d.player, n)?).name,
        },
        // A card name of the game is a card, which the oracles name; "another card" stays a
        // number, which they read as a name no card of the scenario has.
        Opt::Number(n) if d.prompt.purpose == Purpose::CardName => {
            match ChosenValue::<L>::of_number(ChosenKind::CardName, n)
                .and_then(|value| value.card_name(t))
            {
                Some(card) => Pick::Card { card },
                None => Pick::Number { n },
            }
        }
        Opt::Number(n) => {
            value_name(t, d, n).map_or(Pick::Number { n }, |name| Pick::Named { name })
        }
        Opt::Plan(i) => Pick::Plan {
            i: u16::try_from(Into::<usize>::into(i)).ok()?,
        },
        Opt::Player(player) => Pick::Player { player },
        Opt::Defender(None) => Pick::DefendingPlayer,
        _ => return None,
    })
}

/// The name of the creature type a decision of `d` numbers `n`, as [`Pick::Named`] picks it:
/// the oracles name the type, not Beryl's number for it. A color stays a number, which they
/// read in the rules' order, a word of a text change a number, which the harness reads in
/// Beryl's order, and a card name a card ([`Pick::Card`]).
fn value_name<L: Layout>(t: &GameTable, d: &Decision<L>, n: u8) -> Option<&'static str> {
    let kind = ChosenKind::of_purpose(d.prompt.purpose)?;
    Some(match ChosenValue::<L>::of_number(kind, n)? {
        ChosenValue::Color(_) | ChosenValue::Name(_) | ChosenValue::Word(_) => return None,
        ChosenValue::CreatureType(Some(n)) => t
            .subtypes()
            .subtype(n)
            .map_or("Another creature type", |subtype| subtype.name()),
        ChosenValue::CreatureType(None) => "Another creature type",
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
        } => casting_source(face, costs).map_or_else(
            || format!("// {action:?}"),
            |(face, costs)| format!(
                "Action::Cast {{ player: {}, card: {card:?}, face: {face}, costs: {costs} }}",
                seat(player)
            ),
        ),
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
        Action::Activate {
            player,
            card,
            ability: ObjectAbility::Merged(place, i),
        } => format!(
            "Action::activate_merged({}, {card:?}, {place}, {})",
            seat(player),
            i.get()
        ),
        Action::TurnFaceUp { player, card } => {
            format!(
                "Action::TurnFaceUp {{ player: {}, card: {card:?} }}",
                seat(player)
            )
        }
        Action::Companion { player } => {
            format!("Action::Companion {{ player: {} }}", seat(player))
        }
        Action::Resolve => "Action::Resolve".to_owned(),
        Action::Concede { player } => format!("Action::Concede {{ player: {} }}", seat(player)),
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
        Action::Answer {
            player,
            kind,
            pick: Pick::Cast { card, face, costs },
        } => casting_source(face, costs).map_or_else(
            || format!("// {action:?}"),
            |(face, costs)| format!(
                "Action::answer({}, DecisionKind::{kind:?}, Pick::Cast {{ card: {card:?}, face: {face}, costs: {costs} }})",
                seat(player)
            ),
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

fn casting_source(face: FaceIx, costs: Castings) -> Option<(&'static str, String)> {
    let face = match face {
        FaceIx::FRONT => "FaceIx::FRONT",
        FaceIx::BACK => "FaceIx::BACK",
        _ => return None,
    };
    let costs = match costs {
        Castings::NONE => "Castings::NONE".to_owned(),
        Castings::ESCAPE => "Castings::ESCAPE".to_owned(),
        _ if costs.bits().is_power_of_two() => {
            format!("Castings::only({})", costs.bits().trailing_zeros())
        }
        _ => return None,
    };
    Some((face, costs))
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
            ("sideboard", player.sideboard),
        ] {
            if !cards.is_empty() {
                let _ = writeln!(text, "    .{zone}(P{p}, &{cards:?})");
            }
        }
        if let Some(card) = player.companion {
            let _ = writeln!(text, "    .companion(P{p}, {card:?})");
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
