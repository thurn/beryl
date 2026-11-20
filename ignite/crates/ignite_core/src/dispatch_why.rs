//! Why the hub offered a machine nothing while it had idle workers, in one line for
//! `ignite status`. A person reading it decides whether more workers would help: they would only
//! if the answer is about workers, never if it is about tickets or cores.

use super::{JUSTICIAR, ReadyTicket, TicketKind, suits};
use crate::protocol::hub::MachineFree;
use crate::shares::Group;

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
        .into_iter()
        .filter(|t| free.idle_workers.iter().any(|w| suits(free, w, t)))
        .collect();
    if for_us.is_empty() {
        return format!(
            "{} ready {}, none for this machine's idle workers (another owner or model)",
            ready.len(),
            plural(ready.len())
        );
    }
    let in_order: Vec<&ReadyTicket> = for_us
        .iter()
        .copied()
        .filter(|t| justiciar(t) || t.kind != TicketKind::Ordinary || epic_order.contains(&t.epic))
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
        .filter(|t| {
            free.families.is_empty() || t.families.iter().all(|f| free.families.contains(f))
        })
        .collect();
    let Some(smallest) = runnable
        .iter()
        .map(|t| t.expected_cores)
        .min_by(f64::total_cmp)
    else {
        return format!(
            "{} ready {} need check families this machine cannot run",
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

fn plural(n: usize) -> &'static str {
    if n == 1 { "ticket" } else { "tickets" }
}

#[cfg(test)]
#[path = "dispatch_why_tests.rs"]
mod tests;
