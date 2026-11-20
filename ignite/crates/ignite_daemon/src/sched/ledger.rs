//! The scheduler's state machine: who holds what, who waits, and what to grant or stop next.
//!
//! It reads no clock and no operating system. The caller passes the time to [`Ledger::enqueue`]
//! and [`Ledger::step`], and the machine's CPU and memory to [`Ledger::observe`], so every rule
//! can be tested by setting those values.

use std::collections::BTreeMap;

use ignite_core::protocol::Class;
use ignite_core::time::Timestamp;

use super::measured::{GrantUse, Pressure};
use super::{Grant, GrantRequest, Limits, Status};

mod observe;
mod order;

pub use order::{AGED_SECONDS, SMALL_CORES};
use order::{in_order, share};

/// `idle` work starts only while the machine is under this share of its cores busy.
const IDLE_BUSY_LIMIT: f64 = 0.7;

/// How long a request that would start short of the cores it wants waits for lower classes'
/// shrinking grants to give theirs back, before it takes what is free. Today's token pool uses
/// the same 10 s: long enough for a Cargo build to finish the compile jobs it has started, and
/// short enough that a build with nothing to compile does not wait for nothing.
pub const RECLAIM_SECONDS: f64 = 10.0;

/// A request not granted yet.
#[derive(Clone, Debug)]
struct Waiter {
    id: u64,
    request: GrantRequest,
    since: Timestamp,
}

/// A grant not released yet.
#[derive(Clone, Debug)]
struct Held {
    class: Class,
    cores: u32,
    /// The size it was granted or adopted at. What it grew to beyond this was lent from free
    /// cores and is given back while a higher class waits.
    first: u32,
    resources: Vec<String>,
    memory_mb: Option<u64>,
    /// For a grant with a cores file, the cores it asked for, capped at two thirds of the
    /// machine, which it grows to; `None` for a grant that does not grow.
    grows_to: Option<u32>,
    shrinks: bool,
    /// Granted after the last memory reading, so that reading does not yet show its memory.
    unseen_memory: bool,
    /// Asked to stop to make room for a higher class; its cores are freed when it is released.
    stopping: bool,
    /// Whether this daemon granted it or [`Ledger::adopt`] recorded a previous daemon's grant.
    origin: Origin,
    /// The CPU its command has used, when the caller reads it ([`Ledger::observe_use`]).
    usage: GrantUse,
}

impl Held {
    /// The cores this grant counts as using when the scheduler decides what is free: all of
    /// them until its command's use has been measured long enough, then its measured use with
    /// a margin ([`GrantUse::lendable`]). A grant asked to stop counts all of them until it is
    /// released.
    fn committed(&self) -> u32 {
        if self.stopping {
            self.cores
        } else {
            self.usage.lendable(self.cores)
        }
    }
}

/// Where a held grant came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    Granted,
    Adopted,
}

/// What [`Ledger::step`] decided.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub granted: Vec<Grant>,
    /// `idle` grants whose commands must be ended. Each stays held until it is released.
    pub stop: Vec<u64>,
    /// Grants whose size changed, with the new size: shrinking grants that gave a core back
    /// and growing grants that took more.
    pub resized: Vec<(u64, u32)>,
}

#[derive(Clone, Debug)]
pub struct Ledger {
    limits: Limits,
    /// Cores busy with processes outside Ignite at the last reading.
    outside: u32,
    /// Free memory at the last reading; `None` until one succeeds.
    free_memory_mb: Option<u64>,
    /// Swap use over the last minute, to stop admitting work while the machine pages.
    pressure: Pressure,
    next_id: u64,
    /// In the order the requests were first asked for (see [`Ledger::enqueue`]).
    waiting: Vec<Waiter>,
    held: BTreeMap<u64, Held>,
    /// An update waits for the daemon's commands to finish: no `idle` work starts, and what
    /// runs is stopped.
    draining: bool,
    /// The CPU the daemon's whole process tree has used ([`Self::observe_tree`]).
    tree: GrantUse,
}

