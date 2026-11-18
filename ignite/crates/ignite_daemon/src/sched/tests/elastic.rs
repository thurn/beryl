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
    let dir = std::env::temp_dir().join(format!("ignite-sched-grow-{}", std::process::id()));
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
