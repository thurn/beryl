//! What the ledger reads about the machine: its CPU and memory, each grant's use, the process
//! trees that count as Ignite's and swap. Every reading is passed in, so tests set them.

use ignite_core::time::Timestamp;

use super::{Ledger, Origin};

impl Ledger {
    /// Records a reading of the machine: the fraction of all its cores busy, from 0 to 1, and
    /// the free memory, each if it could be read. Whatever is busy beyond what Ignite uses
    /// belongs to processes outside Ignite, and is not granted until they finish, except what
    /// fits on the express cores the express lane leaves idle.
    ///
    /// The fraction covers every logical core, the express lane's too, so it is read against
    /// [`super::Limits::machine_cores`]. Read against the cores handed out, a fully busy
    /// 18-core machine with 3 express cores read as 15 cores busy.
    ///
    /// Ignite's use is the larger of what its grants' commands used and what Ignite's process
    /// trees used ([`Self::observe_trees`]), plus the grants a previous daemon started, whose
    /// commands are not in this daemon's tree. A grant whose command's use has been read
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
                .trees_latest()
                .map_or(grants, |trees| (trees + counted(true)).max(grants));
            // The express cores are kept out of the cores handed out, but while the express
            // lane leaves them idle, processes outside Ignite run on them first. Subtracting
            // that load from the cores handed out as well left them idle twice over: on
            // 2026-11-20 jarvis read 7 cores busy outside Ignite, granted 9 of 15, kept 20 agent
            // commands waiting and stood 43% idle. Ignite's use beyond its grants is the express
            // lane, the verb processes and the daemon, and counts against the express cores.
            let spare = (f64::from(self.limits.reserved) - (ours - grants).max(0.0)).max(0.0);
            self.outside = (busy_cores - ours - spare).max(0.0).round() as u32;
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

    /// Records the CPU seconds used so far by each tree of processes that counts as Ignite's,
    /// as `(root PID, CPU seconds)`, read at `now`, or `None` when they could not be read.
    ///
    /// The daemon's tree holds its grants' commands, the verb processes it started, the express
    /// runs and the daemon itself. Two kinds of Ignite process are outside it: the hub, which
    /// its own login service starts, and a verb process a previous daemon started, which
    /// launchd adopted when that daemon exited. On 2026-11-19 jarvis read 10 cores busy outside
    /// Ignite, two of them the hub and a verb process from an older release, while 9 agent
    /// commands waited for cores. So each such process is read as a root of its own.
    ///
    /// Each root keeps its own readings, so a root that appears, such as a hub that restarted,
    /// or disappears, such as a verb process that exited, changes no other root's rate. A root
    /// missing from the readings is forgotten. A failed reading forgets every earlier one, so a
    /// rate from minutes ago is never taken for the current one, and [`Self::observe`] counts
    /// only the grants until two readings follow each other again.
    pub fn observe_trees(&mut self, now: Timestamp, readings: Option<&[(u32, f64)]>) {
        let Some(readings) = readings else {
            self.trees.clear();
            return;
        };
        self.trees
            .retain(|root, _| readings.iter().any(|(read, _)| read == root));
        for &(root, cpu_s) in readings {
            self.trees.entry(root).or_default().record(now, cpu_s);
        }
    }

    /// The cores Ignite's process trees used between their last two readings, summed over the
    /// roots read twice, or `None` when no root has been.
    fn trees_latest(&self) -> Option<f64> {
        self.trees
            .values()
            .filter_map(super::GrantUse::latest)
            .reduce(|a, b| a + b)
    }

    /// Records the swap in use at `now`. While swap grows fast, or free memory runs out, no
    /// new work is admitted ([`super::Pressure`]).
    pub fn observe_swap(&mut self, now: Timestamp, used_mb: u64) {
        self.pressure.record_swap(now, used_mb);
    }
}
