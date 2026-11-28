//! Which machine runs a regression's culprit search.
//!
//! Seen on 2026-11-27: while a regression held `master` back, every rerun of its failing check
//! ran on multivac, the slow machine that first saw the failure, rather than on jarvis, which
//! is much faster. The search ran inside the daemon that saw the failure, and no machine handed
//! its probes to another.
//!
//! Now the machine that saw the failure keeps the search only when its speed is at least
//! [`machine_speed::AFFINITY_SPEED_FRACTION`] of the fastest free machine with the same operating system that
//! runs the check's family (the rule `machine_speed::preferred` applies to repair tickets). A
//! slower machine reports the failure without searching, and the pending search goes to a
//! pool, which the fastest free such machine claims ([`takes`]). A claim lasts [`CLAIM_S`]; a
//! search whose claim expired before its report returns to the pool. A search nobody claims
//! for [`PATIENCE_S`] goes to whichever such machine asks, in case the fastest never asks or is
//! busy with another search.
//!
//! A machine is free when the operator has not switched it off and its latest capacity report
//! is recent and not under memory pressure.

use rusqlite::Connection;

use ignite_core::config::Os;
use ignite_core::fleet::MachineState;
use ignite_core::machine_speed::{self, Candidate, Speed};
use ignite_core::protocol::hub::CulpritSearcher;
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{capacity, fleet, fleet_settings, machine_speeds};

/// How long a claimed search stays with its claimant without a report. A bounded search runs a
/// check at most ten times, and one run of the slowest checks can take ten minutes.
pub const CLAIM_S: f64 = 2.0 * 3600.0;

/// How long a pooled search waits for the fastest machine. Every daemon asks for a search every
/// 30 seconds while it runs none, so the fastest asks within a minute unless it is busy with
/// another search, which can take an hour.
pub const PATIENCE_S: f64 = 600.0;

/// A registered machine that could run the search.
struct Machine {
    id: String,
    name: String,
    speed: Speed,
}

/// The machines with `os` whose families include `family` and that are free at `now`, plus
/// `asker` when it has `os`, which is asking and so is running.
fn eligible(
    conn: &Connection,
    asker: &str,
    os: Os,
    family: &str,
    now: Timestamp,
) -> Result<Vec<Machine>> {
    let settings = fleet_settings::settings(conn)?;
    let speeds = machine_speeds::speeds(conn, &settings)?;
    let reports = capacity::latest(conn, now)?;
    let mut found = Vec::new();
    for machine in fleet::machines(conn)? {
        let (machine_os, families) =
            fleet::parse_dispatch_capabilities(&machine.os, &machine.families)?;
        if machine_os != os || !(families.is_empty() || families.iter().any(|f| f == family)) {
            continue;
        }
        let free = settings.machine(&machine.name).state != MachineState::Off
            && machine
                .id
                .parse()
                .ok()
                .and_then(|id| reports.get(&id))
                .is_some_and(|report| !report.memory_pressed);
        if machine.id == asker || free {
            found.push(Machine {
                speed: machine_speeds::of(&speeds, &machine.name),
                id: machine.id,
                name: machine.name,
            });
        }
    }
    Ok(found)
}

fn candidates(machines: &[Machine]) -> Vec<Candidate<'_>> {
    machines
        .iter()
        .map(|machine| Candidate {
            name: &machine.name,
            speed: machine.speed,
        })
        .collect()
}

/// The operating system of the registered machine `id`.
pub(super) fn os_of(conn: &Connection, id: &str) -> Result<Option<Os>> {
    for machine in fleet::machines(conn)? {
        if machine.id == id {
            return Ok(Some(
                fleet::parse_dispatch_capabilities(&machine.os, "[]")?.0,
            ));
        }
    }
    Ok(None)
}

/// The faster machine that should run the culprit search for a check of `family` that failed
/// on `reporter`, or `None` when `reporter` is nearly as fast as the fastest and searches
/// itself.
pub fn placement(
    conn: &Connection,
    reporter: &str,
    family: &str,
    now: Timestamp,
) -> Result<Option<CulpritSearcher>> {
    let Some(os) = os_of(conn, reporter)? else {
        return Ok(None);
    };
    let machines = eligible(conn, reporter, os, family, now)?;
    let Some(here) = machines.iter().find(|machine| machine.id == reporter) else {
        return Ok(None);
    };
    let candidates = candidates(&machines);
    // The reporter is the affinity: it keeps the search when nearly as fast.
    let Some(best) = machine_speed::preferred(&candidates, &here.name, Some(&here.name)) else {
        return Ok(None);
    };
    if best.name == here.name {
        return Ok(None);
    }
    Ok(Some(CulpritSearcher {
        machine: best.name.to_owned(),
        speed: best.speed,
        here: here.speed,
    }))
}

/// Whether `asker` takes a pooled search of a check of `family` that needs `os`, queued at
/// `queued_at`: it is the fastest free machine with `os` (ties go to the asker), or the search
/// has waited [`PATIENCE_S`] and the asker has `os` and runs the family.
pub fn takes(
    conn: &Connection,
    asker: &str,
    os: Os,
    family: &str,
    queued_at: Timestamp,
    now: Timestamp,
) -> Result<bool> {
    let machines = eligible(conn, asker, os, family, now)?;
    let Some(here) = machines.iter().find(|machine| machine.id == asker) else {
        return Ok(false);
    };
    if now.seconds_since(queued_at) >= PATIENCE_S {
        return Ok(true);
    }
    let candidates = candidates(&machines);
    Ok(machine_speed::preferred(&candidates, &here.name, None)
        .is_some_and(|best| best.name == here.name))
}
