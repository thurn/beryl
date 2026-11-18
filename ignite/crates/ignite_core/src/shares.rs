//! Shares: dividing idle workers between epics, and the share of hand-offs owed to improvement
//! tickets.
//!
//! Epic shares. A person gives an epic a number of workers (`--workers`). The numbers are
//! shares, not reservations: the next free worker goes to the group with the fewest tickets in
//! progress per share, counted across every machine. A group with fewer ready tickets than its
//! share takes what it can use and the others divide the rest. `--max-workers` is a hard cap,
//! the one setting that can leave a worker idle. A group with no number has a share of 1, and
//! tickets under no epic form one group (`None`) with a share of 1.
//!
//! Improvement share. Improvement tickets get at least 15% of the last 20 hand-offs to a
//! machine, not counting regressions, rising in a straight line to 30% as the backlog's saving
//! grows from 5% to 20% of last week's total in its largest unit. Within the share the largest
//! saving goes first, taking the three units in turn.

use crate::config::Improvement;
use crate::dispatch::TicketKind;
use crate::ids::TicketId;

/// A group of tickets that shares workers: an epic, or `None` for tickets under no epic.
pub type Group = Option<TicketId>;

/// One group's claim on workers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpicDemand {
    pub epic: Group,
    /// The group's `--workers` share; `None` counts as 1.
    pub workers: Option<u32>,
    /// The group's `--max-workers` cap.
    pub max_workers: Option<u32>,
    /// Its ready tickets.
    pub ready: u32,
    /// Workers holding its tickets now, on every machine.
    pub held: u32,
}

impl EpicDemand {
    /// The share, never below 1: a share of 0 would divide nothing, and pausing an epic is what
    /// `--max-workers 0` is for.
    fn share(&self) -> u64 {
        u64::from(self.workers.unwrap_or(1).max(1))
    }

    fn cap(&self) -> u32 {
        self.max_workers.unwrap_or(u32::MAX)
    }
}

/// The group a ticket belongs to, from its ancestors listed nearest first with each one's
/// `--workers` number: the nearest that has one. When none has one, the outermost ancestor, so
/// that an epic's milestones share the epic's 1 instead of each claiming its own. `None` when
/// the ticket has no ancestor.
pub fn group_of(ancestors: &[(TicketId, Option<u32>)]) -> Group {
    ancestors
        .iter()
        .find(|(_, workers)| workers.is_some())
        .or(ancestors.last())
        .map(|(id, _)| id.clone())
}

/// Whether group `a`, holding `a_held`, comes before group `b`, holding `b_held`, for the next
/// worker: fewer in progress per share first, then the larger share, then the earlier in the
/// input (`a_index < b_index`).
fn before(
    a: &EpicDemand,
    a_held: u32,
    a_index: usize,
    b: &EpicDemand,
    b_held: u32,
    b_index: usize,
) -> bool {
    let left = u64::from(a_held) * b.share();
    let right = u64::from(b_held) * a.share();
    left.cmp(&right)
        .then(b.share().cmp(&a.share()))
        .then(a_index.cmp(&b_index))
        .is_lt()
}

/// Whether a group can use one more worker beyond the `given` it has been handed here.
fn wants(epic: &EpicDemand, given: u32) -> bool {
    given < epic.ready && epic.held.saturating_add(given) < epic.cap()
}

/// How many of `idle` workers each group should get, in the order given. Each worker goes in
/// turn to the group with the fewest in progress per share that still has a ready ticket and is
/// under its cap; workers no group can use are left out of the total.
pub fn divide(idle: u32, epics: &[EpicDemand]) -> Vec<(Group, u32)> {
    let mut given = vec![0_u32; epics.len()];
    for _ in 0..idle {
        let mut best: Option<usize> = None;
        for (index, epic) in epics.iter().enumerate() {
            if !wants(epic, given[index]) {
                continue;
            }
            let better = best.is_none_or(|b| {
                before(
                    epic,
                    epic.held + given[index],
                    index,
                    &epics[b],
                    epics[b].held + given[b],
                    b,
                )
            });
            if better {
                best = Some(index);
            }
        }
        let Some(best) = best else { break };
        given[best] += 1;
    }
    epics
        .iter()
        .zip(given)
        .map(|(epic, n)| (epic.epic.clone(), n))
        .collect()
}

/// The groups that can take a worker now, furthest below its share first: the order dispatch
/// tries them in. A group with no ready ticket, or at its cap, is left out.
pub fn epic_order(epics: &[EpicDemand]) -> Vec<Group> {
    let mut open: Vec<usize> = (0..epics.len()).filter(|&i| wants(&epics[i], 0)).collect();
    open.sort_by(|&a, &b| {
        if before(&epics[a], epics[a].held, a, &epics[b], epics[b].held, b) {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });
    open.into_iter().map(|i| epics[i].epic.clone()).collect()
}

/// A unit an improvement's saving is measured in, per week.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unit {
    Dollars,
    AgentHours,
    CoreHours,
}

