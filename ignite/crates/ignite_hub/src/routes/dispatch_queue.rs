//! The queue snapshot behind `POST /next`: every ready ticket as `ignite_core::dispatch` sees it,
//! each group's share of workers, whether improvements are under their share, and each idle
//! worker's last files. The hub keeps no model of a machine's load; it reads what the machine
//! says it has free and the tickets in the database.
//!
//! What the hub cannot know is left out rather than guessed:
//!
//! - A ticket names its files as backquoted paths in its body or acceptance criteria. Areas are
//!   their first two directories, because workspace package roots are in each machine's
//!   `ignite.toml`, not at the hub.
//! - The hub holds no check manifests, so a ticket's families are empty and every machine can
//!   run them. A `needs:<os>` label still limits a ticket to one operating system.
//! - The backlog's ratios for the improvement share come from `crate::rules::backlog`.
//!
//! Not every ready ticket is offered (see [`Snapshot::offered`]): only a ticket under an active
//! epic or one the hub filed itself, never one set aside for a person or kept for something the
//! asking machine lacks, and nothing at all while the machine's hand-offs are paused.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rusqlite::Connection;

use ignite_core::admission::{self, Worked};
use ignite_core::config::{Improvement, Os};
use ignite_core::dispatch::{self, ReadyTicket, TicketKind};
use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::hub::{MachineFree, Reservation};
use ignite_core::protocol::tickets::{LandedWork, Saving, Status, Ticket, TicketType};
use ignite_core::shares::{self, EpicDemand, Group, Unit};
use ignite_core::time::Timestamp;
use ignite_store::hub::{fleet, tickets};
use ignite_store::{Result, StoreError};

use crate::rules;

/// The labels of tickets the hub files itself, which are offered whether or not an epic is
/// active. `hub` is on everything the hub files today (`upkeep::filing::HUB_LABEL`); the other
/// labels are the ones tickets filed before it carry, and `justiciar` marks the judge's
/// reports of faults in the system itself.
const HUB_FILED: [&str; 10] = [
    crate::upkeep::filing::HUB_LABEL,
    "regression",
    "improvement",
    "machine",
    "incomplete",
    "cache-audit",
    "graph-repair",
    "fleet",
    "flaky",
    dispatch::JUSTICIAR,
];

/// The prefix of a label naming what a ticket needs: an operating system, a machine's name, or
/// `person`.
const NEEDS: &str = "needs:";

/// What `needs:` names on a ticket set aside for a person, which no machine is offered.
const NEEDS_PERSON: &str = "person";

/// The ticket to reserve for `free`'s machine, already reserved and logged, or `None`. Call it
/// inside an `IMMEDIATE` transaction after lapsing old reservations, so that two machines asking
/// at once are answered one after the other and never get one ticket.
pub fn next(conn: &Connection, free: &MachineFree, now: Timestamp) -> Result<Option<Reservation>> {
    let machine = free.machine.as_str();
    if fleet::paused(conn, machine)? {
        return Ok(None);
    }
    // A worker of this machine that already holds a reservation is asking again because the
    // reply to its earlier request was lost; it gets the same reservation, not another ticket
    // and not nothing for ten minutes.
    for worker in &free.idle_workers {
        if let Some(held) = fleet::reservation_for_worker(conn, machine, worker.session.as_str())? {
            return Ok(Some(held));
        }
    }
    let snapshot = Snapshot::load(conn, now)?;
    // A worker holding a reservation for another machine is not idle.
    let waiting: HashSet<String> = snapshot.reserved.values().cloned().collect();
    let mut free = free.clone();
    free.idle_workers
        .retain(|worker| !waiting.contains(worker.session.as_str()));
    let mut ready = snapshot.ready_tickets(conn, machine, free.os, now)?;
    let demand = snapshot.demand(&ready);
    // Improvements go ahead of the epic order and so skip the share, but not a group's
    // `--max-workers` cap.
    let full: BTreeSet<&Group> = demand
        .iter()
        .filter(|group| group.max_workers.is_some_and(|cap| group.held >= cap))
        .map(|group| &group.epic)
        .collect();
    ready.retain(|ticket| ticket.kind != TicketKind::Improvement || !full.contains(&ticket.epic));
    let epic_order = shares::epic_order(&demand);
    let improvements = improvements_due(conn, machine, now)?;
    let last_files = last_files(conn, machine)?;
    let Some(handoff) = dispatch::next_for(&free, &ready, &epic_order, improvements, &last_files)
    else {
        return Ok(None);
    };
    let kind = ready
        .iter()
        .find(|ticket| ticket.id == handoff.ticket)
        .map_or(TicketKind::Ordinary, |ticket| ticket.kind);
    reserve(
        conn,
        &handoff.ticket,
        &free.machine,
        &handoff.session,
        kind,
        now,
    )
    .map(Some)
}

