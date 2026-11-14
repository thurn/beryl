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
    use super::super::*;
    use beryl_cards::CARDS;
    use beryl_cards::decks::WU_CONTROL;
    use beryl_engine::Profile;

    #[test]
    fn prior_spends_fewer_visits_on_a_low_scored_child() {
        let table = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let mut state = State::<Small>::new(&table, 0, Profile::SMART).unwrap();
        let Step::Decide(decision) = next(&mut state, &table, &mut Scratch::new(), &mut Replay)
        else {
            panic!("the game starts with a decision");
        };
        let obs = Observation::new(&state, decision.player);
        let visits = |prior| {
            let mut family = crate::DEEP_SEARCHER;
            family.options.prior = prior;
            let mut tree = Tree::new(
                &table,
                family,
                obs,
                &decision,
                Bitboard::default(),
                false,
                0,
            );
            let keys = [1, 2, 3];
            let scores = [Some(-60), None, Some(0)];
            for visit in 0..60 {
                let (child, choice) =
                    tree.select(tree.root, &keys, decision.player, |_| scores.to_vec());
                if prior && visit < 3 {
                    assert_eq!(choice, Choice([2, 1, 0][visit]));
                }
                let node = &mut tree.nodes[child as usize];
                node.visits += 1;
                node.reward += 0.5;
            }
            tree.nodes[tree.child(tree.root, keys[0]).unwrap() as usize].visits
        };
        assert!(visits(true) < visits(false));
    }
}
