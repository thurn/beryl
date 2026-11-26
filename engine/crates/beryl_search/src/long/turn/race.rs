//! With [`crate::Options::race`], the budgeted turn search stops scoring root moves that the
//! leader clearly beats, and spends the work on more worlds for the rest.

use super::{Choice, mean_and_error};

/// Scoring worlds between checks for roots to drop, after the first
/// [`super::MIN_SCORING_WORLDS`].
pub(super) const RACE_INTERVAL: u16 = 16;
/// A root is dropped when the leader's mean lead over it, in the worlds both were scored in,
/// exceeds this many standard errors of the paired difference.
const RACE_ERRORS: f64 = 3.0;

/// The scored root with the highest mean reward, or `default` when none was scored.
///
/// A root dropped by the race has fewer rewards than the default, which is never dropped. It
/// is never chosen: the leader clearly beat it. The roots left were scored in every world, so
/// the best and the default compare over the same worlds.
pub(super) fn best(
    roots: &[Choice],
    rewards: &[Vec<f64>],
    means: &[Option<f64>],
    default: Choice,
) -> Choice {
    let raced = rewards[usize::from(default.0)].len();
    roots
        .iter()
        .copied()
        .filter(|c| means[usize::from(c.0)].is_some() && rewards[usize::from(c.0)].len() == raced)
        .reduce(|a, b| {
            if means[usize::from(a.0)] >= means[usize::from(b.0)] {
                a
            } else {
                b
            }
        })
        .unwrap_or(default)
}

/// Drops from `racing` each root that the leader, the root with the highest mean reward,
/// beats by more than [`RACE_ERRORS`] standard errors of their paired difference. The leader
/// and `default` stay. Returns how many roots were dropped.
///
/// Every root in `racing` was scored in every world so far with that world's seed, so the
/// rewards pair up world by world.
pub(super) fn race_out(racing: &mut Vec<Choice>, rewards: &[Vec<f64>], default: Choice) -> u64 {
    let scores = |c: Choice| &rewards[usize::from(c.0)];
    let mean = |c: Choice| scores(c).iter().sum::<f64>() / scores(c).len() as f64;
    let Some(leader) = racing
        .iter()
        .copied()
        .reduce(|a, b| if mean(a) >= mean(b) { a } else { b })
    else {
        return 0;
    };
    let before = racing.len();
    racing.retain(|&c| {
        if c == leader || c == default {
            return true;
        }
        let leads: Vec<f64> = scores(leader)
            .iter()
            .zip(scores(c))
            .map(|(lead, other)| lead - other)
            .collect();
        let (lead, error) = mean_and_error(&leads);
        lead <= RACE_ERRORS * error
    });
    (before - racing.len()) as u64
}