impl Ledger {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            outside: 0,
            free_memory_mb: None,
            pressure: Pressure::default(),
            next_id: 1,
            waiting: Vec::new(),
            held: BTreeMap::new(),
            draining: false,
            tree: GrantUse::default(),
        }
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Adds a request to the queue, first asked for at `since`, and returns the ID its grant
    /// will carry. The queue is ordered by when each request was first asked for, not by when
    /// it was enqueued: a verb process that asks again after the daemon restarted passes the
    /// moment it first asked (see `crate::host`), so it keeps its place ahead of requests asked
    /// for later. A request enqueued now goes behind every request already waiting.
    pub fn enqueue(&mut self, request: GrantRequest, since: Timestamp) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let at = self
            .waiting
            .iter()
            .position(|waiter| waiter.since > since)
            .unwrap_or(self.waiting.len());
        self.waiting.insert(at, Waiter { id, request, since });
        id
    }

    /// Frees a grant's cores and resources, or withdraws a request still waiting. Returns
    /// whether `id` was known.
    pub fn release(&mut self, id: u64) -> bool {
        if self.held.remove(&id).is_some() {
            return true;
        }
        let before = self.waiting.len();
        self.waiting.retain(|w| w.id != id);
        self.waiting.len() != before
    }

    pub fn holds(&self, id: u64) -> bool {
        self.held.contains_key(&id)
    }

    /// Records a grant that a previous daemon made and whose command still runs, so that its
    /// cores and resources are not handed out a second time. It holds `cores` (at least one)
    /// until released, never shrinks or grows, and returns the ID its release names. Nothing
    /// is checked against the free cores or the resource capacities: the command runs either
    /// way.
    pub fn adopt(
        &mut self,
        class: Class,
        cores: u32,
        resources: Vec<String>,
        memory_mb: Option<u64>,
        grows_to: Option<u32>,
        shrinks: bool,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.held.insert(
            id,
            Held {
                class,
                cores: cores.max(1),
                first: cores.max(1),
                resources,
                memory_mb,
                grows_to,
                shrinks,
                unseen_memory: false,
                stopping: false,
                origin: Origin::Adopted,
                usage: GrantUse::default(),
            },
        );
        id
    }

    /// While draining, `idle` requests start nothing and every `idle` grant is asked to stop,
    /// so an update that waits for the daemon's commands never waits for work that only runs
    /// on an unloaded machine.
    pub fn set_draining(&mut self, draining: bool) {
        self.draining = draining;
    }

    /// Grants what can be granted now and names the `idle` grants to stop.
    ///
    /// Classes are served in order. Within a class, the requests that can run are served in the
    /// order of [`Ledger::served_order`], each given what it asked for, capped at two thirds of
    /// the machine, before the next gets anything ([`in_order`]); then the class's short growing
    /// grants share what is left evenly. A class with a request left waiting
    /// for cores, or a short growing grant of `land`, stops every lower class from starting
    /// anything, except that `tip` and `batch` may still take their floors, and makes each
    /// shrinking grant of a lower class give back one core. A request left waiting that is
    /// small or has waited long ([`Ledger::ranks_ahead_of_grants`]) also makes each shrinking
    /// grant of its own class that holds more than its share give back one core.
    pub fn step(&mut self, now: Timestamp) -> Step {
        let mut step = Step::default();
        let mut reserve: BTreeMap<Class, u32> = Class::ALL
            .iter()
            .map(|&c| (c, self.floor_reserve(c)))
            .collect();
        let mut free = self
            .limits
            .cores
            .saturating_sub(self.outside)
            .saturating_sub(self.committed_cores());
        // While the machine pages or runs out of memory, nothing new starts and no grant grows,
        // unless Ignite holds nothing, so that a reading that never recovers cannot stop all
        // work.
        let pressed = self.pressure.pressed(self.free_memory_mb) && !self.held.is_empty();
        let mut blocked = false;
        // Cores the higher classes' blocked requests want, to decide how much `idle` to stop.
        let mut wanted_by_blocked = 0;
        // The highest class left waiting; shrinking grants of lower classes give a core back.
        let mut highest_blocked = None;
        // The highest class with a request left waiting for cores; its own shrinking grants
        // that hold more than their share give a core back too.
        let mut highest_waiting = None;
        for class in Class::ALL {
            let below: u32 = reserve
                .iter()
                .filter(|(c, _)| **c > class)
                .map(|(_, r)| r)
                .sum();
            let mut usable = if blocked {
                reserve[&class].min(free)
            } else {
                free.saturating_sub(below)
            };
            if class == Class::Idle {
                usable = if self.draining {
                    0
                } else {
                    usable.min(self.idle_room())
                };
            }
            // Outside load that fills the machine must not starve the classes whose work has
            // to make progress: each of them takes one core, beyond what is free, while it
            // holds none.
            // Only outside load counts: when Ignite's own grants fill the machine, the class
            // waits for them, and `idle` work is stopped for it.
            if usable == 0
                && self.committed_cores() < self.limits.cores
                && self.guarantees_a_core(class)
            {
                usable = 1;
            }
            if pressed {
                usable = 0;
            }
            let (picked, mut left) = self.pick(class, usable, now);
            let grants = self.fill(&picked, usable);
            let reclaimable = self.reclaimable(class);
            for (id, cores) in picked.into_iter().zip(grants) {
                // The requests ahead of it in the order took every usable core.
                if cores == 0 {
                    left.push(id);
                    continue;
                }
                if reclaimable > 0 && self.waits_to_reclaim(id, cores, now) {
                    left.push(id);
                    continue;
                }
                let waiter = self.waiting.remove(index_of(&self.waiting, id));
                free = free.saturating_sub(cores);
                usable -= cores;
                if let Some(left) = reserve.get_mut(&class) {
                    *left = left.saturating_sub(cores);
                }
                step.granted.push(self.hold(waiter, cores, now));
            }
            let room = if pressed { 0 } else { usable.min(free) };
            for (id, cores, extra) in self.grow(class, room) {
                free = free.saturating_sub(extra);
                if let Some(left) = reserve.get_mut(&class) {
                    *left = left.saturating_sub(extra);
                }
                step.resized.push((id, cores));
            }
            let short = if class == Class::Land {
                self.short_growing(class)
            } else {
                0
            };
            if !left.is_empty() {
                highest_waiting.get_or_insert(class);
            }
            if !left.is_empty() || short > 0 {
                blocked = true;
                highest_blocked.get_or_insert(class);
                if class != Class::Idle {
                    wanted_by_blocked += left.iter().map(|&w| self.want(w)).sum::<u32>() + short;
                }
            }
        }
        if let Some(class) = highest_blocked {
            step.resized.extend(self.shrink_below(class));
        }
        if let Some(class) = highest_waiting {
            step.resized.extend(self.shrink_to_share(class, now));
        }
        // When grants that lent cores use them again, the machine is over: `idle` work gives
        // them back first.
        let over = (self.outside + self.committed_cores()).saturating_sub(self.limits.cores);
        step.stop = self.stop_idle(wanted_by_blocked.saturating_sub(free).max(over));
        step.stop.extend(self.stop_idle_holding_wanted_resources());
        if self.draining {
            step.stop.extend(self.stop_all_idle());
        }
        step
    }

    /// Every grant held, as `(class, cores, adopted)`, for the log line written when the grants
    /// exceed the machine's cores.
    pub fn holders(&self) -> Vec<(Class, u32, bool)> {
        self.held
            .values()
            .map(|h| (h.class, h.cores, h.origin == Origin::Adopted))
            .collect()
    }

    /// The scheduler's state for `ignite status`.
    pub fn status(&self) -> Status {
        let classes = Class::ALL
            .iter()
            .map(|&class| super::ClassStatus {
                class,
                running: self.held.values().filter(|h| h.class == class).count() as u32,
                cores: self.class_cores(class),
                waiting: self
                    .waiting
                    .iter()
                    .filter(|w| w.request.class == class)
                    .count() as u32,
            })
            .collect();
        let resources = self
            .limits
            .resources
            .iter()
            .map(|(name, &capacity)| super::ResourceStatus {
                name: name.clone(),
                capacity,
                held: self.resource_held(name),
                waiting: self
                    .waiting
                    .iter()
                    .filter(|w| w.request.resources.contains(name))
                    .count() as u32,
            })
            .collect();
        Status {
            cores: self.limits.cores,
            outside: self.outside,
            granted: self.granted_cores(),
            committed: self.committed_cores(),
            used: self
                .held
                .values()
                .filter_map(|h| h.usage.latest())
                .sum::<f64>()
                .round() as u32,
            swap_used_mb: self.pressure.swap_used_mb(),
            memory_pressed: self.pressure.pressed(self.free_memory_mb),
            max_per_command: self.limits.max_per_command(),
            free_memory_mb: self.free_memory_mb,
            classes,
            resources,
        }
    }

    /// The waiters of `class` that can start now, by ID in the order they are served
    /// ([`Ledger::served_order`]), at most `usable` of them (each needs at least one core), and
    /// the IDs of those that could start but found no core. A request whose resources are all
    /// held, or whose memory is not free, is neither.
    fn pick(&self, class: Class, usable: u32, now: Timestamp) -> (Vec<u64>, Vec<u64>) {
        let mut resources: BTreeMap<&str, u32> = BTreeMap::new();
        let mut memory = self.free_memory_mb.map(|free| {
            let unseen: u64 = self
                .held
                .values()
                .filter(|h| h.unseen_memory)
                .filter_map(|h| h.memory_mb)
                .sum();
            free.saturating_sub(unseen)
        });
        let (mut picked, mut left) = (Vec::new(), Vec::new());
        for waiter in self.served_order(class, now) {
            let fits = waiter.request.resources.iter().all(|name| {
                let taken = self.resource_held(name) + resources.get(name.as_str()).unwrap_or(&0);
                taken < self.limits.capacity(name)
            });
            // Memory delays a command only while something else runs that could free some:
            // with nothing running, waiting would never end.
            let alone = self.held.is_empty() && picked.is_empty();
            let room = match (waiter.request.memory_mb, memory) {
                (Some(need), Some(free)) => need <= free || alone,
                _ => true,
            };
            if !fits || !room {
                continue;
            }
            if picked.len() as u32 >= usable {
                left.push(waiter.id);
                continue;
            }
            for name in &waiter.request.resources {
                *resources.entry(name).or_default() += 1;
            }
            if let (Some(need), Some(free)) = (waiter.request.memory_mb, memory.as_mut()) {
                *free = free.saturating_sub(need);
            }
            picked.push(waiter.id);
        }
        (picked, left)
    }

    /// Gives `usable` cores to the picked waiters in their order ([`in_order`]). A waiter gets
    /// none when those ahead of it took every core.
    fn fill(&self, picked: &[u64], usable: u32) -> Vec<u32> {
        let wants: Vec<u32> = picked.iter().map(|&id| self.want(id)).collect();
        in_order(&wants, usable)
    }

    /// Whether waiter `id`, which would be granted `cores`, waits instead for lower classes to
    /// give back cores that shrink: it would start short of what it wants, it has waited under
    /// [`RECLAIM_SECONDS`], and it neither shrinks nor rereads its cores file. A command that
    /// shrinks is a build that may find nothing to compile, and one that rereads its cores file
    /// starts more threads once the cores are free, so both start at once. A command that only
    /// follows its grant through the jobserver still waits: what it runs besides Cargo, such as
    /// tests, keeps the thread count it started with.
    fn waits_to_reclaim(&self, id: u64, cores: u32, now: Timestamp) -> bool {
        let waiter = &self.waiting[index_of(&self.waiting, id)];
        cores < self.want(id)
            && !waiter.request.shrinks
            && !waiter.request.grows
            && now.seconds_since(waiter.since) < RECLAIM_SECONDS
    }

    /// The cores that grants of classes below `class` could still give back ([`gives_back`]).
    fn reclaimable(&self, class: Class) -> u32 {
        self.held
            .values()
            .filter(|h| h.class > class)
            .map(gives_back)
            .sum()
    }

    /// Shares `room` cores among `class`'s growing grants that hold less than they want, evenly
    /// and earliest first ([`share`]). Returns each grant that grew with its new size and the
    /// cores it gained.
    fn grow(&mut self, class: Class, room: u32) -> Vec<(u64, u32, u32)> {
        let short: Vec<(u64, u32)> = self
            .held
            .iter()
            .filter(|(_, h)| h.class == class && !h.stopping)
            .filter_map(|(&id, h)| Some((id, h.grows_to?.checked_sub(h.cores)?)))
            .filter(|&(_, missing)| missing > 0)
            .collect();
        if short.is_empty() || room == 0 {
            return Vec::new();
        }
        let wants: Vec<u32> = short.iter().map(|&(_, missing)| missing).collect();
        let mut grown = Vec::new();
        for ((id, _), extra) in short.into_iter().zip(share(&wants, room)) {
            if extra == 0 {
                continue;
            }
            let held = self.held.get_mut(&id).expect("a short grant is held");
            held.cores += extra;
            // A grant that grew counts all its cores until its use at the new size is known.
            held.usage.reset();
            grown.push((id, held.cores, extra));
        }
        grown
    }

    /// The cores `class`'s growing grants still lack.
    fn short_growing(&self, class: Class) -> u32 {
        self.held
            .values()
            .filter(|h| h.class == class && !h.stopping)
            .filter_map(|h| Some(h.grows_to?.saturating_sub(h.cores)))
            .sum()
    }

    /// Takes one core from each shrinking grant of a class below `class` that holds more than
    /// one, and returns those grants with their new sizes. The command gives the core back when
    /// one of its jobs ends, which for a compile is within seconds, so a waiting `land` request
    /// gets a long build's cores one at a time instead of after the build.
    ///
    /// A grant that grew also gives back what it grew by, down to its first size, whatever its
    /// command. Growing agent commands (a screenshot comparison script, test builds started through
    /// `env`) took every free core while no landing waited and kept them, so landings, the
    /// highest class, ran on one core with three more waiting for half an hour.
    fn shrink_below(&mut self, class: Class) -> Vec<(u64, u32)> {
        let mut shrunk = Vec::new();
        for (&id, held) in &mut self.held {
            if held.class > class && !held.stopping && gives_back(held) > 0 {
                held.cores -= 1;
                shrunk.push((id, held.cores));
            }
        }
        shrunk
    }

    /// Takes one core from each shrinking grant of `class` that holds more than its share: the
    /// machine's cores divided among the class's grants and the waiters it gives cores back for
    /// ([`Ledger::ranks_ahead_of_grants`]), at least one. A build that started alone on twelve
    /// cores kept them while fifteen requests of its own class arrived, and those waited 150 to
    /// 165 seconds for it to finish; now the build gives a core back on each step, as it does
    /// for a higher class, until each small waiter can start. Counting every waiter instead
    /// shrank every build of the class to one core while eighteen requests waited.
    fn shrink_to_share(&mut self, class: Class, now: Timestamp) -> Vec<(u64, u32)> {
        let held = self.held.values().filter(|h| h.class == class).count();
        let waiting = self
            .waiting
            .iter()
            .filter(|w| w.request.class == class && self.ranks_ahead_of_grants(w, now))
            .count();
        if waiting == 0 {
            return Vec::new();
        }
        let share = (self.limits.cores / u32::try_from(held + waiting).unwrap_or(u32::MAX)).max(1);
        let mut shrunk = Vec::new();
        for (&id, held) in &mut self.held {
            if held.shrinks && held.class == class && !held.stopping && held.cores > share {
                held.cores -= 1;
                shrunk.push((id, held.cores));
            }
        }
        shrunk
    }

    /// The cores waiter `id` would take with nothing else waiting.
    fn want(&self, id: u64) -> u32 {
        self.want_of(&self.waiting[index_of(&self.waiting, id)])
    }

    fn hold(&mut self, waiter: Waiter, cores: u32, now: Timestamp) -> Grant {
        let queued_s = now.seconds_since(waiter.since).max(0.0) as u64;
        // A grant with a cores file grows: its command follows the file through its jobserver,
        // and a command that rereads the file starts more threads.
        let grows_to = waiter
            .request
            .cores_file
            .is_some()
            .then(|| waiter.request.cores.clamp(1, self.limits.max_per_command()));
        self.held.insert(
            waiter.id,
            Held {
                class: waiter.request.class,
                cores,
                first: cores,
                resources: waiter.request.resources.clone(),
                memory_mb: waiter.request.memory_mb,
                grows_to,
                shrinks: waiter.request.shrinks,
                unseen_memory: true,
                stopping: false,
                origin: Origin::Granted,
                usage: GrantUse::default(),
            },
        );
        Grant {
            id: waiter.id,
            cores,
            resources: waiter.request.resources,
            queued_s,
            cores_file: waiter.request.cores_file,
        }
    }

    /// Marks the newest `idle` grants as stopping until they, with those already stopping,
    /// would free `needed` cores. Returns the IDs newly marked.
    fn stop_idle(&mut self, needed: u32) -> Vec<u64> {
        let mut freeing: u32 = self
            .held
            .values()
            .filter(|h| h.stopping)
            .map(|h| h.cores)
            .sum();
        let mut stopped = Vec::new();
        for (&id, held) in self.held.iter_mut().rev() {
            if freeing >= needed {
                break;
            }
            if held.class == Class::Idle && !held.stopping {
                held.stopping = true;
                freeing += held.cores;
                stopped.push(id);
            }
        }
        stopped
    }

    /// Whether `class` always gets a core when it holds none: `land`, and a class with a floor
    /// (`tip` and `batch`).
    fn guarantees_a_core(&self, class: Class) -> bool {
        let floor = match class {
            Class::Land => return self.class_cores(class) == 0,
            Class::Tip => self.limits.floor.tip,
            Class::Batch => self.limits.floor.batch,
            _ => 0,
        };
        floor > 0 && self.class_cores(class) == 0
    }

    /// Marks the newest `idle` grants that hold a resource a higher class waits for as
    /// stopping, one for each waiter that finds the resource full, less those already
    /// stopping. Stopping for cores does not free a resource, so a request that waits only for
    /// a resource is not caught by [`Self::stop_idle`]. Returns the IDs newly marked.
    fn stop_idle_holding_wanted_resources(&mut self) -> Vec<u64> {
        let mut stopped = Vec::new();
        let mut wanted: BTreeMap<String, u32> = BTreeMap::new();
        for waiter in self
            .waiting
            .iter()
            .filter(|w| w.request.class != Class::Idle)
        {
            for name in &waiter.request.resources {
                if self.resource_held(name) >= self.limits.capacity(name) {
                    *wanted.entry(name.clone()).or_default() += 1;
                }
            }
        }
        for (name, waiters) in wanted {
            let holders: Vec<u64> = self
                .held
                .iter()
                .filter(|(_, h)| h.class == Class::Idle && h.resources.contains(&name))
                .map(|(&id, _)| id)
                .collect();
            let (stopping, running): (Vec<u64>, Vec<u64>) =
                holders.into_iter().partition(|id| self.held[id].stopping);
            let more = (waiters as usize).saturating_sub(stopping.len());
            for id in running.into_iter().rev().take(more) {
                if let Some(held) = self.held.get_mut(&id) {
                    held.stopping = true;
                    stopped.push(id);
                }
            }
        }
        stopped
    }

    /// Marks every `idle` grant not yet stopping as stopping, and returns their IDs.
    fn stop_all_idle(&mut self) -> Vec<u64> {
        let mut stopped = Vec::new();
        for (&id, held) in &mut self.held {
            if held.class == Class::Idle && !held.stopping {
                held.stopping = true;
                stopped.push(id);
            }
        }
        stopped
    }

    /// The cores held back for `class` so higher classes cannot take them: its floor less what
    /// it already holds, while it has a request waiting. A floor with nothing waiting for it is
    /// not held back.
    fn floor_reserve(&self, class: Class) -> u32 {
        let floor = match class {
            Class::Tip => self.limits.floor.tip,
            Class::Batch => self.limits.floor.batch,
            _ => return 0,
        };
        // Only a waiting request needs cores kept back for it. Counting a class's running
        // commands as well kept cores idle: one `batch` command on one core kept seven more
        // reserved for `batch` work that never came, while seven agent requests waited for
        // the six free cores for over half an hour and every landing stood still.
        if self.waiting.iter().any(|w| w.request.class == class) {
            floor.saturating_sub(self.class_cores(class))
        } else {
            0
        }
    }

    /// The cores `idle` work may take before the machine reaches the idle busy limit.
    fn idle_room(&self) -> u32 {
        let limit = (f64::from(self.limits.cores) * IDLE_BUSY_LIMIT).floor() as u32;
        limit.saturating_sub(self.outside + self.committed_cores())
    }

    fn granted_cores(&self) -> u32 {
        self.held.values().map(|h| h.cores).sum()
    }

    /// The cores held less those lent: what the grants count as using ([`Held::committed`]).
    fn committed_cores(&self) -> u32 {
        self.held.values().map(Held::committed).sum()
    }

    fn class_cores(&self, class: Class) -> u32 {
        self.held
            .values()
            .filter(|h| h.class == class)
            .map(|h| h.cores)
            .sum()
    }

    fn resource_held(&self, name: &str) -> u32 {
        self.held
            .values()
            .filter(|h| h.resources.iter().any(|r| r == name))
            .count() as u32
    }
}

fn index_of(waiting: &[Waiter], id: u64) -> usize {
    waiting
        .iter()
        .position(|w| w.id == id)
        .expect("a picked waiter is still waiting")
}

/// The cores a grant gives back while a higher class waits: all but one for a command that
/// only compiles, and otherwise what it grew by beyond its first size.
fn gives_back(held: &Held) -> u32 {
    if held.shrinks {
        held.cores.saturating_sub(1)
    } else {
        held.cores.saturating_sub(held.first)
    }
}
