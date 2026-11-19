//! What the ledger reads about the machine: its CPU and memory, each grant's use, the daemon's
//! whole process tree and swap. Every reading is passed in, so tests set them.

use ignite_core::time::Timestamp;

use super::{Ledger, Origin};

impl Ledger {
    /// Records a reading of the machine: the fraction of all its cores busy, from 0 to 1, and
    /// the free memory, each if it could be read. Whatever is busy beyond what Ignite uses
    /// belongs to processes outside Ignite, and is not granted until they finish.
    ///
    /// The fraction covers every logical core, the express lane's too, so it is read against
    /// [`super::Limits::machine_cores`]. Read against the cores handed out, a fully busy
    /// 18-core machine with 3 express cores read as 15 cores busy.
    ///
    /// Ignite's use is the larger of what its grants' commands used and what the daemon's whole
    /// process tree used ([`Self::observe_tree`]), plus the grants a previous daemon started,
    /// whose commands are not in this daemon's tree. A grant whose command's use has been read
    /// ([`Self::observe_use`]) counts what it used over the last reading; one not yet read
    /// counts all its cores. Counting only the grants made express runs, verb processes and the
    /// daemon itself read as outside load: on 2026-11-18 the scheduler reported 8 to 11 cores
    /// busy outside Ignite while processes outside it used 3.5 to 4. Read the grants' and the
    /// tree's use before calling this, so the readings cover the same second.
    pub fn observe(&mut self, busy: Option<f64>, free_memory_mb: Option<u64>) {
        if let Some(busy) = busy {
            let busy_cores = busy.clamp(0.0, 1.0) * f64::from(self.limits.machine_cores());
            let counted = |adopted_only: bool| -> f64 {
                self.held
                    .values()
                    .filter(|h| !adopted_only || h.origin == Origin::Adopted)
                    .map(|h| h.usage.latest().unwrap_or(f64::from(h.cores)))
                    .sum()
            };
            let grants = counted(false);
            let ours = self
                .tree
                .latest()
                .map_or(grants, |tree| (tree + counted(true)).max(grants));
            self.outside = (busy_cores - ours).max(0.0).round() as u32;
        }
        if free_memory_mb.is_some() {
            self.free_memory_mb = free_memory_mb;
            for held in self.held.values_mut() {
                held.unseen_memory = false;
            }
        }
    }

    /// Records that grant `id`'s command and every process it started have used `cpu_s`
    /// seconds of CPU so far, read at `now`. Unknown IDs are ignored: the grant was released
    /// while its use was read.
    pub fn observe_use(&mut self, id: u64, now: Timestamp, cpu_s: f64) {
        if let Some(held) = self.held.get_mut(&id) {
            held.usage.record(now, cpu_s);
        }
    }

    /// Records the CPU seconds the daemon and every process it started have used so far, read
    /// at `now`, or `None` when they could not be read. A failed reading forgets the earlier
    /// ones, so a rate from minutes ago is never taken for the current one, and [`Self::observe`]
    /// counts only the grants until two readings follow each other again.
    pub fn observe_tree(&mut self, now: Timestamp, cpu_s: Option<f64>) {
        match cpu_s {
            Some(cpu_s) => self.tree.record(now, cpu_s),
            None => self.tree.reset(),
        }
    }

    /// Records the swap in use at `now`. While swap grows fast, or free memory runs out, no
    /// new work is admitted ([`super::Pressure`]).
    pub fn observe_swap(&mut self, now: Timestamp, used_mb: u64) {
        self.pressure.record_swap(now, used_mb);
    }
}
