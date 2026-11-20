//! Selection with the rollout policy's scores as priors.

use super::{Node, ROLLOUT_TEMPERATURE};

/// Chosen by comparing the tree's leads with paired rollouts.
const EXPLORATION: f64 = 0.7;

/// Pick the unvisited child with the highest prior, then use PUCT once all children have
/// a mean. In a resampled tree each child's availability counts visits to its parent
/// in worlds where the child could be played.
#[inline(never)]
pub(super) fn pick(
    nodes: &[Node],
    children: &[u32],
    score: impl Fn(usize) -> Option<i32>,
) -> usize {
    let candidates = 0..children.len();
    let top = candidates.clone().filter_map(&score).max().unwrap_or(0);
    let weights: Vec<_> = candidates
        .map(|i| {
            score(i).map(|score| ((f64::from(score) - f64::from(top)) / ROLLOUT_TEMPERATURE).exp())
        })
        .collect();
    // Each unscored action keeps its uniform share. Split the remaining mass over the
    // scored actions by softmax, giving equal scores equal weight.
    let share = 1.0 / children.len() as f64;
    let scored = weights.iter().flatten().count();
    let total: f64 = weights.iter().flatten().sum();
    let scale = scored as f64 * share / total.max(1.0);
    children
        .iter()
        .zip(weights)
        .enumerate()
        .map(|(i, (&child, weight))| {
            let n = &nodes[child as usize];
            let prior = weight.map_or(share, |weight| weight * scale);
            if n.visits == 0 {
                return (i, true, prior);
            }
            let visits = f64::from(n.visits);
            let bonus = EXPLORATION * prior * f64::from(n.avail).sqrt() / (1.0 + visits);
            (i, false, n.reward / visits + bonus)
        })
        .max_by(
            |(index_a, unplayed_a, bound_a), (index_b, unplayed_b, bound_b)| {
                unplayed_a
                    .cmp(unplayed_b)
                    .then_with(|| bound_a.total_cmp(bound_b))
                    .then_with(|| index_b.cmp(index_a))
            },
        )
        .expect("a decision has a candidate")
        .0
}

#[cfg(test)]
mod tests {
    use super::*;
    use beryl_engine::Seat;

    /// Three unvisited children are visited by their prior, best score first, and over many
    /// visits the child scored far below the rest gets fewer of them than it does with no
    /// scores, where every child has the same prior.
    #[test]
    fn prior_spends_fewer_visits_on_a_low_scored_child() {
        let scores = [Some(-60), None, Some(0)];
        let visits = |prior: bool| {
            let mut nodes: Vec<Node> = (1..=3).map(|key| Node::new(key, Seat::P0)).collect();
            let children = [0, 1, 2];
            for visit in 0..60 {
                let picked = pick(&nodes, &children, |i| if prior { scores[i] } else { None });
                if prior && visit < 3 {
                    assert_eq!(picked, [2, 1, 0][visit]);
                }
                for node in &mut nodes {
                    node.avail += 1;
                }
                nodes[picked].visits += 1;
                nodes[picked].reward += 0.5;
            }
            nodes[0].visits
        };
        assert!(visits(true) < visits(false));
    }
}
