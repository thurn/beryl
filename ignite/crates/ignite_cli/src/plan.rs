//! `ignite plan check <epic>`: the shape of an epic's ticket graph. It reports the ticket count
//! and how many are ready at the start, the longest chain, the estimated effort remaining,
//! dependencies between tickets that name no file in common, tickets that name no files,
//! paths that the repository does not hold, `lane:` groups that are not chains, and problems
//! that stall a run. It fails when a blocking dependency leaves the outermost epic.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use ignite_core::fleet::FleetView;
use ignite_core::ids::TicketId;
use ignite_core::pins::{Pins, from_labels};
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::tickets::{Edge, Graph, Status, Ticket, TicketType};
use ignite_core::work_estimate::{WorkRemaining, needs_estimate};
use serde_json::{Value, json};

use ignite_hub::routes::dispatch::{named_files, resolve_named_file};

use crate::args::PlanCommand;
use crate::ticket::ids::spelled;
use crate::ticket::{Failure, Hub};

/// The width of the label column.
const LABEL_WIDTH: usize = 15;

pub fn run(command: &PlanCommand) -> Reply {
    let PlanCommand::Check { epic } = command;
    load(epic).unwrap_or_else(Failure::into_reply)
}

fn load(epic: &str) -> Result<Reply, Failure> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let hub = Hub::load(&cwd, "ignite".to_owned(), |name| std::env::var(name).ok())?;
    let epic = hub.get(&["tickets", epic], &[])?.json()?;
    let epic_id = epic["id"].as_str().unwrap_or(epic_name(&epic)).to_owned();
    let query = [("ancestor", epic_id.clone()), ("all", "1".to_owned())];
    let tickets: Vec<Ticket> = decode(hub.get(&["tickets"], &query)?.json()?)?;
    let graph: Graph = decode(hub.get(&["graph"], &[("all", "1".to_owned())])?.json()?)?;
    let ready: HashSet<String> = graph
        .tickets
        .iter()
        .filter(|ticket| ticket.ready)
        .map(|ticket| ticket.ticket.id.to_string())
        .collect();
    let everything: Vec<Ticket> = graph.tickets.into_iter().map(|t| t.ticket).collect();
    let fleet: FleetView = decode(hub.get(&["fleet"], &[])?.json()?)?;
    let machines = fleet
        .machines
        .iter()
        .map(|machine| machine.name.to_ascii_lowercase())
        .collect();
    let providers = fleet
        .settings
        .providers
        .into_keys()
        .map(|provider| provider.to_ascii_lowercase())
        .collect();
    // The repository's files, which short paths are resolved against. Without them every named
    // path is taken as it is written.
    let repository = ignite_git::tracked::toplevel(&cwd)
        .and_then(|root| {
            let tracked = ignite_git::tracked::files(&root)?;
            Ok(Repository { root, tracked })
        })
        .ok();
    Ok(Graphs {
        epic: &epic_id,
        tickets: &tickets,
        everything: &everything,
        edges: &graph.edges,
        ready: &ready,
        machines: &machines,
        providers: &providers,
        repository: repository.as_ref(),
    }
    .reply())
}

fn epic_name(epic: &Value) -> &str {
    epic["id"].as_str().unwrap_or_default()
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, Failure> {
    serde_json::from_value(value).map_err(|error| Failure::new(Exit::Usage, error))
}

/// An epic's tickets and the graph around them, ready to be reported on.
struct Graphs<'a> {
    epic: &'a str,
    /// The epic's descendants, closed ones included.
    tickets: &'a [Ticket],
    /// Every ticket in the graph, for looking up tickets outside the epic.
    everything: &'a [Ticket],
    edges: &'a [Edge],
    ready: &'a HashSet<String>,
    /// The fleet names a ticket may pin.
    machines: &'a BTreeSet<String>,
    providers: &'a BTreeSet<String>,
    /// The repository the named paths are in; `None` when unknown.
    repository: Option<&'a Repository>,
}

/// The checkout `plan check` runs in.
struct Repository {
    root: std::path::PathBuf,
    /// Every file git tracks, relative to the root.
    tracked: Vec<String>,
}