/// The next ready ticket of the `lane:` chain `lane`, reserved for the session that finished
/// the previous one, or `None`. A chain's next ticket skips the dispatch order and the share,
/// but not a pause or the rules of which tickets are offered.
pub fn chain_next(
    conn: &Connection,
    lane: &str,
    machine: &ignite_core::ids::MachineId,
    session: &SessionId,
    now: Timestamp,
) -> Result<Option<Reservation>> {
    if fleet::paused(conn, machine.as_str())? {
        return Ok(None);
    }
    let Some(os) = fleet::machines(conn)?
        .into_iter()
        .find(|m| m.id == machine.as_str())
        .and_then(|m| serde_json::from_value::<Os>(serde_json::Value::String(m.os)).ok())
    else {
        // A machine the hub does not know cannot be told which tickets it can take.
        return Ok(None);
    };
    let snapshot = Snapshot::load(conn, now)?;
    let ready = snapshot.ready_tickets(conn, machine.as_str(), os, now)?;
    let Some(next) = dispatch::chain_next(lane, &ready) else {
        return Ok(None);
    };
    reserve(conn, &next.id, machine, session, next.kind, now).map(Some)
}

fn reserve(
    conn: &Connection,
    ticket: &TicketId,
    machine: &ignite_core::ids::MachineId,
    session: &SessionId,
    kind: TicketKind,
    now: Timestamp,
) -> Result<Reservation> {
    let reservation = Reservation {
        ticket: ticket.clone(),
        machine: machine.clone(),
        session: session.clone(),
        expires_at: fleet::reservation_expiry(now),
        code: fleet::reservation_code()?,
    };
    fleet::reserve(conn, &reservation)?;
    let kind = match kind {
        TicketKind::Regression => "regression",
        TicketKind::Improvement => "improvement",
        TicketKind::Ordinary => "product",
    };
    fleet::record_handoff(conn, machine.as_str(), ticket.as_str(), kind, now)?;
    Ok(reservation)
}

/// The parts of a ticket that place it in a group.
struct Node {
    parent: Option<TicketId>,
    workers: Option<u32>,
    max_workers: Option<u32>,
    in_progress: bool,
}

/// Every ticket's place in the tree, the reservations and the chains of waiting tickets.
struct Snapshot {
    nodes: HashMap<TicketId, Node>,
    /// Reserved tickets and the session each waits for.
    reserved: HashMap<String, String>,
    /// For each ticket, the live tickets that depend on it.
    dependents: HashMap<String, Vec<String>>,
    regressions: HashSet<String>,
    ready: Vec<TicketId>,
}

impl Snapshot {
    fn load(conn: &Connection, now: Timestamp) -> Result<Snapshot> {
        let mut statement =
            conn.prepare("SELECT id, parent, workers, max_workers, status FROM tickets")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<u32>>(2)?,
                row.get::<_, Option<u32>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut nodes = HashMap::new();
        let mut live = HashSet::new();
        for row in rows {
            let (id, parent, workers, max_workers, status) = row?;
            if status != "closed" {
                live.insert(id.clone());
            }
            nodes.insert(
                parse_id(&id)?,
                Node {
                    parent: parent.as_deref().map(parse_id).transpose()?,
                    workers,
                    max_workers,
                    in_progress: status == "in_progress",
                },
            );
        }
        let reserved = pairs(conn, "SELECT ticket, session FROM reservations")?
            .into_iter()
            .collect();
        let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
        let waits = pairs(
            conn,
            "SELECT dst, src FROM edges WHERE kind IN ('blocks', 'conditional-blocks', 'waits-for')",
        )?;
        for (dst, src) in waits {
            if live.contains(&src) {
                dependents.entry(dst).or_default().push(src);
            }
        }
        let mut statement = conn.prepare("SELECT ticket FROM regressions WHERE state = 'open'")?;
        let regressions = statement
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(Snapshot {
            nodes,
            reserved,
            dependents,
            regressions,
            ready: tickets::ready(conn, now)?,
        })
    }

