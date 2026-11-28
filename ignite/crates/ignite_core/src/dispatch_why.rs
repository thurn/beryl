//! Why the hub offered a machine nothing while it had idle workers, in one line for
//! `ignite status`. A person reading it decides whether more workers would help: they would only
//! if the answer is about workers, never if it is about tickets or cores.

use super::{JUSTICIAR, ReadyTicket, TicketKind, label, suits};
use crate::admission;
use crate::placement::{self, MachineView, PlacementTicket};
use crate::protocol::hub::MachineFree;
use crate::shares::Group;

/// Why an eligible ticket was passed over for a better machine.
pub fn why_placement(
    ticket: &PlacementTicket<'_>,
    asker: &MachineView<'_>,
    better: &MachineView<'_>,
) -> String {
    let warm: Vec<_> = placement::missing_outputs(ticket.ready, asker.capacity)
        .filter(|output| placement::holds_output(better.capacity, output))
        .collect();
    let reason = if warm.is_empty() {
        let here = placement::cost(ticket, asker).expect("the asker can take the ticket");
        let there = placement::cost(ticket, better).expect("the peer can take the ticket");
        if !here.total_cmp(&there).is_eq() {
            format!("its estimate is {there:.0} s and ours is {here:.0} s")
        } else if placement::free_fraction(better.capacity)
            > placement::free_fraction(asker.capacity)
        {
            "it has a larger share of its cores free".to_owned()
        } else {
            "the estimates and free-core shares tie, and its name comes first".to_owned()
        }
    } else {
        let outputs = warm.join(", ");
        let seconds = placement::cold_seconds(asker.capacity, warm.len());
        format!("it holds {outputs}, and here they would build cold (about {seconds:.0} s)")
    };
    format!(
        "{} is better placed on {}: {reason}",
        ticket.ready.id, better.name
    )
}

/// The first reason, in the order dispatch tests them, that no ready ticket went to any of
/// `free`'s idle workers. The arguments are those [`super::next_for`] was given and returned
/// `None` for; `free.free_cores` is the machine's room for expected cores ([`super::room`]).
pub fn why_nothing(free: &MachineFree, ready: &[ReadyTicket], epic_order: &[Group]) -> String {
    if free.idle_workers.is_empty() {
        return "every idle worker already has a reservation".to_owned();
    }
    if free.free_ticket_trees == 0 {
        return "no ticket tree is free".to_owned();
    }
    if ready.is_empty() {
        return if free.master_only {
            "the pool is paused and no ticket holds master back".to_owned()
        } else {
            "no ready ticket is offered: file tickets, or release an epic".to_owned()
        };
    }
    let justiciar = |t: &&ReadyTicket| t.labels.iter().any(|l| l == JUSTICIAR);
    let candidates: Vec<&ReadyTicket> = if free.justiciar_only {
        ready.iter().filter(justiciar).collect()
    } else {
        ready.iter().collect()
    };
    if candidates.is_empty() {
        return "the last idle worker is kept for justiciar tickets, and none is ready".to_owned();
    }
    let for_us: Vec<&ReadyTicket> = candidates
        .iter()
        .copied()
        .filter(|t| free.idle_workers.iter().any(|w| suits(free, w, t)))
        .collect();
    if for_us.is_empty() {
        let machine = free.name.as_deref().unwrap_or(free.machine.as_str());
        let pin = candidates.iter().find_map(|ticket| {
            if ticket.pins.admits_machine(machine) {
                ticket.pins.provider.as_deref().filter(|_| {
                    !free
                        .idle_workers
                        .iter()
                        .any(|worker| ticket.pins.admits_provider(&worker.app))
                })
            } else {
                ticket.pins.machine.as_deref()
            }
        });
        let reason = pin.map_or_else(
            || "no idle worker runs the required model".to_owned(),
            |pin| format!("pinned to {pin}"),
        );
        return format!(
            "{} ready {}, none for this machine: {reason}",
            ready.len(),
            plural(ready.len())
        );
    }
    let in_order: Vec<&ReadyTicket> = for_us
        .iter()
        .copied()
        .filter(|t| {
            justiciar(t)
                || t.kind != TicketKind::Ordinary
                || t.saved_work == super::SavedWork::Finished
                || epic_order.contains(&t.epic)
        })
        .collect();
    if in_order.is_empty() {
        return format!(
            "{} ready {} for this machine, all in epics at their --max-workers cap",
            for_us.len(),
            plural(for_us.len())
        );
    }
    let runnable: Vec<&ReadyTicket> = in_order
        .into_iter()
        .filter(|t| super::platform_fits(free, t))
        .collect();
    let Some(smallest) = runnable
        .iter()
        .map(|t| t.expected_cores)
        .min_by(f64::total_cmp)
    else {
        return format!(
            "{} ready {} need an operating system or check families this machine cannot run",
            for_us.len(),
            plural(for_us.len())
        );
    };
    format!(
        "{} ready {} wait for cores: the smallest expects {smallest:.1}, and {:.1} of the \
         machine's admitted cores are free",
        runnable.len(),
        plural(runnable.len()),
        free.free_cores.max(0.0)
    )
}

