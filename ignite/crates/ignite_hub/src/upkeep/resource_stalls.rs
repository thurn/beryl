//! Escalate sustained symptoms of stalled dispatch, disk cleanup, or start pacing.

use ignite_core::capacity::BYTES_PER_GIB;
use ignite_core::config::Os;
use ignite_core::protocol::machine_state::{MachineSnapshot, StartBlock};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{fleet, machine_snapshots, rows};
use rusqlite::Connection;

use super::clearing::HOLD_SECONDS;
use super::filing::Filed;
use super::justiciar;

const IDLE_SECONDS: f64 = 10.0 * 60.0;
const DISK_SECONDS: f64 = 15.0 * 60.0;
const PRESSURE_SECONDS: f64 = 30.0 * 60.0;

/// A minute is expected between samples. Allow heartbeat jitter and one missed sample.
const SAMPLE_GAP_SECONDS: f64 = 120.0;
/// Older daemons measured reclaimable disk hourly. Allow one late measurement.
const RECLAIMABLE_MAX_AGE_SECONDS: f64 = 2.0 * 3_600.0;
/// Allow modest swap use and queues smaller than one host's worth of cores.
const SWAP_LIMIT_MB: u64 = 4_096;
/// With slot-based admission there is no byte target. Watch for cleanup that leaves
/// the worktree volume critically low while work is waiting.
const LOW_DISK_GIB: u64 = 10;
/// Comment at most once for each quarter hour of new evidence.
const SIGHTING_MILLIS: i64 = 15 * 60 * 1_000;

#[derive(Clone, Copy)]
pub(super) enum Invariant {
    Idle,
    Disk,
    Pressure,
}

impl Invariant {
    fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle-with-work",
            Self::Disk => "disk-handling-stalled",
            Self::Pressure => "oversubscription-persisting",
        }
    }

    pub(super) fn from_subject(subject: &str) -> Option<(Self, &str)> {
        let (name, machine) = subject.strip_prefix("resource:")?.split_once('@')?;
        let invariant = match name {
            "idle-with-work" => Self::Idle,
            "disk-handling-stalled" => Self::Disk,
            "oversubscription-persisting" => Self::Pressure,
            _ => return None,
        };
        Some((invariant, machine))
    }

    fn title(self) -> &'static str {
        match self {
            Self::Idle => "Workers idle with work",
            Self::Disk => "Disk handling stalled",
            Self::Pressure => "Oversubscription persisting",
        }
    }

    fn seconds(self) -> f64 {
        match self {
            Self::Idle => IDLE_SECONDS,
            Self::Disk => DISK_SECONDS,
            Self::Pressure => PRESSURE_SECONDS,
        }
    }

    fn handler(self) -> &'static str {
        match self {
            Self::Idle => "The dispatch loop should keep ready work moving when workers are idle.",
            Self::Disk => {
                "Build-output cleanup and disk admission should restore enough disk for starts."
            }
            Self::Pressure => {
                "Start pacing and the scheduler should give sustained pressure time to drain."
            }
        }
    }
}

pub(super) fn file(conn: &mut Connection, now: Timestamp) -> Result<Vec<Filed>> {
    let mut filed = Vec::new();
    for machine in fleet::machines(conn)? {
        let samples = samples(conn, &machine.id, now)?;
        let Some(latest) = fresh_latest(&samples, now) else {
            continue;
        };

        let disk_short = tail_since(&samples, disk_short);
        let reclaimable = tail_since(&samples, enough_to_reclaim);
        let cleanup = disk_short.and_then(|since| {
            latest
                .last_disk_cleanup_at
                .filter(|&at| since <= at && at <= latest.at)
        });

        // Give actionable disk stalls to the disk handler. Without disk evidence, idle
        // workers still report a dispatch stall even when disk is the recorded blocker.
        let idle = if reclaimable.is_some() || cleanup.is_some() {
            None
        } else {
            tail_since(&samples, |sample| {
                dispatch_observable(sample) && idle_with_work(sample)
            })
        };
        let disk = if cleanup.is_some() {
            disk_short
        } else {
            reclaimable
        };
        let pressure = tail_since(&samples, |sample| over_limit(sample, &machine));
        for (invariant, since) in [
            (Invariant::Idle, idle),
            (Invariant::Disk, disk),
            (Invariant::Pressure, pressure),
        ] {
            if let Some(since) =
                since.filter(|since| latest.at.seconds_since(*since) >= invariant.seconds())
            {
                filed.push(report(
                    conn, &machine, invariant, since, latest, cleanup, now,
                )?);
            }
        }
    }
    Ok(filed)
}

pub(super) fn recovered(
    conn: &Connection,
    machine: &rows::Machine,
    invariant: Invariant,
    now: Timestamp,
) -> Result<bool> {
    let samples = samples(conn, &machine.id, now)?;
    let Some(latest) = fresh_latest(&samples, now) else {
        return Ok(false);
    };
    let healthy = |sample: &MachineSnapshot| match invariant {
        Invariant::Idle => dispatch_observable(sample) && !idle_with_work(sample),
        Invariant::Disk => {
            dispatch_observable(sample)
                && !disk_blocked(sample)
                && (sample.ready_tickets == 0
                    || sample.free_disk_gib.is_some_and(|free| {
                        sample
                            .next_ticket_disk_gib
                            .map_or(free > LOW_DISK_GIB, |needed| free >= needed)
                    }))
        }
        Invariant::Pressure => {
            !intentionally_stopped(sample)
                && sample.queued_cores <= cores(machine)
                // Windows daemons do not measure swap. Where it is supported, a
                // missing reading cannot confirm that swap has returned to normal.
                && sample.swap_used_mb.map_or_else(
                    || machine.os == Os::Windows.as_str(),
                    |swap| swap <= SWAP_LIMIT_MB,
                )
        }
    };
    Ok(tail_since(&samples, healthy)
        .is_some_and(|since| latest.at.seconds_since(since) >= HOLD_SECONDS))
}

