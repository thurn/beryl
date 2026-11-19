//! What a ticket's efficiency section shows: how its sessions spent time and dollars, read from
//! the `ticket_spend` windows that machines post (`ignite_store::hub::ticket_spend`) and shaped
//! into the strips that [`super::spend_view`] draws.
//!
//! A window's segments arrive in time order, already grouped by the producer into the colour
//! groups of [`GROUPS`]. This module builds three views of them:
//!
//! - **Timeline**: each ticket's segments in the order they happened, coalesced so that a strip
//!   holds at most [`STRIP_BUDGET`] segments, since one shorter than a pixel cannot be hovered.
//! - **By kind**: each ticket's segments added up by group, kind and part, largest first.
//! - **Merged**: an epic's whole subtree added up by kind as one ticket.
//!
//! An epic's strips stack one band per direct child that has windows in its subtree, in the
//! order the children started, each band rolling up its child's subtree. Waits on a person and idle stretches are not agent time: the timeline draws
//! each one as a fixed-height break, and the other views list them beside the strips.

use std::collections::HashMap;

use rusqlite::Connection;
use serde::Deserialize;

use ignite_core::time::Timestamp;
use ignite_store::hub::ticket_spend;

use super::data::ReadError;

/// The colour groups, in the order the legend lists them, and the label of each. The breaks
/// come last.
pub const GROUPS: [(&str, &str); 10] = [
    ("model", "Thinking"),
    ("checks", "Checks"),
    ("tools", "Tools"),
    ("ignite", "Ignite"),
    ("background", "Background"),
    ("agents", "Agents"),
    ("queued", "Queued"),
    ("other", "Other"),
    ("person", "Person"),
    ("idle", "Idle"),
];

/// The most segments a strip of the timeline holds across all its bands.
pub const STRIP_BUDGET: usize = 64;

/// The most bands an epic's strips stack; the tickets past the largest are one band.
pub const MAX_BANDS: usize = 16;

/// The most kinds a band of an epic's view by kind lists; the rest are one block.
const BAND_KINDS: usize = 8;

/// The shortest break a single ticket's timeline draws, in seconds; shorter waits are only
/// counted in the figures.
const TICKET_BREAK_S: f64 = 60.0;

/// The shortest break an epic's timeline draws, since every child's waits would otherwise stack
/// into a strip of breaks.
const EPIC_BREAK_S: f64 = 15.0 * 60.0;

/// The categories that are breaks rather than agent time.
pub fn is_break(category: &str) -> bool {
    category == "person" || category == "idle"
}

/// One segment as the producer wrote it (see the efficiency dashboard's `spend` command).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct Posted {
    c: String,
    k: String,
    g: String,
    p: String,
    t: f64,
    s: f64,
    n: u32,
    x: Option<String>,
    d: f64,
    r: u32,
    tk: Vec<f64>,
    cd: Vec<f64>,
    m: Option<String>,
    mix: Vec<(String, f64)>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PostedWindow {
    session: String,
    time: Vec<Posted>,
    dollars: Vec<Posted>,
}

/// One drawn segment: a run of one group and kind, or a coalesced run of several.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block {
    pub group: String,
    pub category: String,
    pub kind: String,
    pub part: String,
    /// Seconds in a time strip, dollars in a dollar strip.
    pub weight: f64,
    /// When the run began; `None` in the views that add runs up.
    pub at: Option<Timestamp>,
    pub calls: u32,
    pub requests: u32,
    /// Tokens and dollars by class: input, cache write, cache read, output.
    pub tokens: [f64; 4],
    pub class_dollars: [f64; 4],
    pub model: Option<String>,
    /// The command that took most of the run, shown only on the private listener.
    pub command: Option<String>,
    /// The largest parts of a coalesced run, by label.
    pub mix: Vec<(String, f64)>,
    /// How many runs this block adds up.
    pub runs: u32,
}

impl Block {
    fn key(&self) -> (&str, &str, &str, &str) {
        (&self.group, &self.category, &self.kind, &self.part)
    }