impl Graphs<'_> {
    fn reply(&self) -> Reply {
        let live: Vec<&Ticket> = self
            .tickets
            .iter()
            .filter(|ticket| ticket.status != Status::Closed)
            .collect();
        let ids: HashSet<&str> = live.iter().map(|t| t.id.as_str()).collect();
        let deps = self.dependencies(&ids);
        let external = self.external_edges(&live);
        let exit = if external.is_empty() {
            Exit::Success
        } else {
            Exit::CheckFailed
        };
        let mut missing: Vec<(&str, Vec<String>)> = Vec::new();
        let files: HashMap<&str, Vec<String>> = live
            .iter()
            .map(|ticket| {
                let (found, absent) = resolve(&paths(ticket), self.repository);
                if !absent.is_empty() {
                    missing.push((ticket.id.as_str(), absent));
                }
                (ticket.id.as_str(), found)
            })
            .collect();

        let mut lines = Vec::new();
        let ready = live
            .iter()
            .filter(|ticket| self.ready.contains(ticket.id.as_str()))
            .count();
        lines.push(row(
            "tickets",
            &format!("{}, ready at the start {ready}", live.len()),
        ));
        lines.push(row("longest chain", &self.chain(&live, &deps)));
        lines.extend(self.forecast(&live));
        lines.extend(external);
        lines.extend(self.suspect_edges(&deps, &files));
        lines.extend(self.nameless(&live, &files));
        lines.extend(missing.iter().map(|(id, absent)| {
            let verb = if absent.len() == 1 { "does" } else { "do" };
            row(
                "missing file",
                &format!(
                    "{} names {}, which {verb} not exist",
                    self.short(id),
                    absent.join(", ")
                ),
            )
        }));
        lines.extend(self.lanes(&live, &deps));
        lines.extend(self.unplaceable(&live));
        lines.extend(self.epic_edges(&ids));
        lines.extend(self.reports(&live, &ids));
        lines.extend(self.moved_in(&live));
        Reply {
            exit,
            text: lines.join("\n"),
            json: Some(json!({"epic": spelled(self.epic), "lines": lines})),
            handle: None,
        }
    }

    fn forecast(&self, live: &[&Ticket]) -> Vec<String> {
        let mut remaining = WorkRemaining::default();
        for ticket in self.tickets.iter().chain(
            self.everything
                .iter()
                .find(|ticket| ticket.id.as_str() == self.epic),
        ) {
            remaining.include(ticket);
        }
        let mut lines = vec![row("agent effort", &remaining.summary())];
        let missing: Vec<&str> = live
            .iter()
            .filter(|ticket| needs_estimate(ticket))
            .map(|ticket| ticket.id.as_str())
            .collect();
        if !missing.is_empty() {
            lines.push(row("needs estimate", &self.list(&missing, "and")));
        }
        lines
    }

    /// Each live ticket's blocking dependencies on other live tickets of the epic, in order.
    fn dependencies<'a>(&'a self, ids: &HashSet<&str>) -> BTreeMap<&'a str, Vec<&'a str>> {
        let mut deps: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for edge in self.edges.iter().filter(|edge| edge.kind.blocks()) {
            if ids.contains(edge.src.as_str()) && ids.contains(edge.dst.as_str()) {
                deps.entry(edge.src.as_str())
                    .or_default()
                    .push(edge.dst.as_str());
            }
        }
        deps
    }

    /// A prerequisite outside the outermost epic cannot share its worker allocation.
    fn external_edges(&self, live: &[&Ticket]) -> Vec<String> {
        let by_id: HashMap<_, _> = self
            .everything
            .iter()
            .map(|ticket| (ticket.id.as_str(), ticket))
            .collect();
        let mut sources = HashSet::new();
        for ticket in live.iter().copied().chain(by_id.get(self.epic).copied()) {
            let mut current = Some(ticket);
            while let Some(node) = current.filter(|node| sources.insert(node.id.as_str())) {
                current = node
                    .parent
                    .as_ref()
                    .and_then(|parent| by_id.get(parent.as_str()).copied());
            }
        }
        let outer_epic = |id: &str| {
            let mut current = by_id.get(id).copied();
            let mut seen = HashSet::new();
            let mut epic = None;
            while let Some(node) = current.filter(|node| seen.insert(node.id.as_str())) {
                if node.ticket_type == TicketType::Epic {
                    epic = Some(node.id.as_str());
                }
                current = node
                    .parent
                    .as_ref()
                    .and_then(|parent| by_id.get(parent.as_str()).copied());
            }
            epic
        };
        self.edges
            .iter()
            .filter(|edge| edge.kind.blocks() && sources.contains(edge.src.as_str()))
            .filter_map(|edge| {
                let epic = outer_epic(edge.src.as_str())?;
                (outer_epic(edge.dst.as_str()) != Some(epic)).then(|| {
                    row(
                        "external edge",
                        &format!(
                            "{} depends on {} outside epic {}",
                            spelled(edge.src.as_str()),
                            spelled(edge.dst.as_str()),
                            spelled(epic)
                        ),
                    )
                })
            })
            .collect()
    }

    /// The ticket as printed: its ID after the epic's, such as `.12`, or in full with a `T`.
    fn short(&self, id: &str) -> String {
        match id.strip_prefix(self.epic) {
            Some(rest) if rest.starts_with('.') => rest.to_owned(),
            _ => spelled(id),
        }
    }

    fn list(&self, ids: &[&str], last: &str) -> String {
        let names: Vec<String> = ids.iter().map(|id| self.short(id)).collect();
        match names.split_last() {
            Some((tail, head)) if !head.is_empty() => {
                format!("{} {last} {tail}", head.join(", "))
            }
            _ => names.join(""),
        }
    }

    /// The longest run of tickets that each depend on the one before, from the one that needs
    /// nothing to the one at the end.
    fn chain(&self, live: &[&Ticket], deps: &BTreeMap<&str, Vec<&str>>) -> String {
        let mut best: HashMap<&str, Vec<&str>> = HashMap::new();
        for ticket in live {
            longest(ticket.id.as_str(), deps, &mut best, &mut HashSet::new());
        }
        let mut chains: Vec<&Vec<&str>> = best.values().collect();
        chains.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        let Some(chain) = chains.first() else {
            return "0".to_owned();
        };
        let mut path = chain.iter().rev();
        let first = path.next().map(|id| spelled(id)).unwrap_or_default();
        let rest: Vec<String> = path.map(|id| self.short(id)).collect();
        let tail = rest.iter().fold(String::new(), |mut tail, id| {
            tail.push_str(" > ");
            tail.push_str(id);
            tail
        });
        format!("{}: {first}{tail}", chain.len())
    }

    /// A dependency between two tickets that both name files and share none.
    fn suspect_edges(
        &self,
        deps: &BTreeMap<&str, Vec<&str>>,
        files: &HashMap<&str, Vec<String>>,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        for (src, targets) in deps {
            for dst in targets {
                let (a, b) = (&files[src], &files[dst]);
                if !a.is_empty() && !b.is_empty() && !share(a, b) {
                    lines.push(row(
                        "suspect edge",
                        &format!(
                            "{} depends on {}, and they name no file in common",
                            self.short(src),
                            self.short(dst)
                        ),
                    ));
                }
            }
        }
        lines
    }

    fn nameless(&self, live: &[&Ticket], files: &HashMap<&str, Vec<String>>) -> Option<String> {
        let ids: Vec<&str> = live
            .iter()
            .map(|ticket| ticket.id.as_str())
            .filter(|id| files[id].is_empty())
            .collect();
        let names: Vec<String> = ids.iter().map(|id| self.short(id)).collect();
        let verb = if ids.len() == 1 { "names" } else { "name" };
        (!ids.is_empty()).then(|| {
            row(
                "no files",
                &format!("{} {verb} no files to read", names.join(", ")),
            )
        })
    }

    /// A `lane:` group whose tickets are not one chain, where each needs the one before.
    fn lanes(&self, live: &[&Ticket], deps: &BTreeMap<&str, Vec<&str>>) -> Vec<String> {
        let mut by_label: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for ticket in live {
            for label in ticket.labels.iter().filter(|l| l.starts_with("lane:")) {
                by_label.entry(label).or_default().push(ticket.id.as_str());
            }
        }
        let mut lines = Vec::new();
        for (label, members) in by_label {
            let ordered = members.iter().enumerate().all(|(i, a)| {
                members[i + 1..]
                    .iter()
                    .all(|b| reaches(a, b, deps) || reaches(b, a, deps))
            });
            if ordered {
                continue;
            }
            let ending = if members.len() == 2 {
                "which do not depend on each other"
            } else {
                "which are not one chain"
            };
            lines.push(row(
                "lane",
                &format!("{label} holds {}, {ending}", self.list(&members, "and")),
            ));
        }
        lines
    }

    /// Reports pins that name a machine or provider the fleet does not know.
    fn unplaceable(&self, live: &[&Ticket]) -> Vec<String> {
        let by_id: HashMap<_, _> = self
            .everything
            .iter()
            .map(|ticket| (ticket.id.as_str(), ticket))
            .collect();
        live.iter()
            .flat_map(|ticket| {
                let mut pins = Pins::default();
                let mut seen = HashSet::new();
                let mut current = Some(*ticket);
                while let Some(node) = current.filter(|node| seen.insert(node.id.as_str())) {
                    let own = from_labels(&node.labels);
                    pins.machine = pins.machine.or(own.machine);
                    pins.provider = pins.provider.or(own.provider);
                    if pins.machine.is_some() && pins.provider.is_some() {
                        break;
                    }
                    current = node
                        .parent
                        .as_ref()
                        .and_then(|id| by_id.get(id.as_str()).copied());
                }
                [
                    (pins.machine, self.machines, "no machine"),
                    (pins.provider, self.providers, "no provider"),
                ]
                .into_iter()
                .filter_map(move |(pin, known, heading)| {
                    let pin = pin.filter(|pin| !known.contains(&pin.to_ascii_lowercase()))?;
                    Some(row(
                        heading,
                        &format!(
                            "{} is pinned to {pin}, which the fleet does not know",
                            self.short(ticket.id.as_str())
                        ),
                    ))
                })
            })
            .collect()
    }

    /// A dependency on a ticket that has children, which holds the dependent until every
    /// child closes.
    fn epic_edges(&self, ids: &HashSet<&str>) -> Vec<String> {
        let parents: HashSet<&str> = self
            .everything
            .iter()
            .filter(|ticket| ticket.status != Status::Closed)
            .filter_map(|ticket| ticket.parent.as_ref().map(TicketId::as_str))
            .collect();
        let mut lines = Vec::new();
        for edge in self.edges.iter().filter(|edge| edge.kind.blocks()) {
            if ids.contains(edge.src.as_str()) && parents.contains(edge.dst.as_str()) {
                lines.push(row(
                    "epic edge",
                    &format!(
                        "{} depends on the epic {}; depend on the ticket that needs its result",
                        self.short(edge.src.as_str()),
                        self.short(edge.dst.as_str())
                    ),
                ));
            }
        }
        lines
    }

    /// A ticket labeled `report` that others depend on. A report is read, never built on.
    fn reports(&self, live: &[&Ticket], ids: &HashSet<&str>) -> Vec<String> {
        let mut lines = Vec::new();
        for ticket in live
            .iter()
            .filter(|t| t.labels.iter().any(|l| l == "report"))
        {
            let dependents: Vec<&str> = self
                .edges
                .iter()
                .filter(|edge| edge.kind.blocks() && edge.dst == ticket.id)
                .map(|edge| edge.src.as_str())
                .filter(|src| ids.contains(src))
                .collect();
            if !dependents.is_empty() {
                lines.push(row(
                    "report",
                    &format!(
                        "{} is labeled report, and {} depend on it",
                        self.short(ticket.id.as_str()),
                        self.list(&dependents, "and")
                    ),
                ));
            }
        }
        lines
    }

    /// A ticket whose parent is not the one its ID was made under, when that one is an epic
    /// that is still open.
    fn moved_in(&self, live: &[&Ticket]) -> Vec<String> {
        let open: HashSet<&str> = self
            .everything
            .iter()
            .filter(|ticket| ticket.status != Status::Closed)
            .map(|ticket| ticket.id.as_str())
            .collect();
        let mut lines = Vec::new();
        for ticket in live {
            let Some(origin) = ticket.id.parent() else {
                continue;
            };
            if ticket.parent.as_ref() != Some(&origin) && open.contains(origin.as_str()) {
                lines.push(row(
                    "moved in",
                    &format!(
                        "{} was moved in from the open epic {}",
                        self.short(ticket.id.as_str()),
                        self.short(origin.as_str())
                    ),
                ));
            }
        }
        lines
    }
}

