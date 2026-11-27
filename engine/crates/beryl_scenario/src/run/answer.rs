//! The choice a scripted action names, found among a decision's candidates.

use beryl_engine::card::{ChosenKind, ManaSet, Purpose};
use beryl_engine::chosen::ChosenValue;
use beryl_engine::table::{is_sacrificed, is_simple};
use beryl_engine::{Choice, Decision, GameTable, Layout, Opt, State};

use crate::{Action, Pick};

/// The first option of `d` that `pick` accepts.
pub(super) fn option_index<L: Layout>(
    d: &Decision<L>,
    mut pick: impl FnMut(Opt<L>) -> bool,
) -> Option<Choice> {
    (0..d.len() as u16)
        .map(Choice)
        .find(|&c| d.candidates.option(c).is_some_and(&mut pick))
}

/// The choice that answers `d` as `action` would in a scenario, if `d` offers it, for an
/// action that answers one decision: playing a land, casting, activating, turning face up,
/// taking the companion action or an answer. Whose decision it is and of what kind is the
/// caller's to check. For scripts that drive agents in place of the runner, such as the UI
/// fixtures' boards.
pub fn answer<L: Layout>(
    t: &GameTable,
    s: &State<L>,
    d: &Decision<L>,
    action: &Action,
) -> Option<Choice> {
    match *action {
        Action::Answer { pick, .. } => pick_in(t, s, d, pick),
        _ => option_index(d, |o| takes(t, s, action, o)),
    }
}

/// Whether `o` is the priority option `action` takes: playing, casting, activating or turning
/// face up its card.
pub(super) fn takes<L: Layout>(t: &GameTable, s: &State<L>, action: &Action, o: Opt<L>) -> bool {
    let name = |slot| s.name(t, slot);
    match *action {
        Action::PlayLand { card, .. } => matches!(o, Opt::PlayLand(slot) if name(slot) == card),
        Action::Cast {
            card, face, costs, ..
        } => matches!(o, Opt::Cast { slot, face: f, costs: m }
            if name(slot) == card && f == face && m == costs),
        Action::Activate { card, ability, .. } => match o {
            Opt::Activate { slot, ability: a } => name(slot) == card && a == ability,
            // A mana ability activated at priority is offered as the mana it makes
            // (`Profile::taps_mana`): the one kind this ability makes. A source's mana ability
            // that sacrifices it, beside one that costs {T} alone, is offered as
            // `Opt::Activate` instead, and the color names the other.
            Opt::Mana { slot, color } => {
                name(slot) == card
                    && s.abilities(t, slot).any(|(ix, a)| {
                        ix == ability
                            && a.produces().colors == ManaSet::one(color).colors
                            && !(is_sacrificed(a)
                                && s.abilities(t, slot).any(|(_, other)| is_simple(other)))
                    })
            }
            _ => false,
        },
        Action::TurnFaceUp { card, .. } => {
            matches!(o, Opt::TurnFaceUp(slot) if name(slot) == card)
        }
        Action::Companion { .. } => matches!(o, Opt::Companion),
        _ => false,
    }
}

/// What a priority action takes, as a failure names it.
pub(super) fn taking(action: &Action) -> String {
    match *action {
        Action::PlayLand { card, .. } => format!("playing {card}"),
        Action::Cast {
            card, face, costs, ..
        } => format!(
            "casting {card} (face {}, costs {:#b})",
            face.get(),
            costs.bits()
        ),
        Action::Activate { card, ability, .. } => {
            format!("activating {card}'s ability {ability:?}")
        }
        Action::TurnFaceUp { card, .. } => format!("turning {card} face up"),
        Action::Companion { .. } => "putting the companion into hand".to_owned(),
        action => format!("{action:?}"),
    }
}