    /// Adds `other`; when `mixing`, also lists it among this block's parts.
    fn absorb(&mut self, other: &Block, mixing: bool) {
        self.weight += other.weight;
        self.calls += other.calls;
        self.requests += other.requests;
        for index in 0..4 {
            self.tokens[index] += other.tokens[index];
            self.class_dollars[index] += other.class_dollars[index];
        }
        self.runs += other.runs;
        // Only a run of the same kind lends its command: a block led by thinking that took in
        // a short `grep` did not spend its time on the grep.
        if self.command.is_none() && self.key() == other.key() {
            self.command.clone_from(&other.command);
        }
        if self.model.is_none() {
            self.model.clone_from(&other.model);
        }
        if mixing {
            // A block coalesced before lists only its largest parts; the rest of its weight
            // goes to its own name, which is its largest part's.
            let mut parts = other.mix.clone();
            let listed: f64 = parts.iter().map(|(_, weight)| weight).sum();
            if other.weight > listed {
                parts.push((label(other), other.weight - listed));
            }
            for (name, weight) in parts {
                match self.mix.iter_mut().find(|(known, _)| *known == name) {
                    Some(entry) => entry.1 += weight,
                    None => self.mix.push((name, weight)),
                }
            }
        }
    }
}

/// The name of a part of a coalesced block, as the producer named it (a kind, or a category
/// for a block with no kind), in the words [`label`] uses.
pub fn part_name(name: &str) -> String {
    match name {
        "model" => "Thinking".to_owned(),
        "reply" => "Reply".to_owned(),
        "person" => "Waiting on a person".to_owned(),
        "idle" => "Idle".to_owned(),
        _ => name.to_owned(),
    }
}

/// A block's name: its kind, or its category's name, with its part.
pub fn label(block: &Block) -> String {
    let kind = block
        .kind
        .strip_prefix("mcp__")
        .and_then(|rest| rest.rsplit("__").next())
        .unwrap_or(&block.kind);
    let name = match (block.category.as_str(), kind) {
        ("model", "reply") => "Reply".to_owned(),
        ("model", _) => "Thinking".to_owned(),
        ("person", "question") => "Question".to_owned(),
        ("person", _) => "Waiting on a person".to_owned(),
        ("idle", _) => "Idle".to_owned(),
        ("queued", kind) => format!("Queued: {kind}"),
        ("background", "") => "Background".to_owned(),
        ("background", kind) => format!("Background: {kind}"),
        (_, kind) => kind.to_owned(),
    };
    if block.part.is_empty() {
        name
    } else {
        format!("{name} · {}", block.part)
    }
}

/// One entry of a band: a segment sized by its weight, or a break drawn at a fixed height.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Segment(Block),
    Break(Block),
}

impl Item {
    pub fn block(&self) -> &Block {
        match self {
            Item::Segment(block) | Item::Break(block) => block,
        }
    }
}

/// One ticket's share of a strip.
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    /// The ticket, or empty for the merged view.
    pub ticket: String,
    pub items: Vec<Item>,
}

impl Band {
    /// The weight of its segments, leaving out breaks.
    pub fn weight(&self) -> f64 {
        self.items
            .iter()
            .filter_map(|item| match item {
                Item::Segment(block) => Some(block.weight),
                Item::Break(_) => None,
            })
            .sum()
    }
}

/// A strip of one view: time or dollars.
#[derive(Clone, Debug, PartialEq)]
pub struct Strip {
    pub bands: Vec<Band>,
}

impl Strip {
    pub fn total(&self) -> f64 {
        self.bands.iter().map(Band::weight).sum()
    }
}

/// The figures above the strips.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Totals {
    pub agent_s: f64,
    pub person_s: f64,
    pub idle_s: f64,
    pub dollars: f64,
    pub requests: u32,
    pub sessions: usize,
    pub machines: usize,
}

