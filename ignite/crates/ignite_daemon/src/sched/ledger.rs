//! The scheduler's state machine: who holds what, who waits, and what to grant or stop next.
//!
//! It reads no clock and no operating system. The caller passes the time to [`Ledger::enqueue`]
//! and [`Ledger::step`], and the machine's CPU and memory to [`Ledger::observe`], so every rule
//! can be tested by setting those values.

use std::collections::BTreeMap;

use ignite_core::protocol::Class;
use ignite_core::time::Timestamp;

use super::{Grant, GrantRequest, Limits, Status};

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
    next_id: u64,
    /// In arrival order.
    waiting: Vec<Waiter>,
    held: BTreeMap<u64, Held>,
    /// An update waits for the daemon's commands to finish: no `idle` work starts, and what
    /// runs is stopped.
    draining: bool,
}

impl Ledger {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            outside: 0,
            free_memory_mb: None,
            next_id: 1,
            waiting: Vec::new(),
            held: BTreeMap::new(),
            draining: false,
        }
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Adds a request to the queue and returns the ID its grant will carry.
    pub fn enqueue(&mut self, request: GrantRequest, now: Timestamp) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.waiting.push(Waiter {
            id,
            request,
            since: now,
        });
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
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.held.insert(
            id,
            Held {
                class,
                cores: cores.max(1),
                resources,
                memory_mb,
                grows_to: None,
                shrinks: false,
                unseen_memory: false,
                stopping: false,
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

    /// Records a reading of the machine: the fraction of all cores busy, from 0 to 1, and the
    /// free memory, each if it could be read. Whatever is busy beyond the cores Ignite granted
    /// belongs to processes outside Ignite, and is not granted until they finish.
    ///
    /// The ledger does not know how much of a grant its command really uses, so it counts every
    /// granted core as used by Ignite. That only matters when a command leaves cores idle
    /// (waiting for input or output): then the cores busy beyond the grants are fewer than the
    /// outside load, so `outside` is a lower bound and `status` can show less outside load than
    /// there is. What the scheduler hands out is not affected: a core is free when the machine
    /// reads idle there, or when a grant holds it, and both cases are counted exactly. A grant
    /// that sits idle is therefore never taken for outside load and starved of its own cores.
    pub fn observe(&mut self, busy: Option<f64>, free_memory_mb: Option<u64>) {
        if let Some(busy) = busy {
            let busy_cores = busy.clamp(0.0, 1.0) * f64::from(self.limits.cores);
            let ours = f64::from(self.granted_cores());
            self.outside = (busy_cores - ours).max(0.0).round() as u32;
        }
        if free_memory_mb.is_some() {
            self.free_memory_mb = free_memory_mb;
            for held in self.held.values_mut() {
                held.unseen_memory = false;
            }
        }
    }

    /// Grants what can be granted now and names the `idle` grants to stop.
    ///
    /// Classes are served in order. Within a class, the requests that can run share the free
    /// cores evenly, each capped at what it asked for and at two thirds of the machine, and then
    /// the class's short growing grants share what is left. A class with a request left waiting
    /// for cores, or a short growing grant of `land`, stops every lower class from starting
    /// anything, except that `tip` and `batch` may still take their floors, and makes each
    /// shrinking grant of a lower class give back one core.
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
            .saturating_sub(self.granted_cores());
        let mut blocked = false;
        // Cores the higher classes' blocked requests want, to decide how much `idle` to stop.
        let mut wanted_by_blocked = 0;
        // The highest class left waiting; shrinking grants of lower classes give a core back.
        let mut highest_blocked = None;
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
                && self.granted_cores() < self.limits.cores
                && self.guarantees_a_core(class)
            {
                usable = 1;
            }
            let (picked, mut left) = self.pick(class, usable);
            let grants = self.fill(&picked, usable);
            let reclaimable = self.reclaimable(class);
            for (id, cores) in picked.into_iter().zip(grants) {
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
            for (id, cores, extra) in self.grow(class, usable.min(free)) {
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
        step.stop = self.stop_idle(wanted_by_blocked.saturating_sub(free));
        step.stop.extend(self.stop_idle_holding_wanted_resources());
        if self.draining {
            step.stop.extend(self.stop_all_idle());
        }
        step
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
            max_per_command: self.limits.max_per_command(),
            free_memory_mb: self.free_memory_mb,
            classes,
            resources,
        }
    }

    /// The waiters of `class` that can start now, by ID in arrival order, at most `usable` of
    /// them (each needs at least one core), and the IDs of those that could start but found no
    /// core. A request whose resources are all held, or whose memory is not free, is neither.
    fn pick(&self, class: Class, usable: u32) -> (Vec<u64>, Vec<u64>) {
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
        for waiter in self.waiting.iter().filter(|w| w.request.class == class) {
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

    /// Shares `usable` cores among the picked waiters evenly ([`share`]). `usable` is at least
    /// the number picked, so each gets at least one.
    fn fill(&self, picked: &[u64], usable: u32) -> Vec<u32> {
        let wants: Vec<u32> = picked.iter().map(|&id| self.want(id)).collect();
        share(&wants, usable)
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

    /// The cores that shrinking grants of classes below `class` could still give back: all but
    /// one of each.
    fn reclaimable(&self, class: Class) -> u32 {
        self.held
            .values()
            .filter(|h| h.shrinks && h.class > class)
            .map(|h| h.cores - 1)
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
    fn shrink_below(&mut self, class: Class) -> Vec<(u64, u32)> {
        let mut shrunk = Vec::new();
        for (&id, held) in &mut self.held {
            if held.shrinks && held.class > class && held.cores > 1 {
                held.cores -= 1;
                shrunk.push((id, held.cores));
            }
        }
        shrunk
    }

    /// The cores waiter `id` would take with nothing else waiting.
    fn want(&self, id: u64) -> u32 {
        let waiter = &self.waiting[index_of(&self.waiting, id)];
        waiter.request.cores.clamp(1, self.limits.max_per_command())
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
                resources: waiter.request.resources.clone(),
                memory_mb: waiter.request.memory_mb,
                grows_to,
                shrinks: waiter.request.shrinks,
                unseen_memory: true,
                stopping: false,
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
    /// it already holds, while it has anything running or waiting. A floor with nothing to use
    /// it is not held back.
    fn floor_reserve(&self, class: Class) -> u32 {
        let floor = match class {
            Class::Tip => self.limits.floor.tip,
            Class::Batch => self.limits.floor.batch,
            _ => return 0,
        };
        let active = self.held.values().any(|h| h.class == class)
            || self.waiting.iter().any(|w| w.request.class == class);
        if active {
            floor.saturating_sub(self.class_cores(class))
        } else {
            0
        }
    }

    /// The cores `idle` work may take before the machine reaches the idle busy limit.
    fn idle_room(&self) -> u32 {
        let limit = (f64::from(self.limits.cores) * IDLE_BUSY_LIMIT).floor() as u32;
        limit.saturating_sub(self.outside + self.granted_cores())
    }

    fn granted_cores(&self) -> u32 {
        self.held.values().map(|h| h.cores).sum()
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

/// Shares `cores` among claims that want `wants` each, evenly. A claim that wants less than an
/// even share gets what it wants, and the rest is shared among the others; cores that do not
/// divide evenly go to the earliest claims. A claim may get none when there are more claims
/// than cores.
fn share(wants: &[u32], cores: u32) -> Vec<u32> {
    let mut grants = vec![0; wants.len()];
    let mut open: Vec<usize> = (0..wants.len()).collect();
    let mut left = cores;
    while !open.is_empty() {
        let even = left / open.len() as u32;
        let (settled, rest): (Vec<usize>, Vec<usize>) =
            open.iter().partition(|&&i| wants[i] <= even);
        if settled.is_empty() {
            let extra = (left % open.len() as u32) as usize;
            for (n, &i) in open.iter().enumerate() {
                grants[i] = even + u32::from(n < extra);
            }
            break;
        }
        for i in settled {
            grants[i] = wants[i];
            left -= wants[i];
        }
        open = rest;
    }
    grants
}

fn index_of(waiting: &[Waiter], id: u64) -> usize {
    waiting
        .iter()
        .position(|w| w.id == id)
        .expect("a picked waiter is still waiting")
}
