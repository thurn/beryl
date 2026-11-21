//! What the ticket views read from the hub database: ticket cards, progress counts and
//! everything a ticket's page shows.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, Row, params};
use serde_json::Value;

use ignite_core::protocol::tickets::{EdgeKind, Status};
use ignite_core::schedule;
use ignite_core::time::Timestamp;
use ignite_store::hub::{edges, schedules, tickets};

use super::references::{mentions, ticket_paths};

/// Why a page's data could not be read.
#[derive(Debug)]
pub struct ReadError(pub String);

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<rusqlite::Error> for ReadError {
    fn from(error: rusqlite::Error) -> Self {
        ReadError(error.to_string())
    }
}

impl From<ignite_store::StoreError> for ReadError {
    fn from(error: ignite_store::StoreError) -> Self {
        ReadError(error.to_string())
    }
}

/// The fields a card or row shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub id: String,
    pub parent: Option<String>,
    pub title: String,
    pub ticket_type: String,
    pub status: Status,
    pub priority: u8,
    pub assignee: Option<String>,
    pub updated_at: String,
    /// The ticket's own owner, `<machine>` or `<machine>/<app>`; `None` follows its ancestors.
    pub owner: Option<String>,
}

/// A card on the home page, which also needs the labels to place it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HomeCard {
    pub card: Card,
    pub labels: Vec<String>,
    /// The name of the machine whose worker holds the ticket, when its assignee is a worker
    /// that a machine has registered.
    pub machine: Option<String>,
}

