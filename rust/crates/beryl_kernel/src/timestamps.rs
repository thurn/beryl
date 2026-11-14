//! Timestamps ([CR 613.7]): "Within a layer or sublayer, determining which order effects are
//! applied in is usually done using a timestamp system. An effect with an earlier timestamp is
//! applied before an effect with a later timestamp." A permanent takes one as it enters, turns
//! face up or becomes attached, and a resolved continuous effect as it begins.
//!
//! [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137

use crate::layout::{Layout, Queue};
use crate::state::State;

impl<L: Layout> State<L> {
    /// The next timestamp, later than every one given before. Only the order of timestamps
    /// means anything, so when the `u16` counter would run out, every live one is numbered
    /// again by its rank first ([`State::renumber_timestamps`]).
    #[inline]
    pub(crate) fn take_timestamp(&mut self) -> u16 {
        if self.zones.next_timestamp == u16::MAX {
            self.renumber_timestamps();
        }
        let timestamp = self.zones.next_timestamp;
        self.zones.next_timestamp = timestamp + 1;
        timestamp
    }

    /// Gives the permanents of `order`, which took their timestamps together, those same
    /// timestamps again in the order listed: the earliest to the first. So the rules order
    /// permanents that entered the battlefield together ([CR 613.7m]): "If two or more objects
    /// would receive a timestamp simultaneously, such as by entering a zone simultaneously or
    /// becoming attached simultaneously, their relative timestamps are determined in APNAP
    /// order (see rule 101.4)."
    ///
    /// [CR 613.7m]: https://yawgatog.com/resources/magic-rules/#R6137m
    pub fn order_timestamps(&mut self, order: &[L::Slot]) {
        debug_assert!(order.len() <= 128, "a batch moves at most 128 objects");
        let mut timestamps = [0u16; 128];
        let timestamps = &mut timestamps[..order.len()];
        for (timestamp, &slot) in timestamps.iter_mut().zip(order) {
            debug_assert_eq!(self.object(slot).zone, crate::state::Zone::Battlefield);
            *timestamp = self.object(slot).timestamp;
        }
        timestamps.sort_unstable();
        for (&timestamp, &slot) in timestamps.iter().zip(order) {
            self.object_mut(slot).timestamp = timestamp;
        }
        self.flags.derived_dirty = true;
    }

    /// Numbers the live timestamps 0, 1, 2 and on in the order they were given: those of
    /// the permanents and of the continuous effects, the ones the layers compare. An object
    /// off the battlefield keeps its stale timestamp, which nothing reads, and takes a new one
    /// as it enters. Out of line: a game comes here after some 65,000 timestamps, if ever.
    #[cold]
    #[inline(never)]
    fn renumber_timestamps(&mut self) {
        let mut live: Vec<u16> = self
            .battlefield
            .iter()
            .map(|&slot| self.object(slot).timestamp)
            .collect();
        live.extend(self.effects.timestamps_mut().map(|t| *t));
        live.sort_unstable();
        live.dedup();
        let rank = |timestamp: u16| {
            u16::try_from(live.partition_point(|&t| t < timestamp))
                .expect("fewer live timestamps than a u16 numbers")
        };
        for i in 0..self.battlefield.len() {
            let slot = self.battlefield.as_slice()[i];
            let o = self.object_mut(slot);
            o.timestamp = rank(o.timestamp);
        }
        for timestamp in self.effects.timestamps_mut() {
            *timestamp = rank(*timestamp);
        }
        self.zones.next_timestamp =
            u16::try_from(live.len()).expect("fewer live timestamps than a u16 numbers");
    }
}

#[cfg(test)]
mod tests {
    use crate::ids::{CardId, SlotId};
    use crate::layout::Small;
    use crate::profile::Profile;
    use crate::state::{Continuous, Duration, LayerChange, State};
    use crate::{CardIx, Slot};

    /// When the counter would run out, the live timestamps are numbered again from 0 in
    /// their order, and the next one given is later than all of them.
    #[test]
    fn timestamps_are_numbered_again_by_rank_before_the_counter_runs_out() {
        let mut s: State<Small> = State::empty(Profile::EXACT);
        s.zones.next_timestamp = u16::MAX - 2;
        for i in 0..4 {
            let on = s.object_ref(Slot::from_index(i));
            let effect = Continuous::Layer(on, LayerChange::ModifyPt(1, 1));
            s.start_effect(CardIx::from_index(i), effect, Duration::Indefinite)
                .unwrap();
        }
        let timestamps: Vec<u16> = s.effects().iter().map(|e| e.timestamp).collect();
        assert_eq!(timestamps, [0, 1, 2, 3]);
        assert_eq!(s.zones.next_timestamp, 4);
    }
}