/// The option of `d` that `pick` names.
pub(super) fn pick_in<L: Layout>(
    t: &GameTable,
    s: &State<L>,
    d: &Decision<L>,
    pick: Pick,
) -> Option<Choice> {
    let wanted = match pick {
        // A card name is offered by number, as a catalog lists it.
        Pick::Card { card } if d.prompt.purpose == Purpose::CardName => {
            return option_index(d, |o| {
                matches!(o, Opt::Number(n) if ChosenValue::<L>::of_number(ChosenKind::CardName, n)
                    .is_some_and(|value| value.name(t) == card))
            });
        }
        Pick::Card { card } if d.prompt.purpose == Purpose::Wish => {
            return option_index(d, |o| {
                matches!(o, Opt::Number(index) if t.sideboard_card::<L::Card>(d.player, index)
                    .is_some_and(|found| t.def(found).name == card))
            });
        }
        Pick::Card { card } => {
            return (0..d.len() as u16).map(Choice).find(|&c| {
                d.candidates
                    .object(c)
                    .or_else(|| d.candidates.option(c).and_then(|o| o.object_named(s)))
                    .is_some_and(|slot| s.name(t, slot) == card)
            });
        }
        Pick::Band { card, first } => {
            return option_index(d, |o| {
                matches!(o, Opt::Attack { slot, band: Some(leader) }
                if s.name(t, slot) == card
                    && first.map_or(slot == leader, |name| {
                        slot != leader && s.name(t, leader) == name
                    }))
            });
        }
        Pick::AttackAll { card } => {
            return option_index(
                d,
                |o| matches!(o, Opt::AttackAll(slot) if s.name(t, slot) == card),
            );
        }
        Pick::Cast { card, face, costs } => {
            return option_index(d, |o| {
                matches!(o, Opt::Cast { slot, face: f, costs: m }
                if s.name(t, slot) == card && f == face && m == costs)
            });
        }
        Pick::Pass => Opt::Pass,
        Pick::Number { n } => Opt::Number(n),
        Pick::Yes => Opt::Yes,
        Pick::No => Opt::No,
        Pick::Keep => Opt::Keep,
        Pick::Mulligan => Opt::Mulligan,
        Pick::FreeMulligan => Opt::FreeMulligan,
        Pick::Done => Opt::Done,
        Pick::Player { player } => Opt::Player(player),
        Pick::DefendingPlayer => Opt::Defender(None),
        // A plan past what the layout's index names is no option.
        Pick::Plan { i } => Opt::Plan(L::Index::try_from(usize::from(i)).ok()?),
        Pick::Named { name } => {
            let kind = ChosenKind::of_purpose(d.prompt.purpose)?;
            return option_index(d, |o| match o {
                Opt::Number(n) => {
                    ChosenValue::<L>::of_number(kind, n).is_some_and(|v| v.name(t) == *name)
                }
                _ => false,
            });
        }
    };
    option_index(d, |o| o == wanted)
}

/// A decision as a scenario author reads it.
pub fn describe<L: Layout>(t: &GameTable, s: &State<L>, d: &Decision<L>) -> String {
    let candidates: Vec<String> = (0..d.len() as u16)
        .map(|i| {
            let c = Choice(i);
            match (d.candidates.object(c), d.candidates.option(c)) {
                (Some(slot), _) => s.name(t, slot).to_string(),
                (_, Some(Opt::Number(index))) if d.prompt.purpose == Purpose::Wish => {
                    t.sideboard_card::<L::Card>(d.player, index).map_or_else(
                        || format!("Number({index})"),
                        |card| t.def(card).name.to_owned(),
                    )
                }
                (_, Some(o)) => match o.object_named(s) {
                    Some(slot) => format!("{o:?} ({})", s.name(t, slot)),
                    None => format!("{o:?}"),
                },
                (None, None) => unreachable!("a choice within the candidates"),
            }
        })
        .collect();
    let turn = s.turn();
    let whose = if d.decider == d.player {
        String::new()
    } else {
        format!(" for player {}", d.player)
    };
    format!(
        "player {}{whose} {:?} ({}) in turn {} {:?}, among [{}]",
        d.decider,
        d.kind,
        d.prompt.purpose.name(),
        turn.number(),
        turn.step(),
        candidates.join(", ")
    )
}