/// How many descendants of a ticket are closed, in progress and in all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub id: String,
    pub closed: u64,
    pub working: u64,
    pub total: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentRow {
    pub author: String,
    pub at: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryRow {
    pub actor: String,
    pub at: String,
    /// The change as stored, JSON text.
    pub change: String,
}

/// The columns of a ticket its page shows.
#[derive(Clone, Debug, PartialEq)]
pub struct TicketRow {
    pub card: Card,
    pub body: String,
    pub acceptance: String,
    pub design: String,
    pub notes: String,
    pub labels: Vec<String>,
    pub metadata: Vec<(String, Value)>,
    pub close_reason: Option<String>,
    pub created_at: String,
    pub created_by: String,
    pub closed_at: Option<String>,
    /// The feed position of the ticket's last change, which an edit names as its precondition.
    pub version: u64,
    /// An epic's target number of workers and its hard cap.
    pub workers: Option<u32>,
    pub max_workers: Option<u32>,
    /// A meta-ticket's interval in hours (see `ignite_core::schedule`).
    pub every_hours: Option<u32>,
}

pub(crate) fn parse_status(text: &str) -> Status {
    serde_json::from_value(Value::String(text.to_owned())).unwrap_or_default()
}

fn status_rank(status: Status) -> u8 {
    match status {
        Status::InProgress => 0,
        Status::Blocked => 1,
        Status::Open => 2,
        Status::Deferred => 3,
        Status::PendingApproval => 4,
        Status::Closed => 5,
    }
}

/// Orders cards by what needs attention: working, blocked, open, deferred, pending approval,
/// closed; then by priority; then the most recently updated first, with ties broken by ticket ID.
pub fn by_attention(left: &Card, right: &Card) -> std::cmp::Ordering {
    status_rank(left.status)
        .cmp(&status_rank(right.status))
        .then(left.priority.cmp(&right.priority))
        .then(right.updated_at.cmp(&left.updated_at))
        .then_with(|| by_ticket_id(left, right))
}

/// Compares dotted ticket numbers numerically, so `.3` comes before `.28` and `.30`.
pub(super) fn by_ticket_id(left: &Card, right: &Card) -> std::cmp::Ordering {
    fn parts(id: &str) -> impl Iterator<Item = (u64, &str)> {
        id.split('.')
            .map(|part| (part.parse().unwrap_or(u64::MAX), part))
    }
    parts(&left.id).cmp(parts(&right.id))
}

/// Orders cards with the most recently updated first, ties broken by id. The home page sorts by
/// this so that, within each of its sections, the ticket changed last comes first.
pub fn by_recency(left: &Card, right: &Card) -> std::cmp::Ordering {
    right
        .updated_at
        .cmp(&left.updated_at)
        .then(left.id.cmp(&right.id))
}

const CARD_FIELDS: &str = "id, parent, title, type, status, priority, assignee, updated_at, owner";

fn card(row: &Row) -> rusqlite::Result<Card> {
    Ok(Card {
        id: row.get(0)?,
        parent: row.get(1)?,
        title: row.get(2)?,
        ticket_type: row.get(3)?,
        status: parse_status(&row.get::<_, String>(4)?),
        priority: row.get::<_, i64>(5)?.clamp(0, 255) as u8,
        assignee: row.get(6)?,
        updated_at: row.get(7)?,
        owner: row.get(8)?,
    })
}

fn labels_of(text: &str) -> Vec<String> {
    serde_json::from_str(text).unwrap_or_default()
}

/// The descendant counts of each of `ids`; tickets without descendants have no entry.
pub fn progress(conn: &Connection, ids: &[String]) -> Result<Vec<Progress>, ReadError> {
    let wanted = serde_json::to_string(ids).unwrap_or_default();
    let mut statement = conn.prepare(
        "WITH RECURSIVE descendants(root, id, status) AS (
           SELECT wanted.value, t.id, t.status
           FROM json_each(?1) AS wanted JOIN tickets t ON t.parent = wanted.value
           UNION ALL
           SELECT d.root, t.id, t.status FROM descendants d JOIN tickets t ON t.parent = d.id
         )
         SELECT root, COUNT(*), SUM(status = 'closed'), SUM(status = 'in_progress')
         FROM descendants GROUP BY root ORDER BY root",
    )?;
    let rows = statement.query_map([wanted], |row| {
        let count = |index: usize| {
            row.get::<_, i64>(index)
                .map(|n| u64::try_from(n).unwrap_or(0))
        };
        Ok(Progress {
            id: row.get(0)?,
            total: count(1)?,
            closed: count(2)?,
            working: count(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The open tickets the home page can show, most recently updated first, with their progress.
pub fn home_data(conn: &Connection) -> Result<(Vec<HomeCard>, Vec<Progress>), ReadError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {CARD_FIELDS}, labels,
                (SELECT m.name FROM workers w JOIN machines m ON m.id = w.machine
                 WHERE w.session = tickets.assignee LIMIT 1)
         FROM tickets
         WHERE status != 'closed'
           AND (parent IS NULL OR status IN ('in_progress', 'deferred') OR labels != '[]')"
    ))?;
    let rows = statement.query_map([], |row| {
        Ok(HomeCard {
            card: card(row)?,
            labels: labels_of(&row.get::<_, String>(9)?),
            machine: row.get(10)?,
        })
    })?;
    let mut cards: Vec<HomeCard> = rows.collect::<Result<_, _>>()?;
    cards.sort_by(|left, right| by_recency(&left.card, &right.card));
    let ids: Vec<String> = cards.iter().map(|card| card.card.id.clone()).collect();
    let counts = progress(conn, &ids)?;
    Ok((cards, counts))
}

/// A set that keeps the order its members arrived in.
#[derive(Default)]
struct Ordered {
    members: Vec<String>,
    seen: HashSet<String>,
}

impl Ordered {
    fn add(&mut self, id: &str) {
        if self.seen.insert(id.to_owned()) {
            self.members.push(id.to_owned());
        }
    }
}

/// Everything a ticket's page shows.
pub struct TicketData {
    pub ticket: TicketRow,
    pub comments: Vec<CommentRow>,
    pub history: Vec<HistoryRow>,
    pub children: Vec<Card>,
    /// The ancestors, nearest last, at most four.
    pub chain: Vec<String>,
    /// Each ticket name found in the text, and the ID it names.
    pub links: Vec<(String, String)>,
    /// The cards of every ticket the page mentions.
    pub references: Vec<Card>,
    /// Every ticket whose change should refresh the page.
    pub watched: Vec<String>,
    /// `Blocked by`, `Blocking` and `Related`, each with the IDs in it.
    pub groups: Vec<(&'static str, Vec<String>)>,
    pub counts: Vec<Progress>,
    /// Whether the ticket or any ticket under it has efficiency windows, which the page shows
    /// in its efficiency section (see [`super::spend`]).
    pub spend: bool,
    /// When a meta-ticket files its next copy; `None` for any other ticket.
    pub next_copy: Option<NextCopy>,
    /// The ticket's counted releases, which hold it back from the pool once different sessions
    /// made them (see `ignite_store::hub::releases`).
    pub releases: ignite_store::hub::releases::Releases,
    /// The ticket's image attachments, oldest first, removed ones included.
    pub attachments: Vec<ignite_core::protocol::attachments::Attachment>,
    /// The CLI worker sessions with a transcript on the ticket, oldest first (see
    /// [`super::session`]).
    pub sessions: Vec<String>,
}

/// When a meta-ticket files its next copy, as its page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NextCopy {
    /// The meta-ticket is not open, so its schedule does not run.
    Paused,
    /// The next copy falls due at this RFC 3339 time; the hub files it within a minute after.
    At(String),
    /// A copy is still live, and the next falls due at the first due time after it closes.
    AfterLiveCopy,
}

/// When the meta-ticket in `ticket` files its next copy; `None` when it is not a meta-ticket.
fn next_copy(conn: &Connection, ticket: &TicketRow) -> Result<Option<NextCopy>, ReadError> {
    let Some(every) = ticket
        .every_hours
        .filter(|_| ticket.card.ticket_type == "meta")
    else {
        return Ok(None);
    };
    if ticket.card.status != Status::Open || every == 0 {
        return Ok(Some(NextCopy::Paused));
    }
    let created = Timestamp::parse(&ticket.created_at)
        .map_err(|error| ReadError(format!("created_at: {error}")))?;
    let last = schedules::last_copy(conn, &ticket.card.id)?;
    Ok(Some(match schedule::due_at(created, every, last) {
        Some(due) => NextCopy::At(due.to_string()),
        None => NextCopy::AfterLiveCopy,
    }))
}

/// One ticket by exact ID.
pub fn ticket_row(conn: &Connection, id: &str) -> Result<Option<TicketRow>, ReadError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {CARD_FIELDS}, body, acceptance, design, notes, labels, metadata, close_reason,
                created_at, created_by, closed_at, version, workers, max_workers, every_hours
         FROM tickets WHERE id = ?1"
    ))?;
    let mut rows = statement.query_map([id], |row| {
        let metadata: String = row.get(14)?;
        Ok(TicketRow {
            card: card(row)?,
            body: row.get(9)?,
            acceptance: row.get(10)?,
            design: row.get(11)?,
            notes: row.get(12)?,
            labels: labels_of(&row.get::<_, String>(13)?),
            metadata: ordered_object(&metadata),
            close_reason: row.get(15)?,
            created_at: row.get(16)?,
            created_by: row.get(17)?,
            closed_at: row.get(18)?,
            version: u64::try_from(row.get::<_, i64>(19)?).unwrap_or_default(),
            workers: row.get(20)?,
            max_workers: row.get(21)?,
            every_hours: row.get(22)?,
        })
    })?;
    Ok(rows.next().transpose()?)
}