    /// The group a ticket shares workers in: its ancestors, nearest first, through
    /// [`shares::group_of`].
    fn group(&self, id: &TicketId) -> Group {
        let ancestors: Vec<(TicketId, Option<u32>)> = self
            .ancestors(id)
            .map(|id| {
                let workers = self.nodes.get(&id).and_then(|n| n.workers);
                (id, workers)
            })
            .collect();
        shares::group_of(&ancestors)
    }

    /// The cap on a group's workers: the smallest `max_workers` of the group's own ticket and
    /// all its ancestors, so a cap on an outer epic limits the epics inside it.
    fn cap(&self, id: &TicketId) -> Option<u32> {
        std::iter::once(id.clone())
            .chain(self.ancestors(id))
            .filter_map(|id| self.nodes.get(&id).and_then(|node| node.max_workers))
            .min()
    }

    /// The longest chain of live tickets waiting behind `id`.
    fn chain(&self, id: &str, memo: &mut HashMap<String, u32>, path: &mut HashSet<String>) -> u32 {
        if let Some(&known) = memo.get(id) {
            return known;
        }
        if !path.insert(id.to_owned()) {
            return 0;
        }
        let longest = self.dependents.get(id).map_or(0, |dependents| {
            dependents
                .iter()
                .map(|dependent| 1 + self.chain(dependent, memo, path))
                .max()
                .unwrap_or(0)
        });
        path.remove(id);
        memo.insert(id.to_owned(), longest);
        longest
    }

    /// The ready tickets nobody holds or has reserved that `machine` may be offered, leaving out
    /// epics and milestones, which are containers rather than work.
    fn ready_tickets(
        &self,
        conn: &Connection,
        machine: &str,
        os: Os,
        now: Timestamp,
    ) -> Result<Vec<ReadyTicket>> {
        let history = history(conn, machine)?;
        let landed = landed_files(conn)?;
        let name = fleet::name_of(conn, machine)?;
        let has = |needed: &str| needed == os.as_str() || name.as_deref() == Some(needed);
        let mut memo = HashMap::new();
        let mut ready = Vec::new();
        for id in &self.ready {
            if self.reserved.contains_key(id.as_str()) {
                continue;
            }
            let Some(ticket) = tickets::load(conn, id.as_str())? else {
                continue;
            };
            if ticket.status != Status::Open
                || ticket.assignee.is_some()
                || matches!(ticket.ticket_type, TicketType::Epic | TicketType::Milestone)
            {
                continue;
            }
            let Some(mut ticket) = self.offered(ticket, has) else {
                continue;
            };
            // Every `needs:` label has been checked against this machine, by name as well as by
            // operating system, which `ignite_core::dispatch` cannot do because it sees only the
            // operating system.
            ticket.labels.retain(|label| !label.starts_with(NEEDS));
            ready.push(self.ready_ticket(ticket, &history, &landed, &mut memo, now));
        }
        Ok(ready)
    }

    /// The ticket if it may be offered to a machine that `has` each thing a `needs:` label
    /// names: it is under an active epic or the hub filed it itself, and no `needs:` label sets
    /// it aside for a person or names something the machine lacks.
    ///
    /// An epic is active when it has a `workers` number greater than zero: the nearest ancestor
    /// with a `workers` number decides, so an epic set to 0 stops its milestones however their
    /// parent is set. A ticket started by name skips these rules, because `/claims` does not
    /// ask the queue.
    fn offered(&self, ticket: Ticket, has: impl Fn(&str) -> bool) -> Option<Ticket> {
        let filed_by_hub = self.regressions.contains(ticket.id.as_str())
            || ticket
                .labels
                .iter()
                .any(|label| HUB_FILED.contains(&label.as_str()));
        let active = self
            .ancestors(&ticket.id)
            .find_map(|id| self.nodes.get(&id).and_then(|node| node.workers))
            .is_some_and(|workers| workers > 0);
        let needs_met = ticket
            .labels
            .iter()
            .filter_map(|label| label.strip_prefix(NEEDS))
            .all(|needed| needed != NEEDS_PERSON && has(needed));
        ((filed_by_hub || active) && needs_met).then_some(ticket)
    }

