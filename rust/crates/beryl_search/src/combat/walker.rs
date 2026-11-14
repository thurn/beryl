//! A short clock from a planeswalker's alternate-win abilities.

use beryl_engine::card::{
    Ability, AbilityIx, Amount, Behavior, Condition, CostPart, Effect, ManaCost, Objects, Players,
    Replacement, TargetSpec,
};
use beryl_engine::filter::Filter;
use beryl_engine::state::Tracker;
use beryl_engine::{GameTable, Layout, State, Zone, wins_instead_of_drawing};

// A five-turn clock is worth at least 40 points, about a small creature.
const FORECAST_TURNS: i32 = 5;
const WIN_PREMIUM: i32 = 200;

pub(super) fn value<L: Layout>(t: &GameTable, s: &State<L>, slot: L::Slot) -> i32 {
    let face = s.copiable(slot).face(t);
    let abilities = (0u8..)
        .zip(face.abilities)
        .filter(|&(index, a)| {
            a.zones().contains(Zone::Battlefield) && s.has_ability(slot, AbilityIx::new(index))
        })
        .map(|(_, a)| a);
    let controller = s.controller(slot);
    let on_turn = s.turn().active() == controller;
    clock(
        abilities,
        s.library(controller).len(),
        i16::from(s.object(slot).counters().loyalty),
        on_turn,
        on_turn && s.tracked(Tracker::LoyaltyActivated, slot),
    )
    .map_or(0, |turns| WIN_PREMIUM / turns)
}

fn clock<'a>(
    abilities: impl Iterator<Item = &'a Ability> + Clone,
    mut cards: usize,
    mut loyalty: i16,
    on_turn: bool,
    used_loyalty: bool,
) -> Option<i32> {
    let wins_on_empty_draw = abilities.clone().any(|a| {
        a.replacement() == Some(&Replacement::draws(Players::You))
            && a.condition().is_some_and(|c| empty_library(*c))
            && wins_instead_of_drawing(a)
    });
    let activations = abilities.filter_map(|a| {
        let [CostPart::Loyalty(change)] = a.cost().parts() else {
            return None;
        };
        let Behavior::Data(effect) = a.body() else {
            return None;
        };
        // The only targeted abilities modeled here offer one unrestricted player target.
        if a.cost().mana != ManaCost::NONE
            || a.condition().is_some()
            || !matches!(a.targets(), [] | [_])
            || a.targets()
                .first()
                .is_some_and(|&target| target != TargetSpec::player())
        {
            return None;
        }
        Some((i16::from(*change), !a.targets().is_empty(), effect))
    });
    if !wins_on_empty_draw
        && !activations.clone().any(|(_, _, effect)| {
            effect.any(&|effect| matches!(effect, Effect::WinGame(Players::You)))
        })
    {
        return None;
    }
    // Assume the planeswalker survives and its controller gets one normal draw and at
    // most one activation per turn. Ignore other permanents and spells in the forecast.
    for turn in 1..=FORECAST_TURNS {
        if turn > 1 || !on_turn {
            if cards == 0 {
                return wins_on_empty_draw.then_some(turn);
            }
            cards -= 1;
        } else if used_loyalty {
            continue;
        }
        // Keep the line that gets closest to an empty library. With equal progress,
        // keep more loyalty available for subsequent turns.
        let mut best = (cards, -loyalty);
        for (change, self_target, effect) in activations.clone() {
            let remaining = loyalty + change;
            if remaining < 0 {
                continue;
            }
            // Paying the last loyalty removes the replacement before the ability resolves.
            match apply(
                effect,
                cards,
                self_target,
                wins_on_empty_draw && remaining > 0,
            ) {
                Progress::Win => return Some(turn),
                Progress::Library(left) if remaining > 0 => {
                    best = best.min((left, -remaining));
                }
                Progress::Library(_) | Progress::Unknown => {}
            }
        }
        (cards, loyalty) = (best.0, -best.1);
    }
    None
}

fn empty_library(condition: Condition) -> bool {
    // Library counts are nonnegative. Both equality and an upper bound can express zero.
    matches!(
        condition,
        Condition::Compare(
            Amount::Count(Objects::All {
                zone: Zone::Library,
                filter: Filter::Whose(Players::You),
            }),
            comparison,
            Amount::Fixed(0)
        ) if comparison.holds(0, 0) && !comparison.holds(1, 0)
    )
}

enum Progress {
    Win,
    Library(usize),
    Unknown,
}

/// Follow simple draw and mill instructions in order. Unmodeled instructions
/// end the projection for that activation.
fn apply(effect: &Effect, cards: usize, self_target: bool, draw_win: bool) -> Progress {
    match *effect {
        Effect::Sequence(effects) => {
            let mut cards = cards;
            for effect in effects {
                match apply(effect, cards, self_target, draw_win) {
                    Progress::Library(left) => cards = left,
                    result => return result,
                }
            }
            Progress::Library(cards)
        }
        Effect::Draw {
            player: Players::You,
            count: Amount::Fixed(count),
        } => {
            let count = usize::from(count);
            if count > cards && draw_win {
                Progress::Win
            } else {
                Progress::Library(cards.saturating_sub(count))
            }
        }
        Effect::Mill {
            player,
            count: Amount::Fixed(count),
        } if player == Players::You
            || (self_target && matches!(player, Players::Target(index) if index.index() == 0)) =>
        {
            Progress::Library(cards.saturating_sub(usize::from(count)))
        }
        Effect::If {
            condition,
            then,
            otherwise,
        } if empty_library(condition) => {
            let branch = if cards == 0 { Some(then) } else { otherwise };
            branch.map_or(Progress::Library(cards), |effect| {
                apply(effect, cards, self_target, draw_win)
            })
        }
        Effect::WinGame(Players::You) => Progress::Win,
        _ => Progress::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beryl_engine::card::Types;

    #[test]
    fn every_planeswalker_has_a_stable_clock() {
        for &card in beryl_cards::CARDS.cards() {
            for face in card
                .faces
                .iter()
                .filter(|f| f.types.intersects(Types::PLANESWALKER))
            {
                let clocks = |loyalties: std::ops::RangeInclusive<i16>| {
                    loyalties.flat_map(|loyalty| {
                        (0..=20).map(move |cards| {
                            clock(face.abilities.iter(), cards, loyalty, false, false)
                        })
                    })
                };
                assert!(clocks(1..=12).eq(clocks(1..=12)), "{}", card.name);
            }
        }
    }
}