/// The members of a JSON object in the order they were written; empty if it is not one.
pub fn ordered_object(text: &str) -> Vec<(String, Value)> {
    serde_json::from_str::<super::history::Ordered>(text)
        .map(|object| object.0)
        .unwrap_or_default()
}

/// The cards of the tickets `ids` that exist, by ID.
pub fn cards_by_id(conn: &Connection, ids: &[&str]) -> Result<HashMap<String, Card>, ReadError> {
    let mut cards = HashMap::new();
    for id in ids {
        for card in cards_where(conn, "id = ?1", id)? {
            cards.insert(card.id.clone(), card);
        }
    }
    Ok(cards)
}

fn cards_where(conn: &Connection, clause: &str, argument: &str) -> Result<Vec<Card>, ReadError> {
    let mut statement =
        conn.prepare(&format!("SELECT {CARD_FIELDS} FROM tickets WHERE {clause}"))?;
    let rows = statement.query_map([argument], card)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Reads a ticket's page data. `project` is the name that ticket names in text may carry.
#[allow(clippy::too_many_lines)]
pub fn ticket_data(
    conn: &Connection,
    project: &str,
    ticket: TicketRow,
) -> Result<TicketData, ReadError> {
    let id = ticket.card.id.clone();
    let comments = comments(conn, &id)?;
    let history = history_rows(conn, &id)?;
    let mut children = cards_where(conn, "parent = ?1", &id)?;
    children.sort_by(by_attention);
    // Milestones come first, in plan order; the subtasks follow in attention order.
    let (mut milestones, subtasks): (Vec<Card>, Vec<Card>) = children
        .into_iter()
        .partition(super::milestones::is_milestone);
    super::milestones::order(&mut milestones, &sibling_dependencies(conn, &id)?);
    milestones.extend(subtasks);
    let children = milestones;
    let edge_list = edges::of(conn, &id)?;

    let mut ids = Ordered::default();
    for child in &children {
        ids.add(&child.id);
    }
    let mut sections: [(&'static str, Ordered); 3] = [
        ("Blocked by", Ordered::default()),
        ("Blocking", Ordered::default()),
        ("Related", Ordered::default()),
    ];
    for edge in &edge_list {
        let outgoing = edge.src.as_str() == id;
        let other = if outgoing {
            edge.dst.as_str()
        } else {
            edge.src.as_str()
        };
        let group = match edge.kind {
            EdgeKind::Blocks | EdgeKind::ConditionalBlocks | EdgeKind::WaitsFor => {
                usize::from(!outgoing)
            }
            _ => 2,
        };
        sections[group].1.add(other);
        ids.add(other);
    }

    let mut chain: Vec<String> = Vec::new();
    let mut current = ticket.card.parent.clone();
    while let Some(parent) = current.take() {
        if chain.len() >= 4 || chain.contains(&parent) {
            break;
        }
        ids.add(&parent);
        current = conn
            .query_row(
                "SELECT parent FROM tickets WHERE id = ?1",
                [&parent],
                |row| row.get::<_, Option<String>>(0),
            )
            .unwrap_or(None);
        chain.push(parent);
    }
    chain.reverse();

    let mut links: Vec<(String, String)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut collect = |name: &str| -> String {
        if seen.insert(name.to_owned())
            && let Ok(Some(target)) = tickets::resolve(conn, name)
        {
            ids.add(target.as_str());
            links.push((name.to_owned(), target.to_string()));
        }
        name.to_owned()
    };
    let sources = [
        &ticket.body,
        &ticket.acceptance,
        &ticket.design,
        &ticket.notes,
    ]
    .into_iter()
    .map(String::as_str)
    .chain(ticket.close_reason.as_deref())
    .chain(comments.iter().map(|comment| comment.text.as_str()))
    .chain(history.iter().map(|row| row.change.as_str()));
    for source in sources {
        mentions(project, source, &mut collect);
        ticket_paths(project, source, &mut |name, _| collect(name));
    }

    let wanted = serde_json::to_string(&ids.members).unwrap_or_default();
    let mut statement = conn.prepare(&format!(
        "SELECT {} FROM json_each(?1) AS wanted JOIN tickets ON tickets.id = wanted.value ORDER BY tickets.id",
        CARD_FIELDS.split(", ").map(|column| format!("tickets.{column}")).collect::<Vec<_>>().join(", ")
    ))?;
    let references: Vec<Card> = statement
        .query_map(params![wanted], card)?
        .collect::<Result<_, _>>()?;
    let reference_ids: Vec<String> = references.iter().map(|card| card.id.clone()).collect();
    let counts = progress(conn, &reference_ids)?;
    let groups = sections
        .into_iter()
        .map(|(name, set)| (name, set.members))
        .collect();
    Ok(TicketData {
        comments,
        history,
        children,
        chain,
        links,
        references,
        watched: ids.members,
        groups,
        counts,
        spend: super::spend::exists(conn, &super::spend::subtree(conn, &id)?),
        next_copy: next_copy(conn, &ticket)?,
        releases: ignite_store::hub::releases::of(conn, &id)?,
        attachments: ignite_store::hub::attachments::list(conn, &id)?,
        sessions: super::session::sessions_of(conn, &id)?,
        ticket,
    })
}

/// The `(dependent, dependency)` pairs between children of `parent`, from the edges that
/// hold a ticket back (`blocks`, `conditional-blocks` and `waits-for`).
fn sibling_dependencies(
    conn: &Connection,
    parent: &str,
) -> Result<Vec<(String, String)>, ReadError> {
    let mut statement = conn.prepare(
        "SELECT edges.src, edges.dst FROM edges
         JOIN tickets AS dependent ON dependent.id = edges.src
         JOIN tickets AS dependency ON dependency.id = edges.dst
         WHERE dependent.parent = ?1 AND dependency.parent = ?1
           AND edges.kind IN ('blocks', 'conditional-blocks', 'waits-for')",
    )?;
    let rows = statement.query_map([parent], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn comments(conn: &Connection, id: &str) -> Result<Vec<CommentRow>, ReadError> {
    let mut statement =
        conn.prepare("SELECT author, at, text FROM comments WHERE ticket = ?1 ORDER BY n")?;
    let rows = statement.query_map([id], |row| {
        Ok(CommentRow {
            author: row.get(0)?,
            at: row.get(1)?,
            text: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn history_rows(conn: &Connection, id: &str) -> Result<Vec<HistoryRow>, ReadError> {
    let mut statement =
        conn.prepare("SELECT actor, at, change FROM history WHERE ticket = ?1 ORDER BY seq DESC")?;
    let rows = statement.query_map([id], |row| {
        Ok(HistoryRow {
            actor: row.get(0)?,
            at: row.get(1)?,
            change: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
