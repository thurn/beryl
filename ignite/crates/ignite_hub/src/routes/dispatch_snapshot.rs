//! Observe the next hand-off without reserving a ticket or updating the dispatch queue.

use ignite_core::dispatch::{self, ReadyTicket};
use ignite_core::dispatch_pressure::{Budget, HandoffWait};
use ignite_core::fleet::MachineState;
use ignite_core::protocol::hub::{Heartbeat, MachineFree};
use ignite_core::protocol::machine_state::{MachineSample, MachineSnapshot, StartBlock};
use ignite_core::time::Timestamp;
use ignite_store::hub::{emergency, machine_snapshots};
use ignite_store::{Result, StoreError};
use rusqlite::Connection;

use super::{
    Snapshot, capacity, dispatch_groups, fleet, fleet_settings, held, history, improvements_due,
    landed_files, mix, provider_usage,
};

pub(in crate::routes::dispatch) fn record(
    conn: &mut Connection,
    beat: &Heartbeat,
    sample: &MachineSample,
    now: Timestamp,
) -> Result<()> {
    let mut snapshot = MachineSnapshot {
        at: now,
        free_disk_gib: sample.pressure.free_gb,
        reclaimable: sample.reclaimable,
        last_disk_cleanup_at: sample.last_disk_cleanup_at,
        next_ticket: None,
        next_ticket_disk_gib: None,
        start_block: None,
        start_reason: None,
        cpu_percent: sample.cpu_percent,
        queued_cores: sample.queued_cores,
        swap_used_mb: sample.swap_used_mb,
        idle_workers: sample.idle_workers,
        working_workers: 0,
        stalled_workers: 0,
        down_workers: 0,
        ready_tickets: 0,
        active_memory_pressure: sample.active_memory_pressure,
        fseventsd_rss_mib: sample.fseventsd_rss_mib,
    };
    // Keep the queue and capacity reads consistent. Release the read snapshot before writing
    // so a dispatch on another connection cannot prevent upgrading this transaction.
    let tx = conn.unchecked_transaction()?;
    project_next_start(&tx, beat, sample, &mut snapshot)?;
    tx.commit()?;
    for worker in &beat.workers {
        match worker.state.as_str() {
            "working" | "reserved" => snapshot.working_workers += 1,
            "stalled" => snapshot.stalled_workers += 1,
            "down" => snapshot.down_workers += 1,
            _ => {}
        }
    }
    machine_snapshots::record(
        conn,
        beat.machine.as_str(),
        &snapshot,
        beat.capacity.as_ref(),
    )
}

fn project_next_start(
    conn: &Connection,
    beat: &Heartbeat,
    sample: &MachineSample,
    result: &mut MachineSnapshot,
) -> Result<()> {
    let now = result.at;
    let machine = beat.machine.as_str();
    let (name, os, families) = fleet::dispatch_capabilities(conn, machine)?
        .ok_or_else(|| StoreError::Invalid(format!("unknown machine {machine}")))?;
    let settings = fleet_settings::settings(conn)?;
    let state = settings.machine(&name).state;
    let pause = pause_block(conn, state, sample, now)?;
    if let Some((block, reason)) = &pause {
        result.start_block = Some(*block);
        result.start_reason = Some(reason.clone());
    }
    if sample.room.providers.is_empty() {
        return Ok(());
    }

    let mut free = MachineFree {
        machine: beat.machine.clone(),
        os,
        idle_workers: Vec::new(),
        free_ticket_trees: sample.free_ticket_trees,
        free_cores: 0.0,
        admit_cores: Some(f64::from(sample.admit_cores)),
        pressure: None,
        families,
        name: Some(name),
        justiciar_only: false,
        master_only: false,
        explain: false,
        room: None,
    };
    let queue = Snapshot::load(conn, now)?;
    let history = history(conn, machine)?;
    let landed = landed_files(conn)?;
    let capacity = capacity::read(
        conn,
        machine,
        &history,
        &landed,
        now,
        Some(&sample.pressure),
        free.admit_cores,
    )?;
    free.free_cores = dispatch::room(&free, &capacity.cores);
    let fallback = (state == MachineState::ChecksOnly)
        .then(|| held::Fallback::load(conn, &settings, &queue.heard, &free, now))
        .transpose()?;
    let mut ready = queue.ready_tickets(conn, &history, &landed, now, fallback.as_ref())?;
    let order = dispatch_groups(&queue, &mut ready);
    let holds = provider_usage::held(conn, &settings, now)?;
    let loads = mix::loads(conn, &settings, &holds, &sample.room, now)?;
    let pending = mix::pending(conn, machine, now)?;
    let holds_one = held::holds_one(conn, machine)?;
    let candidates = dispatch::order(&ready, &order, improvements_due(conn, machine)?)
        .into_iter()
        .map(|(ticket, _)| ticket)
        .filter(|ticket| {
            dispatch::platform_fits(&free, ticket)
                && ticket
                    .pins
                    .admits_machine(free.name.as_deref().unwrap_or(machine))
                && ignite_core::mix::choose(
                    &loads,
                    &sample.room.providers,
                    dispatch::label(&ticket.labels, "tier:"),
                    ticket.pins.provider.as_deref(),
                )
                .is_ok()
        });
    let budget = &capacity.budget;
    let mut next = None;
    for ticket in candidates {
        result.ready_tickets += 1;
        if matches!(next, Some((_, None))) {
            continue;
        }
        let block = pause
            .clone()
            .or_else(|| ticket_block(&free, sample, ticket, budget, state, pending, holds_one));
        if next.is_none() || block.is_none() {
            next = Some((ticket, block));
        }
    }
    if let Some((ticket, block)) = next {
        result.next_ticket = Some(ticket.id.clone());
        if let Some((block, reason)) = block {
            result.start_block = Some(block);
            result.start_reason = Some(reason);
        }
    }
    Ok(())
}