impl Unit {
    /// The three units in the order improvements take turns.
    pub const ALL: [Unit; 3] = [Unit::Dollars, Unit::AgentHours, Unit::CoreHours];

    /// The unit's name in a ticket's saving: `dollars`, `agent-hours` or `core-hours`.
    pub fn as_str(self) -> &'static str {
        match self {
            Unit::Dollars => "dollars",
            Unit::AgentHours => "agent-hours",
            Unit::CoreHours => "core-hours",
        }
    }

    pub fn parse(text: &str) -> Option<Unit> {
        Unit::ALL.into_iter().find(|unit| unit.as_str() == text)
    }

    /// The unit whose turn follows this one.
    #[must_use]
    pub fn next(self) -> Unit {
        match self {
            Unit::Dollars => Unit::AgentHours,
            Unit::AgentHours => Unit::CoreHours,
            Unit::CoreHours => Unit::Dollars,
        }
    }
}

/// The unit whose largest saving goes first in the next improvement hand-off: the one after the
/// unit of the last improvement handed off, or dollars when there was none.
pub fn first_unit(last: Option<Unit>) -> Unit {
    last.map_or(Unit::Dollars, Unit::next)
}

/// An amount in each of the three units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Amounts {
    pub dollars: f64,
    pub agent_hours: f64,
    pub core_hours: f64,
}

impl Amounts {
    pub fn get(&self, unit: Unit) -> f64 {
        match unit {
            Unit::Dollars => self.dollars,
            Unit::AgentHours => self.agent_hours,
            Unit::CoreHours => self.core_hours,
        }
    }
}

/// The backlog's estimated saving as a fraction of last week's total, in whichever unit gives
/// the largest fraction. A unit with no total last week is left out.
pub fn backlog_fraction(backlog: &Amounts, last_week: &Amounts) -> f64 {
    Unit::ALL
        .into_iter()
        .filter(|&unit| last_week.get(unit) > 0.0)
        .map(|unit| backlog.get(unit) / last_week.get(unit))
        .fold(0.0, f64::max)
}

/// The improvement share for a backlog whose saving is `backlog_fraction` of last week's
/// total, in the largest of the three units: `min_share` at or below `backlog_low`,
/// `max_share` at or above `backlog_high`, and a straight line between.
pub fn improvement_share(backlog_fraction: f64, config: &Improvement) -> f64 {
    let span = config.backlog_high - config.backlog_low;
    if span <= 0.0 {
        return if backlog_fraction >= config.backlog_high {
            config.max_share
        } else {
            config.min_share
        };
    }
    let t = ((backlog_fraction - config.backlog_low) / span).clamp(0.0, 1.0);
    config.min_share + (config.max_share - config.min_share) * t
}

