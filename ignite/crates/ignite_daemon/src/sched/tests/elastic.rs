//! Grants that shrink and grow while their commands run.

use std::fs;
use std::path::PathBuf;

use ignite_core::protocol::Class;

use super::{T0, ask, at, limits, scheduler};
use crate::sched::{GrantRequest, Ledger, RECLAIM_SECONDS};

/// A Cargo build that only compiles.
fn compile(class: Class, cores: u32) -> GrantRequest {
    GrantRequest {
        shrinks: true,
        ..ask(class, cores)
    }
}

/// A command that rereads its cores file.
fn growing(class: Class, cores: u32, file: PathBuf) -> GrantRequest {
    GrantRequest {
        grows: true,
        cores_file: Some(file),
        ..ask(class, cores)
    }
}

/// A command that only follows its grant through the jobserver, such as a script that runs
/// Cargo and then a program, or a test build that also runs the tests.
fn under_jobserver(class: Class, cores: u32, file: PathBuf) -> GrantRequest {
    GrantRequest {
        cores_file: Some(file),
        ..ask(class, cores)
    }
}

fn agent_cores(ledger: &Ledger) -> u32 {
    ledger.status().classes[1].cores
}

#[test]
fn a_compiling_build_gives_its_cores_back_one_at_a_time_while_land_waits() {
    let mut ledger = Ledger::new(limits());
    let build = ledger.enqueue(compile(Class::Agent, 12), T0);
    ledger.enqueue(ask(Class::Agent, 12), T0);
    assert_eq!(ledger.step(T0).granted.len(), 2);
    assert_eq!(agent_cores(&ledger), 24);

    let land = ledger.enqueue(ask(Class::Land, 16), at(1.0));
    let mut sizes = Vec::new();
    let mut now = 1.0;
    let grant = loop {
        let step = ledger.step(at(now));
        if let Some(grant) = step.granted.first() {
            assert!(step.resized.is_empty());
            break grant.clone();
        }
        // One core per step, and only from the build.
        assert_eq!(step.resized.len(), 1);
        assert_eq!(step.resized[0].0, build);
        sizes.push(step.resized[0].1);
        now += 0.25;
    };
    assert_eq!(sizes, (1..=11).rev().collect::<Vec<u32>>());
    assert_eq!(grant.id, land);
    assert_eq!(grant.cores, 11);

    // The build keeps its last core and takes none back while the landing runs.
    for later in 1..5 {
        let step = ledger.step(at(now + f64::from(later)));
        assert!(step.resized.is_empty());
    }
    assert_eq!(agent_cores(&ledger), 13);
}

#[test]
fn a_short_request_takes_what_is_free_once_the_reclaim_wait_ends() {
    let mut ledger = Ledger::new(limits());
    ledger.enqueue(compile(Class::Agent, 12), T0);
    ledger.enqueue(ask(Class::Agent, 12), T0);
    ledger.step(T0);
    let land = ledger.enqueue(ask(Class::Land, 16), T0);
    ledger.step(T0);
    // One core is free and ten more could still come back, but the request has waited long
    // enough: it starts on the one.
    let step = ledger.step(at(RECLAIM_SECONDS));
    assert_eq!(step.granted.len(), 1);
    assert_eq!(step.granted[0].id, land);
    assert_eq!(step.granted[0].cores, 1);
}

#[test]
fn a_short_land_grant_receives_freed_cores_before_an_agent_request() {
    let mut ledger = Ledger::new(limits());
    let holder = ledger.enqueue(ask(Class::Agent, 16), T0);
    ledger.step(T0);
    let land = ledger.enqueue(growing(Class::Land, 16, "/runs/land/cores".into()), T0);
    let step = ledger.step(T0);
    assert_eq!(step.granted[0].cores, 8);

    // The landing is short, so it counts as a waiter and the agent starts nothing.
    let agent = ledger.enqueue(ask(Class::Agent, 16), at(1.0));
    assert!(ledger.step(at(1.0)).granted.is_empty());

    ledger.release(holder);
    let step = ledger.step(at(2.0));
    assert_eq!(step.resized, [(land, 16)]);
    assert_eq!(step.granted.len(), 1);
    assert_eq!(step.granted[0].id, agent);
    assert_eq!(step.granted[0].cores, 8);
}