fn row(label: &str, text: &str) -> String {
    format!("{label:<LABEL_WIDTH$}{text}")
}

/// The longest chain ending at `id`, listed from `id` back to the ticket that needs nothing.
fn longest<'a>(
    id: &'a str,
    deps: &BTreeMap<&'a str, Vec<&'a str>>,
    best: &mut HashMap<&'a str, Vec<&'a str>>,
    visiting: &mut HashSet<&'a str>,
) -> Vec<&'a str> {
    if let Some(found) = best.get(id) {
        return found.clone();
    }
    // A cycle cannot make the walk loop forever.
    if !visiting.insert(id) {
        return vec![id];
    }
    let mut chain = vec![id];
    for dep in deps.get(id).into_iter().flatten() {
        let mut candidate = vec![id];
        candidate.extend(longest(dep, deps, best, visiting));
        if candidate.len() > chain.len() {
            chain = candidate;
        }
    }
    visiting.remove(id);
    best.insert(id, chain.clone());
    chain
}

/// Whether `from` depends on `to`, directly or through other tickets.
fn reaches(from: &str, to: &str, deps: &BTreeMap<&str, Vec<&str>>) -> bool {
    let mut seen = HashSet::new();
    let mut stack = vec![from];
    while let Some(id) = stack.pop() {
        for dep in deps.get(id).into_iter().flatten() {
            if *dep == to {
                return true;
            }
            if seen.insert(*dep) {
                stack.push(dep);
            }
        }
    }
    false
}