    /// The ticket's ancestors, nearest first, stopping at a cycle.
    fn ancestors(&self, id: &TicketId) -> impl Iterator<Item = TicketId> + '_ {
        let mut seen = HashSet::new();
        let mut parent = self.nodes.get(id).and_then(|node| node.parent.clone());
        std::iter::from_fn(move || {
            let id = parent.take()?;
            if !seen.insert(id.clone()) {
                return None;
            }
            parent = self.nodes.get(&id).and_then(|node| node.parent.clone());
            Some(id)
        })
    }

    fn ready_ticket(
        &self,
        ticket: Ticket,
        history: &[Worked],
        landed: &[String],
        memo: &mut HashMap<String, u32>,
        now: Timestamp,
    ) -> ReadyTicket {
        let files = named_files(&format!("{}\n{}", ticket.body, ticket.acceptance));
        let areas = areas(&files, landed);
        let saving = ticket
            .saving
            .as_ref()
            .and_then(|Saving { unit, amount, .. }| Unit::parse(unit).map(|unit| (unit, *amount)));
        let kind = if self.regressions.contains(ticket.id.as_str()) {
            TicketKind::Regression
        } else if ticket.saving.is_some() {
            TicketKind::Improvement
        } else {
            TicketKind::Ordinary
        };
        ReadyTicket {
            kind,
            epic: self.group(&ticket.id),
            priority: ticket.priority,
            chain: self.chain(ticket.id.as_str(), memo, &mut HashSet::new()),
            created_at: ticket.created_at,
            expected_cores: admission::expected_cores(ticket.cores, &areas, history, now),
            families: Vec::new(),
            files,
            saving,
            labels: ticket.labels,
            id: ticket.id,
        }
    }

    /// Each group's share, cap, ready tickets and tickets in progress or reserved, across every
    /// machine, for the groups that have a ready ticket. Sorted by group, so the input order that
    /// breaks ties is the same on every request.
    fn demand(&self, ready: &[ReadyTicket]) -> Vec<EpicDemand> {
        let mut groups: BTreeMap<Group, EpicDemand> = BTreeMap::new();
        for ticket in ready {
            let group = ticket.epic.clone();
            let node = group.as_ref().and_then(|id| self.nodes.get(id));
            let cap = group.as_ref().and_then(|id| self.cap(id));
            groups
                .entry(group.clone())
                .or_insert_with(|| EpicDemand {
                    epic: group,
                    workers: node.and_then(|n| n.workers),
                    max_workers: cap,
                    ready: 0,
                    held: 0,
                })
                .ready += 1;
        }
        for (id, node) in &self.nodes {
            if (node.in_progress || self.reserved.contains_key(id.as_str()))
                && let Some(demand) = groups.get_mut(&self.group(id))
            {
                demand.held += 1;
            }
        }
        groups.into_values().collect()
    }
}

/// The paths a ticket names, in order and without repeats:
///
/// - backquoted words that contain a `/` and end in a file name with an extension;
/// - on a line that starts with `Files:` (after any list marker or emphasis), each word that
///   contains a `/` and ends in such a file name, backquoted or not, such as
///   `Files: game_scene/src/a.rs, tools/b.py (the parser)`. Tickets imported from the old
///   service list their files this way.
///
/// A path may be short, relative to a directory below the repository's root;
/// [`resolve_named_file`] finds the file it means.
pub fn named_files(text: &str) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    let mut add = |word: &str| {
        if is_named_path(word) && !files.iter().any(|f| f == word) {
            files.push(word.to_owned());
        }
    };
    for word in text.split('`').skip(1).step_by(2) {
        add(word);
    }
    for line in text.lines() {
        let line = line.trim_start_matches(|c: char| c.is_whitespace() || "-*+>_".contains(c));
        let Some(rest) = line
            .get(..6)
            .filter(|head| head.eq_ignore_ascii_case("files:"))
            .map(|_| &line[6..])
        else {
            continue;
        };
        let rest = rest.trim_start_matches(['*', '_']);
        for word in rest.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
            let word = word.trim_start_matches(|c: char| "`'\"([*_".contains(c));
            add(word.trim_end_matches(|c: char| "`'\")]*_.:".contains(c)));
        }
    }
    files
}