/// Explain a room-based request using the same order and provider checks as the
/// selection. Include the first eligible ticket's blockers in the reply.
pub fn why_nothing_with_room(
    free: &MachineFree,
    ready: &[ReadyTicket],
    epic_order: &[Group],
    improvements: Option<crate::shares::Unit>,
    provider_blocker: impl Fn(&ReadyTicket) -> Option<String>,
) -> String {
    if ready.is_empty() {
        return if free.master_only {
            "the pool is paused and no ticket holds master back".to_owned()
        } else {
            "no ready ticket is offered: file tickets, or release an epic".to_owned()
        };
    }
    let ordered = super::order(ready, epic_order, improvements);
    let machine = free.name.as_deref().unwrap_or(free.machine.as_str());
    let candidate = ordered
        .iter()
        .find(|(ticket, _)| {
            ticket.pins.admits_machine(machine) && ticket.needs_os.is_none_or(|os| os == free.os)
        })
        .or_else(|| ordered.first());
    if let Some((ticket, _)) = candidate {
        let reason = blocker_with(free, ticket, || provider_blocker(ticket))
            .unwrap_or_else(|| "waiting for a worker to start".to_owned());
        return format!("{} {reason}", ticket.id);
    }
    "all ready tickets are in epics at their --max-workers cap".to_owned()
}

/// [`blocker`]'s reason when the machine asked only for tickets that hold `master` back
/// (`MachineFree::master_only`), which a pause, a backed-up build queue or a usage hold sets.
pub const MASTER_ONLY: &str = "only tickets that hold master back start here";

/// Why the machine `free` describes cannot take `ticket` now, in a few words, or `None` when it
/// can: it has a free ticket tree, an idle worker that suits the ticket, the check families the
/// ticket needs and room for its expected cores. The tests run in the order a person would fix
/// them, so the first reason is the one to read. `free.free_cores` is the machine's room for
/// expected cores ([`super::room`]).
pub fn blocker(free: &MachineFree, ticket: &ReadyTicket) -> Option<String> {
    blocker_with(free, ticket, || {
        if let Some(pin) = &ticket.pins.provider
            && !free
                .idle_workers
                .iter()
                .any(|worker| ticket.pins.admits_provider(&worker.app))
        {
            return Some(format!("pinned to provider {pin}"));
        }
        if !free.idle_workers.iter().any(|w| suits(free, w, ticket)) {
            let tier = label(&ticket.labels, "tier:").unwrap_or_default();
            return Some(format!("no idle worker runs {tier}"));
        }
        None
    })
}

/// Explain a machine's constraints with its dispatch mode's worker or provider check.
pub fn blocker_with(
    free: &MachineFree,
    ticket: &ReadyTicket,
    worker_blocker: impl FnOnce() -> Option<String>,
) -> Option<String> {
    let justiciar = ticket.labels.iter().any(|l| l == JUSTICIAR);
    let machine = free.name.as_deref().unwrap_or(free.machine.as_str());
    if let Some(pin) = &ticket.pins.machine
        && !ticket.pins.admits_machine(machine)
    {
        return Some(format!("pinned to machine {pin}"));
    }
    if let Some(os) = ticket.needs_os
        && os != free.os
    {
        return Some(format!("needs {}", os.as_str()));
    }
    if free.master_only && !justiciar && ticket.kind != TicketKind::Regression {
        return Some(MASTER_ONLY.to_owned());
    }
    if free.room.is_none() && free.idle_workers.is_empty() {
        return Some("every worker is busy".to_owned());
    }
    if free.free_ticket_trees == 0 {
        return Some("no free ticket tree".to_owned());
    }
    if free.justiciar_only && !justiciar {
        return Some("last idle worker kept for justiciar tickets".to_owned());
    }
    if let Some(reason) = worker_blocker() {
        return Some(reason);
    }
    if !ticket.families_fit(free.os, &free.families) {
        return Some("needs check families this machine cannot run".to_owned());
    }
    if ticket.kind != TicketKind::Regression
        && !admission::fits(free.free_cores, ticket.expected_cores)
    {
        return Some(format!(
            "needs {:.1} cores, {:.1} free",
            ticket.expected_cores,
            free.free_cores.max(0.0)
        ));
    }
    None
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "ticket" } else { "tickets" }
}

#[cfg(test)]
#[path = "dispatch_why_tests.rs"]
mod tests;