/// The paths a ticket names: backticked text without spaces that has a directory or an
/// extension, with a trailing `:line` removed, and the paths on a `Files:` line (see
/// [`named_files`]).
fn paths(ticket: &Ticket) -> Vec<String> {
    let mut found = Vec::new();
    for text in [&ticket.body, &ticket.design] {
        for (i, span) in text.split('`').enumerate() {
            if i % 2 == 1
                && let Some(path) = as_path(span)
            {
                found.push(path);
            }
        }
        found.extend(named_files(text));
    }
    found.sort();
    found.dedup();
    found
}

/// The named paths as the repository holds them, and the paths of files, with a directory, that
/// it does not hold. A path is kept as written when it is a tracked file, a directory that holds one,
/// or something on disk under the root (such as a file behind a tracked symbolic link), or when
/// several tracked files end in it; a short path that one tracked file ends in becomes that
/// file's path. A plain name the repository does not hold, such as `ticket.body`, is dropped,
/// since it is more likely code than a file. A path holding a pattern, a placeholder or a
/// variable (`*`, `<target>`, `$CACHE`) is kept but never reported. With the repository
/// unknown, every path is kept as written.
fn resolve(named: &[String], repository: Option<&Repository>) -> (Vec<String>, Vec<String>) {
    let Some(Repository { root, tracked }) = repository else {
        return (named.to_vec(), Vec::new());
    };
    let mut found = Vec::new();
    let mut absent = Vec::new();
    for path in named {
        let directory = format!("{path}/");
        let suffix = format!("/{path}");
        let literal = !path.contains(['*', '?', '<', '>', '{', '$', '~']);
        if !literal || tracked.iter().any(|t| t.starts_with(&directory)) || root.join(path).exists()
        {
            found.push(path.clone());
        } else if let Some(file) = resolve_named_file(path, tracked) {
            found.push(file);
        } else if tracked.iter().any(|t| t.ends_with(&suffix)) {
            found.push(path.clone());
        } else if path.contains('/') {
            found.push(path.clone());
            // Only a path that looks like a file is reported: one without an extension, such
            // as `origin/staging`, is as likely a branch or an ignored folder.
            let file = path
                .rsplit('/')
                .next()
                .and_then(|name| name.rsplit_once('.'));
            if file.is_some_and(|(stem, extension)| {
                !stem.trim_matches('.').is_empty()
                    && !extension.is_empty()
                    && extension.chars().all(char::is_alphanumeric)
            }) {
                absent.push(path.clone());
            }
        }
    }
    found.sort();
    found.dedup();
    (found, absent)
}

fn as_path(span: &str) -> Option<String> {
    let span = span.split(':').next().unwrap_or_default();
    let span = span.trim_end_matches(['*', '/']);
    let plain = !span.is_empty()
        && !span.starts_with('-')
        && span
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c));
    let extension = span
        .rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && (1..=5).contains(&ext.len()));
    (plain && (span.contains('/') || extension)).then(|| span.to_owned())
}

/// Whether any path of `a` is a path of `b`, or a directory holding it, or the reverse.
fn share(a: &[String], b: &[String]) -> bool {
    a.iter().any(|x| {
        b.iter()
            .any(|y| x == y || y.starts_with(&format!("{x}/")) || x.starts_with(&format!("{y}/")))
    })
}

#[cfg(test)]
mod tests;