fn samples(conn: &Connection, machine: &str, now: Timestamp) -> Result<Vec<MachineSnapshot>> {
    machine_snapshots::between(
        conn,
        machine,
        now.plus_seconds(-HOLD_SECONDS - 2.0 * SAMPLE_GAP_SECONDS),
        now,
    )
}

fn fresh_latest(samples: &[MachineSnapshot], now: Timestamp) -> Option<&MachineSnapshot> {
    samples
        .last()
        .filter(|sample| now.seconds_since(sample.at) <= SAMPLE_GAP_SECONDS)
}

/// The first sample in the uninterrupted run at the end of the history.
fn tail_since(
    samples: &[MachineSnapshot],
    matches: impl Fn(&MachineSnapshot) -> bool,
) -> Option<Timestamp> {
    let mut since = samples.last().filter(|sample| matches(sample))?.at;
    for sample in samples.iter().rev().skip(1) {
        if since.seconds_since(sample.at) > SAMPLE_GAP_SECONDS || !matches(sample) {
            break;
        }
        since = sample.at;
    }
    Some(since)
}

fn intentionally_stopped(sample: &MachineSnapshot) -> bool {
    matches!(
        sample.start_block,
        Some(StartBlock::Emergency | StartBlock::Paused | StartBlock::Draining)
    )
}

fn dispatch_observable(sample: &MachineSnapshot) -> bool {
    // A justiciar bypasses admission limits and says little about whether ordinary
    // work can start. Older hubs recorded its disk target as zero.
    let justiciar_next = sample
        .next_ticket
        .as_ref()
        .is_some_and(|id| id.as_str().starts_with('J') || sample.next_ticket_disk_gib == Some(0));
    !(intentionally_stopped(sample) || justiciar_next)
}

fn idle_with_work(sample: &MachineSnapshot) -> bool {
    sample.idle_workers > 0 && sample.ready_tickets > 0
}

fn disk_blocked(sample: &MachineSnapshot) -> bool {
    sample.start_block == Some(StartBlock::Disk)
}

fn enough_to_reclaim(sample: &MachineSnapshot) -> bool {
    if !disk_blocked(sample) {
        return false;
    }
    let Some((free, needed)) = sample.free_disk_gib.zip(sample.next_ticket_disk_gib) else {
        return false;
    };
    sample.reclaimable.is_some_and(|disk| {
        let age = sample.at.seconds_since(disk.measured_at);
        (0.0..=RECLAIMABLE_MAX_AGE_SECONDS).contains(&age)
            && free < needed
            && disk.bytes >= needed.saturating_sub(free).saturating_mul(BYTES_PER_GIB)
    })
}

fn disk_short(sample: &MachineSnapshot) -> bool {
    dispatch_observable(sample)
        && sample.ready_tickets > 0
        && sample.free_disk_gib.is_some_and(|free| {
            sample
                .next_ticket_disk_gib
                .map_or(free <= LOW_DISK_GIB, |needed| {
                    disk_blocked(sample) && free < needed
                })
        })
}

fn cores(machine: &rows::Machine) -> u64 {
    u64::try_from(machine.cores).unwrap_or_default().max(1)
}

fn over_limit(sample: &MachineSnapshot, machine: &rows::Machine) -> bool {
    !intentionally_stopped(sample)
        && (sample.queued_cores > cores(machine)
            || sample.swap_used_mb.is_some_and(|swap| swap > SWAP_LIMIT_MB))
}

fn report(
    conn: &mut Connection,
    machine: &rows::Machine,
    invariant: Invariant,
    since: Timestamp,
    sample: &MachineSnapshot,
    cleanup: Option<Timestamp>,
    now: Timestamp,
) -> Result<Filed> {
    use std::fmt::Write as _;

    let minutes = sample.at.seconds_since(since) / 60.0;
    let condition = match invariant {
        Invariant::Idle => "Idle workers and eligible tickets were both present".to_owned(),
        Invariant::Disk => match (cleanup, sample.next_ticket_disk_gib) {
            (Some(_), Some(_)) => "Disk blocked starts".to_owned(),
            (Some(_), None) => {
                format!("Work was waiting and free disk stayed at or below {LOW_DISK_GIB} GiB")
            }
            (None, _) => "Disk blocked starts while enough disk could be reclaimed".to_owned(),
        },
        Invariant::Pressure => format!(
            "Queued cores exceeded the host's {} cores or swap exceeded {SWAP_LIMIT_MB} MB",
            cores(machine)
        ),
    };
    let mut body = format!(
        "{condition} on {} for {minutes:.0} minutes. The latest snapshot was taken at {}.\n\n",
        machine.name, sample.at
    );
    if matches!(invariant, Invariant::Disk)
        && let Some(at) = cleanup
    {
        let _ = writeln!(
            body,
            "Free space was still below the required level after cleanup at {at}.\n"
        );
    }
    for (label, value) in sample.readings() {
        let _ = writeln!(body, "- {label}: {value}");
    }
    let _ = write!(
        body,
        "\n{} Read `ignite guide justiciar` first, then identify and record the root cause of          the handler's failure as well as restoring service.",
        invariant.handler()
    );
    justiciar::file_resource_stall(
        conn,
        &format!("resource:{}@{}", invariant.name(), machine.id),
        sample.at.millis().div_euclid(SIGHTING_MILLIS).to_string(),
        format!("{} on {}", invariant.title(), machine.name),
        body,
        now,
    )
}