/// Whether a word names a file: it contains a `/`, no space and no URL scheme, and its last
/// part has an extension and is not a dotfile.
fn is_named_path(word: &str) -> bool {
    let name = word.rsplit('/').next().unwrap_or("");
    word.contains('/')
        && !word.contains(char::is_whitespace)
        && !word.contains("://")
        && name.contains('.')
        && !name.starts_with('.')
        && !name.ends_with('.')
}

/// The tracked file a named path means: the path itself when `tracked` (paths relative to the
/// repository's root) holds it, or else the one tracked file that ends in `/<path>`, such as
/// `rust/client/crates/game_scene/src/a.rs` for `game_scene/src/a.rs`. `None` when no
/// tracked file matches or several do.
pub fn resolve_named_file(path: &str, tracked: &[String]) -> Option<String> {
    let path = path.trim_start_matches("./");
    if tracked.iter().any(|t| t == path) {
        return Some(path.to_owned());
    }
    let suffix = format!("/{path}");
    let mut matching = tracked.iter().filter(|t| t.ends_with(&suffix));
    let first = matching.next()?;
    matching.next().is_none().then(|| first.clone())
}

/// The areas of the files a ticket names. A short path (see [`resolve_named_file`]) is resolved
/// against `known`, the repository paths that landed tickets touched, because the hub has no
/// checkout to list; a path that resolves to nothing is used as written.
fn areas(files: &[String], known: &[String]) -> Vec<String> {
    let mut areas: Vec<String> = files
        .iter()
        .map(|f| {
            let file = resolve_named_file(f, known).unwrap_or_else(|| f.clone());
            admission::area_of(&file, &[])
        })
        .collect();
    areas.sort();
    areas.dedup();
    areas
}

/// The repository paths that every landed ticket touched, without repeats, standing in for the
/// list of tracked files that the hub cannot read.
fn landed_files(conn: &Connection) -> Result<Vec<String>> {
    let mut statement = conn.prepare("SELECT landed FROM tickets WHERE landed IS NOT NULL")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut files = BTreeSet::new();
    for row in rows {
        if let Ok(work) = serde_json::from_str::<LandedWork>(&row?) {
            files.extend(work.files);
        }
    }
    Ok(files.into_iter().collect())
}

/// The machine's landed tickets, as history for expected cores: the areas of the files they
/// touched, the CPU their records used and the minutes from claim to landing.
fn history(conn: &Connection, machine: &str) -> Result<Vec<Worked>> {
    let mut statement = conn.prepare(
        "SELECT landed, claimed_at, landed_at,
           (SELECT COALESCE(SUM(cpu_s), 0) FROM records WHERE records.ticket = tickets.id)
         FROM tickets
         WHERE machine = ?1 AND landed_at IS NOT NULL AND claimed_at IS NOT NULL",
    )?;
    let rows = statement.query_map([machine], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, f64>(3)?,
        ))
    })?;
    let mut history = Vec::new();
    for row in rows {
        let (landed, claimed_at, landed_at, cpu_s) = row?;
        let work: Option<LandedWork> = landed.and_then(|text| serde_json::from_str(&text).ok());
        let (Ok(claimed_at), Ok(landed_at)) = (
            claimed_at.parse::<Timestamp>(),
            landed_at.parse::<Timestamp>(),
        ) else {
            continue;
        };
        history.push(Worked {
            landed_at,
            areas: areas(&work.map(|w| w.files).unwrap_or_default(), &[]),
            core_minutes: cpu_s / 60.0,
            minutes: landed_at.seconds_since(claimed_at) / 60.0,
        });
    }
    Ok(history)
}

/// `None` when improvements have their share of the machine's recent hand-offs, and otherwise
/// the unit whose largest saving goes first: the one after the last improvement's unit.
fn improvements_due(conn: &Connection, machine: &str, now: Timestamp) -> Result<Option<Unit>> {
    let config = Improvement::default();
    let recent = taken_handoffs(conn, machine, config.window)?;
    let kinds: Vec<TicketKind> = recent
        .iter()
        .rev()
        .map(|(_, kind)| match kind.as_str() {
            "improvement" => TicketKind::Improvement,
            "regression" => TicketKind::Regression,
            _ => TicketKind::Ordinary,
        })
        .collect();
    let share = shares::improvement_share(rules::backlog::ratios(conn, now)?.fraction, &config);
    if !shares::improvements_due(&kinds, share, &config) {
        return Ok(None);
    }
    let mut last = None;
    if let Some((ticket, _)) = recent.iter().find(|(_, kind)| kind == "improvement") {
        last = tickets::load(conn, ticket)?
            .and_then(|ticket| ticket.saving)
            .and_then(|saving| Unit::parse(&saving.unit));
    }
    Ok(Some(shares::first_unit(last)))
}

