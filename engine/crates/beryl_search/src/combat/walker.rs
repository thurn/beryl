//! A short clock from a planeswalker's alternate-win abilities.

use beryl_engine::card::{
    Ability, AbilityIx, Amount, Behavior, Condition, CostPart, Effect, ManaCost, Objects, Players,
    Replacement, TargetSpec,
};
use beryl_engine::filter::Filter;
use beryl_engine::state::Tracker;
use beryl_engine::{GameTable, Layout, State, Zone, wins_instead_of_drawing};

use crate::decking::next_drawer;

// A five-turn clock is worth at least 40 points, about a small creature.
const FORECAST_TURNS: i32 = 5;
const WIN_PREMIUM: i32 = 200;

#[allow(
    clippy::disallowed_methods,
    reason = "values a permanent by the printed abilities of its face up"
)]
pub(super) fn value<L: Layout>(t: &GameTable, s: &State<L>, slot: L::Slot) -> i32 {
    let face = s.definition(slot).face(t);
    let abilities = (0u8..)
        .zip(face.abilities)
        .filter(|&(index, a)| {
            a.zones().contains(Zone::Battlefield) && s.has_ability(slot, AbilityIx::new(index))
        })
        .map(|(_, a)| a);
    let controller = s.controller(slot);
    let on_turn = s.turn().active() == controller;
    let library = Library {
        cards: s.library(controller).len(),
        shared: s.shares_zones(),
        opponent_first: !on_turn && next_drawer(s) != controller,
    };
    clock(
        abilities,
        library,
        i16::try_from(beryl_engine::Number::to_i64(
            s.object(slot).counters().loyalty,
        ))
        .unwrap_or(i16::MAX),
        on_turn,
        on_turn && s.tracked(Tracker::LoyaltyActivated, slot),
    )
    .map_or(0, |turns| WIN_PREMIUM / turns)
}

/// The library a planeswalker's clock runs down.
#[derive(Clone, Copy)]
struct Library {
    /// The cards in the controller's library.
    cards: usize,
    /// Whether the opponent draws from it too, as in the shared-deck format.
    shared: bool,
    /// Whether the opponent's draw step comes before the controller's next one this turn: on
    /// the opponent's turn before their draw.
    opponent_first: bool,
}

/// How many of the controller's turns, counting this one when it is theirs, the planeswalker
/// takes to win, if it does within [`FORECAST_TURNS`]. The draws are counted in turn order:
/// the controller's one each turn, and in a shared library the opponent's one each turn
/// between theirs as well, so that the opponent drawing from the empty library is a win
/// ([`crate::decking`]).
fn clock<'a>(
    abilities: impl Iterator<Item = &'a Ability> + Clone,
    library: Library,
    mut loyalty: i16,
    on_turn: bool,
    used_loyalty: bool,
) -> Option<i32> {
    let mut cards = library.cards;
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
    // most one activation per turn, and the opponent one draw per turn from a shared library.
    // Ignore other permanents and spells in the forecast.
    for turn in 1..=FORECAST_TURNS {
        if library.shared && (turn > 1 || library.opponent_first) {
            if cards == 0 {
                return Some(turn);
            }
            cards -= 1;
        }
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
    use beryl_engine::Zone;
    use beryl_engine::card::{
        Ability, Amount, Comparison, Condition, Effect, Objects, Players, Replacement, SpecIx,
        TargetSpec,
    };
    use beryl_engine::filter::Filter;

    use super::{Library, clock};

    const LIBRARY_IS_EMPTY: Condition = Condition::Compare(
        Amount::Count(&Objects::All {
            zone: Zone::Library,
            filter: Filter::Whose(&Players::You),
        }),
        Comparison::Equal,
        Amount::Fixed(0),
    );

    static YOU_WIN: Effect = Effect::WinGame(Players::You);

    const fn draw(count: u8) -> Effect {
        Effect::Draw {
            player: Players::You,
            count: Amount::Fixed(count),
        }
    }

    /// The abilities of Jace, Wielder of Mysteries: "If you would draw a card while your
    /// library has no cards in it, you win the game instead. +1: Target player mills two
    /// cards. Draw a card. −8: Draw seven cards. Then if your library has no cards in it, you
    /// win the game."
    static ABILITIES: [Ability; 3] = [
        Ability::replacing(&Replacement::draws(Players::You))
            .only_if(&LIBRARY_IS_EMPTY)
            .effect(&YOU_WIN),
        Ability::loyalty(1)
            .target(&TargetSpec::player())
            .effect(&Effect::Sequence(&[
                Effect::Mill {
                    player: Players::Target(SpecIx::new(0)),
                    count: Amount::Fixed(2),
                },
                draw(1),
            ])),
        Ability::loyalty(-8).effect(&Effect::Sequence(&[
            draw(7),
            Effect::If {
                condition: LIBRARY_IS_EMPTY,
                then: &YOU_WIN,
                otherwise: None,
            },
        ])),
    ];

    /// Jace, Wielder of Mysteries with twelve cards in his controller's library, on the
    /// opponent's turn after their draw: each of his turns takes a draw and his +1's two milled
    /// cards and one drawn. From a library of his controller's own, the third +1 leaves it empty
    /// and the fourth turn's draw wins. From a shared one, the opponent's draws take a card a turn
    /// too, so the third +1 already draws from the empty library and wins.
    #[test]
    fn a_shared_library_runs_down_with_both_players_draws() {
        let turns = |shared| {
            let library = Library {
                cards: 12,
                shared,
                opponent_first: false,
            };
            clock(ABILITIES.iter(), library, 4, false, false)
        };
        assert_eq!(turns(false), Some(4));
        assert_eq!(turns(true), Some(3));
    }
}