fn pause_block(
    conn: &Connection,
    state: MachineState,
    sample: &MachineSample,
    now: Timestamp,
) -> Result<Option<(StartBlock, String)>> {
    if let Some(emergency) = emergency::in_force(conn, now)? {
        return Ok(Some((
            StartBlock::Emergency,
            format!("Emergency {}", emergency.ticket),
        )));
    }
    if state == MachineState::Off || fleet::paused_everywhere(conn)? {
        let reason = if state == MachineState::Off {
            "This machine is off"
        } else {
            "Hand-offs are paused across the fleet"
        };
        return Ok(Some((StartBlock::Paused, reason.to_owned())));
    }
    Ok(sample.draining.then(|| {
        (
            StartBlock::Draining,
            "Waiting for running commands before a restart".to_owned(),
        )
    }))
}

fn ticket_block(
    free: &MachineFree,
    sample: &MachineSample,
    ticket: &ReadyTicket,
    budget: &Budget<'_>,
    state: MachineState,
    pending: u32,
    holds_one: bool,
) -> Option<(StartBlock, String)> {
    let master_blocker = held::holds_master(ticket);
    let backlog = sample.pressure.queue_backlogged();
    let fallback = state == MachineState::ChecksOnly;
    let pressure_block = || {
        budget
            .blocked(
                ticket,
                free.name.as_deref().unwrap_or(free.machine.as_str()),
            )
            .and_then(|HandoffWait { block, reason }| block.map(|block| (block, reason)))
    };
    if (backlog || fallback || sample.shared_failure.is_some()) && (holds_one || !master_blocker) {
        return if let Some(reason) = &sample.shared_failure {
            Some((StartBlock::SharedFailure, reason.clone()))
        } else if backlog && !master_blocker {
            pressure_block()
        } else if backlog {
            Some((
                StartBlock::Pacing,
                format!(
                    "Ticket commands have waited {} s for cores",
                    sample.pressure.queue_seconds
                ),
            ))
        } else {
            Some((
                StartBlock::Paused,
                "Checks-only mode allows one ticket holding master back".to_owned(),
            ))
        };
    }
    if free.free_ticket_trees == 0 {
        return Some((StartBlock::Trees, "No ticket tree is free".to_owned()));
    }
    if sample.room.memory_pressed {
        return Some((
            StartBlock::Memory,
            "Memory pressure prevents new workers".to_owned(),
        ));
    }
    if pending >= sample.room.start && !master_blocker {
        return Some((
            StartBlock::Workers,
            "No room to start another worker".to_owned(),
        ));
    }
    if !dispatch::cores_fit(free, ticket) {
        return Some((
            StartBlock::Cores,
            format!(
                "Needs {:.1} cores, {:.1} free",
                ticket.expected_cores,
                free.free_cores.max(0.0)
            ),
        ));
    }
    pressure_block()
}