/// The last `n` hand-offs to a machine that were not regressions and whose reservation did not
/// lapse, newest first, as `(ticket, kind)`: the improvement share is a share of these. A
/// hand-off counts while its reservation stands, and once the machine has claimed the ticket
/// since; a reservation that lapsed unclaimed was never work, so it does not count.
fn taken_handoffs(conn: &Connection, machine: &str, n: u32) -> Result<Vec<(String, String)>> {
    let mut statement = conn.prepare(
        "SELECT h.ticket, h.kind FROM handoffs h
         WHERE h.machine = ?1 AND h.kind != 'regression'
           AND (EXISTS (SELECT 1 FROM reservations r
                        WHERE r.ticket = h.ticket AND r.machine = h.machine)
                OR EXISTS (SELECT 1 FROM tickets t
                           WHERE t.id = h.ticket AND t.machine = h.machine
                             AND t.claimed_at >= h.at))
         ORDER BY h.seq DESC LIMIT ?2",
    )?;
    let rows = statement
        .query_map(rusqlite::params![machine, i64::from(n)], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

/// Each worker's last ticket's files, as its machine last reported them.
fn last_files(conn: &Connection, machine: &str) -> Result<Vec<(SessionId, Vec<String>)>> {
    let mut last = Vec::new();
    for worker in fleet::workers(conn, machine)? {
        let (Ok(session), Ok(files)) = (
            worker.session.parse::<SessionId>(),
            serde_json::from_str::<Vec<String>>(&worker.files),
        ) else {
            continue;
        };
        last.push((session, files));
    }
    Ok(last)
}

fn pairs(conn: &Connection, sql: &str) -> Result<Vec<(String, String)>> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

fn parse_id(id: &str) -> Result<TicketId> {
    id.parse()
        .map_err(|_| StoreError::Invalid(format!("bad ticket ID {id}")))
}

#[cfg(test)]
mod named_file_tests {
    use super::{areas, named_files, resolve_named_file};

    #[test]
    fn a_short_path_gets_the_area_of_the_file_it_resolves_to() {
        let known = ["rust/crates/scene/src/a.rs".to_owned()];
        let full = areas(&["rust/crates/scene/src/a.rs".to_owned()], &[]);
        assert_eq!(areas(&["scene/src/a.rs".to_owned()], &known), full);
        assert_ne!(areas(&["scene/src/a.rs".to_owned()], &[]), full);
    }

    #[test]
    fn a_ticket_names_the_paths_on_its_files_line() {
        let text = "Fix the parser.\n\n\
                    Files: game_scene/src/element/style.rs, `tools/b.py` (the parser), \
                    docs/a.md, `e/f.rs`.\n\
                    - **Files:** ui/src/x.ts; the `why` command in ignite/crates/ignite_cli\n\
                    Not a files line: c/d.rs";
        assert_eq!(
            named_files(text),
            vec![
                "tools/b.py",
                "e/f.rs",
                "game_scene/src/element/style.rs",
                "docs/a.md",
                "ui/src/x.ts",
            ]
        );
    }

    #[test]
    fn a_short_path_resolves_to_the_one_tracked_file_it_ends() {
        let tracked: Vec<String> = [
            "rust/client/crates/game_scene/src/style.rs",
            "rust/a/src/lib.rs",
            "rust/b/src/lib.rs",
            "docs/a.md",
        ]
        .map(str::to_owned)
        .to_vec();
        let resolve = |path: &str| resolve_named_file(path, &tracked);
        assert_eq!(resolve("docs/a.md").as_deref(), Some("docs/a.md"));
        assert_eq!(
            resolve("game_scene/src/style.rs").as_deref(),
            Some("rust/client/crates/game_scene/src/style.rs")
        );
        // Two crates hold a `src/lib.rs`, and nothing holds `scene/src/style.rs` as a whole part.
        assert_eq!(resolve("src/lib.rs"), None);
        assert_eq!(resolve("scene/src/style.rs"), None);
        assert_eq!(resolve("docs/missing.md"), None);
    }
}