/// Everything the section draws.
#[derive(Clone, Debug, PartialEq)]
pub struct Spend {
    /// The tickets with windows, in the order they started, with their titles.
    pub tickets: Vec<(String, String)>,
    pub totals: Totals,
    pub timeline: (Strip, Strip),
    pub by_kind: (Strip, Strip),
    /// Only for more than one ticket.
    pub merged: Option<(Strip, Strip)>,
    /// The seconds waiting on a person and idle, which the views other than the timeline list
    /// beside the strips.
    pub breaks: Vec<(String, f64)>,
}

/// The ticket `id` and every ticket under it.
pub fn subtree(conn: &Connection, id: &str) -> Result<Vec<String>, ReadError> {
    let mut statement = conn.prepare(
        "WITH RECURSIVE under(id) AS (SELECT ?1 UNION SELECT tickets.id FROM tickets \
         JOIN under ON tickets.parent = under.id) SELECT id FROM under",
    )?;
    let rows = statement.query_map([id], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Whether any ticket in `ids` has windows. A database without the table, such as an old copy
/// in a preview, has none.
pub fn exists(conn: &Connection, ids: &[String]) -> bool {
    let wanted = serde_json::to_string(ids).unwrap_or_default();
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM json_each(?1) AS wanted \
         JOIN ticket_spend ON ticket_spend.ticket = wanted.value)",
        [wanted],
        |row| row.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

/// Reads and shapes the windows of `ids`, the subtree of the ticket `root`; `None` when there
/// are none. A window belongs to the band of the child of `root` above its ticket, or to
/// `root`'s own band.
pub fn load(conn: &Connection, root: &str, ids: &[String]) -> Result<Option<Spend>, ReadError> {
    let mut windows = ticket_spend::for_tickets(conn, ids)?;
    let parents = parents(conn, ids)?;
    for window in &mut windows {
        window.ticket = band_of(&parents, root, &window.ticket);
    }
    if windows.is_empty() {
        return Ok(None);
    }
    let Collected {
        order,
        mut time,
        mut dollars,
        totals,
    } = collect(&windows, root);
    let kept = largest(&order, &time);
    let bands = |by: &mut HashMap<String, Vec<Item>>| -> Vec<Band> {
        let mut found: Vec<Band> = Vec::new();
        let mut rest = Band {
            ticket: String::new(),
            items: Vec::new(),
        };
        for ticket in &order {
            let items = by.remove(ticket).unwrap_or_default();
            if kept.contains(ticket) {
                found.push(Band {
                    ticket: ticket.clone(),
                    items,
                });
            } else {
                rest.items.extend(items);
            }
        }
        if !rest.items.is_empty() {
            found.push(rest);
        }
        found
    };
    let time_bands = bands(&mut time);
    let dollar_bands = bands(&mut dollars);
    // Every wait counts here, including those too short for the timeline to draw.
    let breaks = [
        ("Waiting on a person", totals.person_s),
        ("Idle", totals.idle_s),
    ]
    .into_iter()
    .filter(|(_, seconds)| *seconds >= 1.0)
    .map(|(name, seconds)| (name.to_owned(), seconds))
    .collect();
    let by_kind = (kinds(&time_bands), kinds(&dollar_bands));
    let merged = (order.len() > 1).then(|| {
        let all = |bands: &[Band]| Strip {
            bands: vec![Band {
                ticket: String::new(),
                items: summed(bands.iter().flat_map(|band| band.items.iter()), false)
                    .into_iter()
                    .map(Item::Segment)
                    .collect(),
            }],
        };
        (all(&time_bands), all(&dollar_bands))
    });
    let timeline = (timeline(time_bands), timeline(dollar_bands));
    let titles = titles(conn, &order)?;
    Ok(Some(Spend {
        tickets: order
            .iter()
            .map(|id| (id.clone(), titles.get(id).cloned().unwrap_or_default()))
            .collect(),
        totals,
        timeline,
        by_kind,
        merged,
        breaks,
    }))
}

/// The tickets that keep a band of their own: all of them, or the [`MAX_BANDS`] - 1 with the
/// most agent time when there are more.
fn largest(order: &[String], time: &HashMap<String, Vec<Item>>) -> Vec<String> {
    if order.len() <= MAX_BANDS {
        return order.to_vec();
    }
    let mut weighed: Vec<(&String, f64)> = order
        .iter()
        .map(|ticket| {
            let weight = time.get(ticket).map_or(0.0, |items| {
                items
                    .iter()
                    .filter_map(|item| match item {
                        Item::Segment(block) => Some(block.weight),
                        Item::Break(_) => None,
                    })
                    .sum()
            });
            (ticket, weight)
        })
        .collect();
    weighed.sort_by(|left, right| right.1.total_cmp(&left.1));
    weighed
        .into_iter()
        .take(MAX_BANDS - 1)
        .map(|(ticket, _)| ticket.clone())
        .collect()
}

/// The windows' segments by band, in the order the bands started, and the figures.
struct Collected {
    order: Vec<String>,
    time: HashMap<String, Vec<Item>>,
    dollars: HashMap<String, Vec<Item>>,
    totals: Totals,
}

/// Reads each window's segments into its band (`window.ticket`), counting every wait in the
/// figures and keeping as breaks only those long enough to draw.
fn collect(windows: &[ticket_spend::Window], root: &str) -> Collected {
    let mut order: Vec<String> = Vec::new();
    let mut time: HashMap<String, Vec<Item>> = HashMap::new();
    let mut dollars: HashMap<String, Vec<Item>> = HashMap::new();
    let mut totals = Totals::default();
    let mut sessions: Vec<String> = Vec::new();
    let mut machines: Vec<String> = Vec::new();
    let epic = windows.iter().any(|window| window.ticket != root);
    let shortest_break = if epic { EPIC_BREAK_S } else { TICKET_BREAK_S };
    for window in windows {
        let Ok(posted) = serde_json::from_str::<PostedWindow>(&window.data) else {
            continue;
        };
        let Ok(start) = Timestamp::parse(&window.start) else {
            continue;
        };
        if !order.contains(&window.ticket) {
            order.push(window.ticket.clone());
        }
        if !sessions.contains(&posted.session) {
            sessions.push(posted.session.clone());
        }
        if !machines.contains(&window.machine) {
            machines.push(window.machine.clone());
        }
        let items = time.entry(window.ticket.clone()).or_default();
        for segment in &posted.time {
            let block = block_of(segment, start, false);
            if is_break(&block.category) {
                if block.category == "person" {
                    totals.person_s += block.weight;
                } else {
                    totals.idle_s += block.weight;
                }
                if block.weight >= shortest_break {
                    push_break(items, block);
                }
            } else {
                totals.agent_s += block.weight;
                items.push(Item::Segment(block));
            }
        }
        let items = dollars.entry(window.ticket.clone()).or_default();
        for segment in &posted.dollars {
            let block = block_of(segment, start, true);
            totals.dollars += block.weight;
            totals.requests += block.requests;
            items.push(Item::Segment(block));
        }
    }
    totals.sessions = sessions.len();
    totals.machines = machines.len();
    Collected {
        order,
        time,
        dollars,
        totals,
    }
}

/// Each ticket's parent, for the tickets `ids`.
fn parents(conn: &Connection, ids: &[String]) -> Result<HashMap<String, String>, ReadError> {
    let wanted = serde_json::to_string(ids).unwrap_or_default();
    let mut statement = conn.prepare(
        "SELECT tickets.id, tickets.parent FROM json_each(?1) AS wanted \
         JOIN tickets ON tickets.id = wanted.value WHERE tickets.parent IS NOT NULL",
    )?;
    let rows = statement.query_map([wanted], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The child of `root` that `ticket` is under, `ticket` itself if it is one, or `root`.
fn band_of(parents: &HashMap<String, String>, root: &str, ticket: &str) -> String {
    let mut current = ticket;
    // A cycle in the parents cannot be stored, but a bound keeps a bad row from hanging a page.
    for _ in 0..64 {
        match parents.get(current) {
            Some(parent) if parent == root => return current.to_owned(),
            Some(parent) => current = parent,
            None => break,
        }
    }
    root.to_owned()
}

fn titles(conn: &Connection, ids: &[String]) -> Result<HashMap<String, String>, ReadError> {
    let wanted = serde_json::to_string(ids).unwrap_or_default();
    let mut statement = conn.prepare(
        "SELECT tickets.id, tickets.title FROM json_each(?1) AS wanted \
         JOIN tickets ON tickets.id = wanted.value",
    )?;
    let rows = statement.query_map([wanted], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn block_of(segment: &Posted, start: Timestamp, dollars: bool) -> Block {
    let pair = |values: &[f64]| -> [f64; 4] {
        let mut found = [0.0; 4];
        for (slot, value) in found.iter_mut().zip(values) {
            *slot = *value;
        }
        found
    };
    Block {
        group: if segment.g.is_empty() {
            segment.c.clone()
        } else {
            segment.g.clone()
        },
        category: segment.c.clone(),
        kind: segment.k.clone(),
        part: segment.p.clone(),
        weight: if dollars { segment.d } else { segment.s }.max(0.0),
        at: Some(start.plus_seconds(segment.t)),
        calls: segment.n,
        requests: segment.r,
        tokens: pair(&segment.tk),
        class_dollars: pair(&segment.cd),
        model: segment.m.clone(),
        command: segment.x.clone(),
        mix: segment.mix.clone(),
        runs: 1,
    }
}

/// Adds a break, joining it to a break just before it.
fn push_break(items: &mut Vec<Item>, block: Block) {
    if let Some(Item::Break(last)) = items.last_mut() {
        last.absorb(&block, false);
        if block.category == "person" {
            "person".clone_into(&mut last.category);
            "person".clone_into(&mut last.group);
        }
        return;
    }
    items.push(Item::Break(block));
}

/// The blocks of `items` added up by group, category, kind and part, largest first: the breaks
/// when `breaks`, else the segments.
fn summed<'a>(items: impl Iterator<Item = &'a Item>, breaks: bool) -> Vec<Block> {
    let mut found: Vec<Block> = Vec::new();
    for item in items {
        let ((Item::Segment(block), false) | (Item::Break(block), true)) = (item, breaks) else {
            continue;
        };
        match found.iter_mut().find(|known| known.key() == block.key()) {
            Some(known) => known.absorb(block, false),
            None => found.push(Block {
                at: None,
                mix: Vec::new(),
                ..block.clone()
            }),
        }
    }
    for block in &mut found {
        block.command = None;
    }
    found.sort_by(|left, right| right.weight.total_cmp(&left.weight));
    found
}

/// Each band added up by kind. With more than one band, a band lists as many kinds as its share
/// of the strip has room for (at most [`BAND_KINDS`], at least one) and adds the rest up as one
/// block, `other kinds`; a band with room for one block shows its largest kind with every other
/// kind in its mix.
fn kinds(bands: &[Band]) -> Strip {
    let total: f64 = bands.iter().map(Band::weight).sum();
    Strip {
        bands: bands
            .iter()
            .map(|band| {
                let mut blocks = summed(band.items.iter(), false);
                let room = if bands.len() > 1 && total > 0.0 {
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        clippy::cast_precision_loss
                    )]
                    let room = (band.weight() / total * BAND_KINDS as f64 * 5.0).round() as usize;
                    room.clamp(1, BAND_KINDS)
                } else {
                    usize::MAX
                };
                if blocks.len() > room {
                    let rest = blocks.split_off(room - 1);
                    let mut folded = if blocks.is_empty() {
                        Block {
                            weight: 0.0,
                            requests: 0,
                            calls: 0,
                            tokens: [0.0; 4],
                            class_dollars: [0.0; 4],
                            runs: 0,
                            ..rest[0].clone()
                        }
                    } else {
                        Block {
                            group: "other".to_owned(),
                            category: "tool".to_owned(),
                            kind: "other kinds".to_owned(),
                            ..Block::default()
                        }
                    };
                    for block in &rest {
                        folded.absorb(block, true);
                    }
                    folded.mix.truncate(3);
                    blocks.push(folded);
                }
                Band {
                    ticket: band.ticket.clone(),
                    items: blocks.into_iter().map(Item::Segment).collect(),
                }
            })
            .collect(),
    }
}

/// The bands in time order, coalesced to the strip's budget: each band gets a share of
/// [`STRIP_BUDGET`] by its weight, and at least one segment.
fn timeline(bands: Vec<Band>) -> Strip {
    let total: f64 = bands.iter().map(Band::weight).sum();
    Strip {
        bands: bands
            .into_iter()
            .map(|band| {
                let share = if total > 0.0 {
                    band.weight() / total
                } else {
                    0.0
                };
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss
                )]
                let budget = ((STRIP_BUDGET as f64 * share).round() as usize).max(1);
                Band {
                    items: coalesced(band.items, budget),
                    ticket: band.ticket,
                }
            })
            .collect(),
    }
}

/// At most about `limit` segments: neighbouring runs of the same key become one, then runs
/// between breaks are gathered into buckets of at least 1/`limit` of the weight, each under the
/// key of its largest part. Breaks are kept as they are.
pub fn coalesced(items: Vec<Item>, limit: usize) -> Vec<Item> {
    let mut joined: Vec<Item> = Vec::with_capacity(items.len());
    for item in items {
        if let (Some(Item::Segment(last)), Item::Segment(block)) = (joined.last_mut(), &item)
            && last.key() == block.key()
        {
            last.absorb(block, false);
            continue;
        }
        joined.push(item);
    }
    let segments = joined
        .iter()
        .filter(|item| matches!(item, Item::Segment(_)))
        .count();
    if segments <= limit {
        return joined;
    }
    let total: f64 = joined
        .iter()
        .filter_map(|item| match item {
            Item::Segment(block) => Some(block.weight),
            Item::Break(_) => None,
        })
        .sum();
    #[allow(clippy::cast_precision_loss)]
    let target = total / limit as f64;
    let mut found = Vec::new();
    let mut bucket: Vec<Block> = Vec::new();
    let close = |bucket: &mut Vec<Block>, found: &mut Vec<Item>| match bucket.len() {
        0 => {}
        1 => found.push(Item::Segment(bucket.remove(0))),
        _ => {
            let mut parts: Vec<(&Block, f64)> = Vec::new();
            for block in bucket.iter() {
                match parts
                    .iter_mut()
                    .find(|(known, _)| known.key() == block.key())
                {
                    Some(entry) => entry.1 += block.weight,
                    None => parts.push((block, block.weight)),
                }
            }
            let lead = parts
                .iter()
                .max_by(|left, right| left.1.total_cmp(&right.1))
                .map(|(block, _)| *block)
                .cloned()
                .unwrap_or_default();
            let mut head = Block {
                weight: 0.0,
                calls: 0,
                requests: 0,
                tokens: [0.0; 4],
                class_dollars: [0.0; 4],
                runs: 0,
                mix: Vec::new(),
                command: None,
                at: bucket[0].at,
                ..lead
            };
            for block in bucket.iter() {
                head.absorb(block, true);
            }
            head.mix.sort_by(|left, right| right.1.total_cmp(&left.1));
            head.mix.truncate(3);
            found.push(Item::Segment(head));
            bucket.clear();
        }
    };
    for item in joined {
        match item {
            Item::Break(block) => {
                close(&mut bucket, &mut found);
                found.push(Item::Break(block));
            }
            Item::Segment(block) => {
                bucket.push(block);
                if bucket.iter().map(|block| block.weight).sum::<f64>() >= target {
                    close(&mut bucket, &mut found);
                }
            }
        }
    }
    close(&mut bucket, &mut found);
    found
}

#[cfg(test)]
#[path = "spend_tests.rs"]
mod tests;