#[test]
fn a_growing_commands_file_shows_its_grant_rise_as_another_command_ends() {
    let dir = ignite_core::test_files::temp_dir().join(format!("ignite-sched-grow-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("cores");
    let read = || fs::read_to_string(&file).unwrap().trim().to_owned();

    let (scheduler, _, _) = scheduler();
    let other = scheduler.acquire(&ask(Class::Agent, 16));
    let grant = scheduler.acquire(&growing(Class::Agent, 16, file.clone()));
    assert_eq!(grant.cores, 8);
    assert_eq!(read(), "8");
    assert!(
        grant
            .env()
            .contains(&("IGNITE_CORES_FILE".to_owned(), file.display().to_string()))
    );

    scheduler.release(&other);
    assert_eq!(read(), "16");
    scheduler.release(&grant);
    fs::remove_dir_all(&dir).unwrap();
}

/// A build started on one core while the machine was busy stayed on one core: only a command
/// that reread its cores file grew, and a script that runs Cargo does not. A grant with a cores
/// file now grows for its jobserver, but it still waits for shrinking grants before it starts
/// short, because what it runs besides Cargo keeps the thread count it started with.
#[test]
fn a_grant_followed_through_the_jobserver_grows_and_still_waits_to_reclaim() {
    let mut ledger = Ledger::new(limits());
    let holder = ledger.enqueue(ask(Class::Agent, 16), T0);
    ledger.enqueue(ask(Class::Agent, 7), T0);
    assert_eq!(ledger.step(T0).granted.len(), 2);
    let script = ledger.enqueue(
        under_jobserver(Class::Agent, 12, "/runs/s/cores".into()),
        T0,
    );
    let step = ledger.step(T0);
    assert_eq!(step.granted[0].id, script);
    assert_eq!(step.granted[0].cores, 1);

    ledger.release(holder);
    let step = ledger.step(at(1.0));
    assert_eq!(step.resized, [(script, 12)]);

    // With a shrinking build of a lower class holding cores, a landing under the jobserver
    // waits for them instead of starting on the one core that is free.
    let mut ledger = Ledger::new(limits());
    ledger.enqueue(compile(Class::Agent, 12), T0);
    ledger.enqueue(ask(Class::Agent, 11), T0);
    ledger.step(T0);
    ledger.enqueue(under_jobserver(Class::Land, 4, "/runs/l/cores".into()), T0);
    assert!(ledger.step(T0).granted.is_empty());
}

#[test]
fn a_compiling_build_gives_cores_back_to_requests_of_its_own_class() {
    let mut ledger = Ledger::new(limits());
    let build = ledger.enqueue(compile(Class::Agent, 16), T0);
    let other = ledger.enqueue(ask(Class::Agent, 8), T0);
    assert_eq!(ledger.step(T0).granted.len(), 2);
    assert_eq!(agent_cores(&ledger), 24);

    // Two small agent requests, such as listing steps: the build gives back one core per step
    // until each can start, and the other command, which does not shrink, keeps its eight.
    let late: Vec<u64> = (0..2)
        .map(|_| ledger.enqueue(ask(Class::Agent, 1), at(1.0)))
        .collect();
    let mut granted = Vec::new();
    let mut now = 1.0;
    while granted.len() < 2 {
        let step = ledger.step(at(now));
        assert!(
            step.resized
                .iter()
                .all(|(id, _)| *id == build && *id != other)
        );
        granted.extend(step.granted.iter().map(|g| g.id));
        now += 0.25;
        assert!(now < 30.0, "the late requests never started");
    }
    assert_eq!(granted, late);
}

/// Counting every waiting request of the class in the build's share shrank every build to one
/// core while eighteen requests waited. A large request of the same class waits for the build,
/// which keeps its cores, until it has waited `AGED_SECONDS`; then the build gives cores back.
#[test]
fn a_compiling_build_keeps_its_cores_from_a_large_request_until_it_has_waited_long() {
    use crate::sched::AGED_SECONDS;
    let mut ledger = Ledger::new(limits());
    let build = ledger.enqueue(compile(Class::Agent, 16), T0);
    ledger.enqueue(ask(Class::Agent, 8), T0);
    ledger.step(T0);
    ledger.enqueue(ask(Class::Agent, 8), at(1.0));
    let mut now = 1.0;
    while now < AGED_SECONDS {
        let step = ledger.step(at(now));
        assert!(
            step.resized.is_empty() && step.granted.is_empty(),
            "at {now} s"
        );
        now += 1.0;
    }
    let step = ledger.step(at(AGED_SECONDS + 1.0));
    assert_eq!(step.resized, [(build, 15)]);
}

/// After a restart the daemon took over a growing agent grant of one core, stepped, and so
/// grew it to fill cores that a ten-core command it had not yet taken over still held: 20 of
/// 18 cores were granted. Taking over every command before the next step keeps the total at
/// the machine's cores.
#[test]
fn commands_taken_over_after_a_restart_never_hold_more_than_the_machine() {
    let dir = ignite_core::test_files::temp_dir().join(format!("ignite-sched-adopt-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (scheduler, _, _) = scheduler();
    let adoption = |class, cores, follow: Option<&str>| crate::sched::Adoption {
        class,
        cores,
        resources: Vec::new(),
        memory_mb: None,
        follow: follow.map(|name| crate::sched::Follow {
            cores_file: dir.join(name),
            asked: 16,
            shrinks: false,
        }),
    };
    // 24 cores: a growing grant of one, then commands that hold the other 23.
    scheduler.adopt_all(vec![
        adoption(Class::Agent, 1, Some("growing")),
        adoption(Class::Agent, 16, None),
        adoption(Class::Land, 7, None),
    ]);
    assert_eq!(scheduler.status().granted, 24);
    assert_eq!(fs::read_to_string(dir.join("growing")).ok(), None);
    fs::remove_dir_all(&dir).unwrap();
}

/// Agent commands that grew into every free core while no landing waited kept those cores, and
/// landings ran on one core with three more waiting. What a grant grew by is given back while a
/// higher class waits, down to the size it started at.
#[test]
fn a_grant_that_grew_gives_its_growth_back_while_land_waits() {
    let mut ledger = Ledger::new(limits());
    let compare = ledger.enqueue(
        under_jobserver(Class::Agent, 16, "/runs/a/cores".into()),
        T0,
    );
    let other = ledger.enqueue(ask(Class::Agent, 12), T0);
    let step = ledger.step(T0);
    let first = |id| step.granted.iter().find(|g| g.id == id).unwrap().cores;
    assert_eq!(
        first(compare),
        12,
        "the other agent took 12 of the 24 cores first"
    );
    ledger.release(other);
    let step = ledger.step(at(1.0));
    assert_eq!(step.resized, [(compare, 16)], "grew into the free cores");

    let land = ledger.enqueue(ask(Class::Land, 12), at(2.0));
    let mut sizes = Vec::new();
    let mut now = 2.0;
    let started = loop {
        let step = ledger.step(at(now));
        sizes.extend(step.resized.iter().map(|&(_, cores)| cores));
        if let Some(grant) = step.granted.first() {
            break grant.clone();
        }
        now += 0.25;
        assert!(now < 30.0, "the landing never started");
    };
    assert_eq!(started.id, land);
    // It gave back the four it grew by, one per step, and keeps the twelve it started with.
    assert_eq!(sizes, [15, 14, 13, 12]);
    assert_eq!(started.cores, 12);
}