/// Whether improvements are under `share` of the machine's recent hand-offs, given oldest first.
/// Regressions are not counted, and only the last `config.window` of the rest are. With no
/// hand-offs yet, improvements are under any share above zero.
pub fn improvements_due(recent: &[TicketKind], share: f64, config: &Improvement) -> bool {
    let counted: Vec<TicketKind> = recent
        .iter()
        .rev()
        .filter(|&&kind| kind != TicketKind::Regression)
        .take(config.window as usize)
        .copied()
        .collect();
    let improvements = counted
        .iter()
        .filter(|&&kind| kind == TicketKind::Improvement)
        .count();
    if counted.is_empty() {
        return share > 0.0;
    }
    (improvements as f64) < share * counted.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> TicketId {
        TicketId::new(text).unwrap()
    }

    fn epic(name: &str, workers: Option<u32>, ready: u32) -> EpicDemand {
        EpicDemand {
            epic: Some(id(name)),
            workers,
            max_workers: None,
            ready,
            held: 0,
        }
    }

    fn counts(result: &[(Group, u32)]) -> Vec<u32> {
        result.iter().map(|(_, n)| *n).collect()
    }

    #[test]
    fn shares_of_5_and_2_divide_14_and_7_workers_in_proportion() {
        let epics = [epic("Tq44s", Some(5), 100), epic("Tw8gv", Some(2), 100)];
        assert_eq!(counts(&divide(14, &epics)), vec![10, 4]);
        assert_eq!(counts(&divide(7, &epics)), vec![5, 2]);
    }

    #[test]
    fn an_epic_with_one_ready_ticket_leaves_the_rest_to_the_other() {
        let epics = [epic("Tq44s", Some(5), 100), epic("Tw8gv", Some(2), 1)];
        assert_eq!(counts(&divide(7, &epics)), vec![6, 1]);
    }

    #[test]
    fn workers_in_progress_count_against_a_share() {
        let mut busy = epic("Tq44s", Some(5), 100);
        busy.held = 5;
        let epics = [busy, epic("Tw8gv", Some(2), 100)];
        // 12 in all, so 5 x 12 / 7 is about 8.6 against 3.4: the busy epic gets 3 more and
        // the other 4.
        assert_eq!(counts(&divide(7, &epics)), vec![3, 4]);
        assert_eq!(
            epic_order(&epics),
            vec![Some(id("Tw8gv")), Some(id("Tq44s"))]
        );
    }

    #[test]
    fn max_workers_is_a_hard_cap_that_can_leave_workers_idle() {
        let mut capped = epic("Tq44s", None, 100);
        capped.max_workers = Some(2);
        capped.held = 1;
        assert_eq!(counts(&divide(5, &[capped.clone()])), vec![1]);
        capped.held = 2;
        assert!(epic_order(&[capped]).is_empty());
    }

    #[test]
    fn tickets_under_no_epic_share_like_an_epic_with_a_share_of_one() {
        let loose = EpicDemand {
            epic: None,
            workers: None,
            max_workers: None,
            ready: 10,
            held: 0,
        };
        let epics = [epic("Tq44s", Some(3), 10), loose];
        assert_eq!(counts(&divide(8, &epics)), vec![6, 2]);
        assert_eq!(divide(8, &epics)[1].0, None);
    }

    #[test]
    fn a_ticket_belongs_to_the_nearest_ancestor_with_a_number() {
        let milestone = (id("Tq44s.3"), None);
        let numbered = (id("Tq44s"), Some(5));
        assert_eq!(group_of(&[milestone.clone(), numbered]), Some(id("Tq44s")));
        let top = (id("Tq44s"), None);
        assert_eq!(group_of(&[milestone, top]), Some(id("Tq44s")));
        assert_eq!(group_of(&[]), None);
    }

    #[test]
    fn a_backlog_at_11_percent_gives_a_21_percent_share() {
        let backlog = Amounts {
            dollars: 310.0,
            agent_hours: 9.0,
            core_hours: 200.0,
        };
        let last_week = Amounts {
            dollars: 8305.0,
            agent_hours: 1100.0,
            core_hours: 1800.0,
        };
        let fraction = backlog_fraction(&backlog, &last_week);
        assert!((fraction - 200.0 / 1800.0).abs() < 1e-12);
        let config = Improvement::default();
        // The document rounds the fraction to 11%, which gives exactly 21%.
        assert!((improvement_share(0.11, &config) - 0.21).abs() < 1e-12);
        assert!((improvement_share(fraction, &config) - 0.211).abs() < 1e-3);
    }

    #[test]
    fn the_improvement_share_is_clamped_between_its_ends() {
        let config = Improvement::default();
        assert!((improvement_share(0.01, &config) - 0.15).abs() < 1e-12);
        assert!((improvement_share(0.05, &config) - 0.15).abs() < 1e-12);
        assert!((improvement_share(0.20, &config) - 0.30).abs() < 1e-12);
        assert!((improvement_share(0.90, &config) - 0.30).abs() < 1e-12);
    }

    #[test]
    fn improvements_are_due_below_their_share_of_the_last_hand_offs() {
        use TicketKind::{Improvement as I, Ordinary as O, Regression as R};
        let config = Improvement::default();
        assert!(improvements_due(&[], 0.15, &config));
        // 3 of 20 is exactly 15%: not under the share.
        let mut recent = vec![O; 17];
        recent.extend([I, I, I]);
        assert!(!improvements_due(&recent, 0.15, &config));
        // Regressions are not counted, so they do not push an improvement out of the window.
        recent.extend([R; 5]);
        assert!(!improvements_due(&recent, 0.15, &config));
        // One more ordinary hand-off pushes the oldest out: 3 of 20 still.
        recent.push(O);
        assert!(!improvements_due(&recent, 0.15, &config));
        // At a 21% share, 3 of 20 is under.
        assert!(improvements_due(&recent, 0.21, &config));
    }

    #[test]
    fn improvement_units_take_turns() {
        assert_eq!(first_unit(None), Unit::Dollars);
        assert_eq!(first_unit(Some(Unit::Dollars)), Unit::AgentHours);
        assert_eq!(first_unit(Some(Unit::CoreHours)), Unit::Dollars);
        for unit in Unit::ALL {
            assert_eq!(Unit::parse(unit.as_str()), Some(unit));
        }
    }
}
