//! Grants that lend the cores their commands leave idle, the outside load measured against
//! what Ignite's commands use, and memory pressure.

use ignite_core::protocol::Class;

use super::{T0, ask, at, limits};
use crate::sched::{LEND_WINDOW_SECONDS, Ledger};

/// Records that grant `id`'s command has used `cores` cores on average from `from` to `to`
/// seconds after `T0`, one reading a second, starting from `cpu_s` CPU seconds. Returns the
/// CPU seconds at `to`.
fn run_at(ledger: &mut Ledger, id: u64, cores: f64, from: f64, to: f64, mut cpu_s: f64) -> f64 {
    let mut now = from;
    while now <= to {
        ledger.observe_use(id, at(now), cpu_s);
        now += 1.0;
        cpu_s += cores;
    }
    cpu_s - cores
}

/// Over an hour every core was granted while the machine stayed half idle and requests
/// waited: a `uv run` wrapper held almost five cores and used 0.14. A grant whose command uses
/// little lends the rest once it has been measured for the whole window.
#[test]
fn a_grant_that_leaves_its_cores_idle_lends_them_to_a_waiting_request() {
    let mut ledger = Ledger::new(limits());
    let wrapper = ledger.enqueue(ask(Class::Agent, 16), T0);
    let other = ledger.enqueue(ask(Class::Agent, 16), T0);
    ledger.step(T0);
    assert_eq!(ledger.status().granted, 24);

    let waiting = ledger.enqueue(ask(Class::Agent, 8), T0);
    // Measured for less than the window: nothing is lent yet.
    let window = LEND_WINDOW_SECONDS;
    let cpu = run_at(&mut ledger, wrapper, 0.1, 0.0, window - 1.0, 0.0);
    run_at(&mut ledger, other, 12.0, 0.0, window - 1.0, 0.0);
    assert!(ledger.step(at(window - 1.0)).granted.is_empty());

    // A whole window at 0.1 cores: the wrapper counts as 2 of its 12, and lends 10.
    run_at(&mut ledger, wrapper, 0.1, window, window, cpu);
    run_at(
        &mut ledger,
        other,
        12.0,
        window,
        window,
        12.0 * (window - 1.0),
    );
    let step = ledger.step(at(window));
    assert_eq!(step.granted.len(), 1);
    assert_eq!(step.granted[0].id, waiting);
    assert_eq!(step.granted[0].cores, 8);
    let status = ledger.status();
    assert_eq!((status.granted, status.committed), (32, 22));
}

/// When a grant that lent its cores uses them again, the cores count as its own within a few
/// seconds: nothing more is lent, and `idle` work is stopped to make room.
#[test]
fn a_lender_whose_use_rises_takes_its_cores_back_from_idle_work() {
    let mut ledger = Ledger::new(limits());
    let lender = ledger.enqueue(ask(Class::Agent, 16), T0);
    ledger.step(T0);
    let window = LEND_WINDOW_SECONDS;
    let cpu = run_at(&mut ledger, lender, 0.0, 0.0, window, 0.0);
    // The machine is idle beside the lender, so idle work may take up to 70% of it.
    ledger.observe(Some(0.0), None);
    let idle = ledger.enqueue(ask(Class::Idle, 16), at(window));
    let step = ledger.step(at(window));
    assert_eq!(step.granted[0].id, idle);
    // 70% of 24 is 16; the lender counts as one core, so idle work gets 15.
    assert_eq!(step.granted[0].cores, 15);

    // The lender starts using all 16 cores: within a few readings it counts all of them, the
    // machine is over, and the idle work is stopped.
    let mut now = window + 1.0;
    let mut cpu = cpu;
    let stopped = loop {
        cpu += 16.0;
        ledger.observe_use(lender, at(now), cpu);
        let step = ledger.step(at(now));
        if !step.stop.is_empty() {
            break step.stop;
        }
        now += 1.0;
        assert!(now < window + 10.0, "the idle work was never stopped");
    };
    assert_eq!(stopped, [idle]);
}

/// `ignite status` showed no load outside Ignite whenever every core was granted, though about
/// seven cores were busy outside it: every granted core was counted as Ignite's.
#[test]
fn outside_load_is_what_is_busy_beyond_what_ignites_commands_use() {
    let mut ledger = Ledger::new(limits());
    let id = ledger.enqueue(ask(Class::Agent, 16), T0);
    ledger.enqueue(ask(Class::Agent, 16), T0);
    ledger.step(T0);
    // Before any use is read, a grant counts all its cores: 18 busy is all Ignite's.
    ledger.observe(Some(0.75), None);
    assert_eq!(ledger.status().outside, 0);
    // One grant's command used 2 cores over the last second; the other is not measured and
    // counts its 12. Of 18 busy cores, 4 are outside.
    ledger.observe_use(id, at(1.0), 0.0);
    ledger.observe_use(id, at(2.0), 2.0);
    ledger.observe(Some(0.75), None);
    assert_eq!(ledger.status().outside, 4);
    assert_eq!(ledger.status().used, 2);
}

#[test]
fn nothing_new_starts_while_swap_grows() {
    let mut ledger = Ledger::new(limits());
    ledger.enqueue(ask(Class::Agent, 4), T0);
    ledger.step(T0);
    ledger.observe_swap(T0, 1_600);
    ledger.observe_swap(at(30.0), 1_900);
    assert!(ledger.status().memory_pressed);
    let waiting = ledger.enqueue(ask(Class::Land, 4), at(30.0));
    assert!(ledger.step(at(30.0)).granted.is_empty());

    // Swap stops growing; once the growth leaves the window, work starts again.
    ledger.observe_swap(at(95.0), 1_900);
    assert!(!ledger.status().memory_pressed);
    assert_eq!(ledger.step(at(95.0)).granted[0].id, waiting);
}

#[test]
fn nothing_new_starts_while_free_memory_runs_out_unless_nothing_runs() {
    let mut ledger = Ledger::new(limits());
    ledger.observe(Some(0.0), Some(500));
    let first = ledger.enqueue(ask(Class::Agent, 4), T0);
    // Nothing runs, so waiting would not free memory: it starts.
    assert_eq!(ledger.step(T0).granted[0].id, first);
    ledger.enqueue(ask(Class::Land, 4), T0);
    assert!(ledger.step(at(1.0)).granted.is_empty());
}
