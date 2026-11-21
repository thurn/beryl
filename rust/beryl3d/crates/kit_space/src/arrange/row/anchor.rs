//! Places anchored groups among other groups in an ordered row.

use super::{Group, Row};

/// Order the groups and give their left edges for a row with at least one anchor.
pub(super) fn arrange(
    groups: &[Group],
    widths: &[f32],
    full: f32,
    row: Row,
) -> (Vec<usize>, Vec<f32>) {
    let face: Vec<Option<f32>> = groups
        .iter()
        .zip(widths)
        .map(|(group, width)| group.anchor.map(|centre| centre - width / 2.0))
        .collect();
    // Where each group would stand without anchors, which is where the others want to stay.
    let mut left = row.start(full);
    let mut wants = Vec::with_capacity(groups.len());
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            left += row.gap_between(groups[index - 1].gap_class, groups[index].gap_class);
        }
        wants.push(face[index].unwrap_or(left));
        left += width;
    }
    let step = |before: usize, after: usize| {
        widths[before] + row.gap_between(groups[before].gap_class, groups[after].gap_class)
    };
    let anchored = Anchored {
        wants: &wants,
        face: &face,
        widths,
        ends: (row.left, row.right),
    };
    anchored.arrange(&step)
}

/// A row's groups: the left each `wants`, the left each anchored one prefers,
/// their `widths`, and the left and right `ends` of the row's room.
struct Anchored<'a> {
    wants: &'a [f32],
    face: &'a [Option<f32>],
    widths: &'a [f32],
    ends: (f32, f32),
}

impl Anchored<'_> {
    /// The groups settled about the ones that have an anchor, in the orders they request:
    /// those stand in the order of the middles they want, and the rest in their own order
    /// between them. Where the row has no room for them all where they want to stand, the rest
    /// give way to whichever side of each anchored group leaves the anchored ones nearest their preferred left edges; between equals, each stands before the first anchored group whose middle its
    /// own is left of. It tries every such split, so its cost grows with the anchored groups'
    /// count as a power. `step` gives how far apart two groups, side
    /// by side in that order, stand.
    fn arrange(&self, step: &impl Fn(usize, usize) -> f32) -> (Vec<usize>, Vec<f32>) {
        let middle = |index: usize| self.wants[index] + self.widths[index] / 2.0;
        let mut fixed: Vec<usize> = (0..self.wants.len())
            .filter(|&index| self.face[index].is_some())
            .collect();
        fixed.sort_by(|&a, &b| middle(a).total_cmp(&middle(b)));
        let free: Vec<usize> = (0..self.wants.len())
            .filter(|&index| self.face[index].is_none())
            .collect();
        // How many of the rest stand before each anchored group, by their middles.
        let natural: Vec<usize> = fixed
            .iter()
            .map(|&f| free.iter().filter(|&&i| middle(i) < middle(f)).count())
            .collect();
        let mut best: Option<Best> = None;
        let mut before = Vec::with_capacity(fixed.len());
        self.visit((&fixed, &free, &natural), &mut before, step, &mut best);
        let best = best.expect("every row has at least one split");
        (best.order, best.lefts)
    }

    /// Tries each way of splitting the rest about the anchored groups that begins with
    /// `before`, where `before[j]` of the rest stand before `fixed[j]`, keeping the best in
    /// `best`.
    fn visit(
        &self,
        (fixed, free, natural): (&[usize], &[usize], &[usize]),
        before: &mut Vec<usize>,
        step: &impl Fn(usize, usize) -> f32,
        best: &mut Option<Best>,
    ) {
        if before.len() < fixed.len() {
            for count in before.last().copied().unwrap_or(0)..=free.len() {
                before.push(count);
                self.visit((fixed, free, natural), before, step, best);
                before.pop();
            }
            return;
        }
        let mut order = Vec::with_capacity(self.wants.len());
        let mut start = 0;
        for (&f, &end) in fixed.iter().zip(before.iter()) {
            order.extend_from_slice(&free[start..end]);
            order.push(f);
            start = end;
        }
        order.extend_from_slice(&free[start..]);
        let lefts = self.settle(&order, step);
        // How far the anchored groups stand from where they want, to a tenth of a millimetre so
        // that splits that tie in effect tie in number.
        let off: f32 = order
            .iter()
            .zip(&lefts)
            .filter_map(|(&index, &left)| self.face[index].map(|want| (left - want).abs()))
            .sum();
        let off = (off * 10.0).round();
        let moved: usize = before
            .iter()
            .zip(natural)
            .map(|(&count, &natural)| count.abs_diff(natural))
            .sum();
        let better = best.as_ref().is_none_or(|best| {
            off < best.off || (off.total_cmp(&best.off).is_eq() && moved < best.moved)
        });
        if better {
            *best = Some(Best {
                order,
                lefts,
                off,
                moved,
            });
        }
    }

    /// The groups in `order` moved as near as they can be to the lefts they want, `step` apart
    /// at least and inside the row's ends: each group without an anchor first gives way leftward to its right neighbour, then each is pushed right of
    /// its left neighbour, then left of the end and its right neighbour, then right of the start
    /// again.
    fn settle(&self, order: &[usize], step: &impl Fn(usize, usize) -> f32) -> Vec<f32> {
        let (start, end) = self.ends;
        let count = order.len();
        let steps: Vec<f32> = order
            .windows(2)
            .map(|pair| step(pair[0], pair[1]))
            .collect();
        let mut lefts: Vec<f32> = order.iter().map(|&index| self.wants[index]).collect();
        for k in (0..count.saturating_sub(1)).rev() {
            if self.face[order[k]].is_none() {
                lefts[k] = lefts[k].min(lefts[k + 1] - steps[k]);
            }
        }
        let from_left = |lefts: &mut [f32]| {
            for k in 0..count {
                let least = if k == 0 {
                    start
                } else {
                    lefts[k - 1] + steps[k - 1]
                };
                lefts[k] = lefts[k].max(least);
            }
        };
        from_left(&mut lefts);
        for k in (0..count).rev() {
            let most = if k == count - 1 {
                end - self.widths[order[k]]
            } else {
                lefts[k + 1] - steps[k]
            };
            lefts[k] = lefts[k].min(most);
        }
        from_left(&mut lefts);
        lefts
    }
}

/// The best split found so far: its order and lefts, how far its anchored groups stand from where
/// they want (`off`), and how far it moves the rest from their natural sides (`moved`).
struct Best {
    order: Vec<usize>,
    lefts: Vec<f32>,
    off: f32,
    moved: usize,
}
