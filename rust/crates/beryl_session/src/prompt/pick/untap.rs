//! Which lands an "untap up to N lands" opens with picked.

use beryl_engine::card::{AbilityKind, Purpose};
use beryl_engine::{Layout, Replay, Scratch, Seat, SlotId as _, Step as Played, apply, next};

use crate::prompt::ahead::{self, forced};
use crate::prompt::held::{Run, Step, Table, Want};
use crate::protocol::ObjectId;
use crate::protocol::Side;
use crate::script::Subject;

/// The lands among `eligible` an untap of up to `most` opens with picked: the viewer's own,
/// since the engine offers only tapped ones
/// ([CR 701.26b](https://yawgatog.com/resources/magic-rules/#R70126b): "Only tapped
/// permanents can be untapped") and each of theirs is worth untapping; an opponent's, whose
/// mana would let them answer, only by a click. With more of their own than the untap takes,
/// those that let them cast the most of the cards in their hand once the untap is done, by
/// the engine's own check of what they may cast then; then those that make the most colors
/// of mana, a dual land over a basic; then the first.
pub(super) fn opening<L: Layout>(
    table: &Table<'_, L>,
    eligible: &[Subject],
    most: usize,
) -> Vec<Subject> {
    let controls = |slot: ObjectId| {
        (table.view.objects.iter()).any(|o| o.slot == slot && o.controller == Side::You)
    };
    let mut own: Vec<ObjectId> = (eligible.iter())
        .filter_map(|on| match *on {
            Subject::Object { slot } if controls(slot) => Some(slot),
            _ => None,
        })
        .collect();
    own.sort_unstable();
    let picks = match table.d {
        Some(d) if most < own.len() => {
            let viewer = d.player;
            let score = |picks: &[ObjectId]| {
                let made = picks.iter().map(|&slot| colors(table, slot));
                (
                    castable(table, viewer, picks),
                    made.clone().map(u8::count_ones).sum::<u32>(),
                    made.fold(0, |all, kinds| all | kinds).count_ones(),
                )
            };
            // The first of the best: the choices come in order, and a later one must beat it.
            let mut best: Option<(Vec<ObjectId>, _)> = None;
            for picks in choices(&own, most) {
                let scored = score(&picks);
                if best.as_ref().is_none_or(|(_, top)| scored > *top) {
                    best = Some((picks, scored));
                }
            }
            best.map(|(picks, _)| picks).unwrap_or_default()
        }
        _ => own,
    };
    picks
        .into_iter()
        .map(|slot| Subject::Object { slot })
        .collect()
}

/// Every way of choosing `k` of `items`, in order.
fn choices(items: &[ObjectId], k: usize) -> Vec<Vec<ObjectId>> {
    if k == 0 {
        return vec![Vec::new()];
    }
    (items.iter().enumerate())
        .flat_map(|(i, &first)| {
            (choices(&items[i + 1..], k - 1).into_iter()).map(move |mut rest| {
                rest.insert(0, first);
                rest
            })
        })
        .collect()
}

/// The kinds of mana the permanent in `slot` makes, as a set: those its mana abilities name.
fn colors<L: Layout>(table: &Table<'_, L>, slot: ObjectId) -> u8 {
    let Some(slot) = L::Slot::from_num(slot.0) else {
        return 0;
    };
    let face = table.shown.copiable(slot).face(table.t);
    (face.abilities.iter())
        .filter(|a| a.kind() == AbilityKind::Mana)
        .fold(0, |kinds, a| kinds | a.produces().colors)
}

/// How many of the nonland cards in `viewer`'s hand they could pay for, at a time they may
/// cast them, once `picks` are untapped and the untap is done: the open decision answered
/// with them on a copy of the game, played on to the next decision. A spell with nothing to
/// target yet counts, as a counterspell held for the opponent's turn. Only cards in their
/// hand now count, so nothing the rest of the resolution draws says anything here.
fn castable<L: Layout>(table: &Table<'_, L>, viewer: Seat, picks: &[ObjectId]) -> usize {
    let Some(mut d) = table.d.cloned() else {
        return 0;
    };
    let t = table.t;
    let mut copy = forced(table.s);
    let mut scratch = Scratch::new();
    let mut run = Run::new(
        Purpose::Untap,
        [
            Step::Among(
                d.prompt.part,
                picks.iter().map(|&slot| Subject::Object { slot }).collect(),
            ),
            Step::One(d.prompt.part, Want::Done),
        ],
    );
    while let Some(c) = run.answer(&copy, &d) {
        apply(&mut copy, t, &mut scratch, &mut Replay, c);
        match next(&mut copy, t, &mut scratch, &mut Replay) {
            Played::Decide(asked) if asked.player == viewer => d = asked,
            _ => break,
        }
    }
    ahead::castable(table, viewer, &copy)
}
