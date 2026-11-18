//! The text of `ignite start`, `ignite brief`, `ignite release` and `ignite close`.
//!
//! Each line is a label padded to one column and a value, the verdict first, as every verb
//! prints:
//!
//! ```text
//! ticket   Tq44s.9.48  Manual table: counter names, untap lock
//! tree     /trees/tree-07  at staging 8c1f2ab
//! title    ⚒️ [Tq44s.9.48] Manual table: counter names, untap lock
//! files    crates/scene/src/table/badges.rs
//! needs    Tq44s.9.31 landed 2f0c1d9: "badges draw from one record pool"
//! next     cd to the tree, set the title, read the files, then edit
//! ```

use std::fmt::Write as _;
use std::path::Path;

use serde_json::{Value, json};

use ignite_core::ids::{Commit, Handle, TicketId, TreeId};
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::hub::{DependencyNote, TicketBrief};
use ignite_core::protocol::tickets::Ticket;
use ignite_core::traits::Error;

use super::{Resumed, Saved};

/// The emoji of a session's title while it works its ticket.
const WORKING: &str = "⚒️";
/// The emoji while it works a regression ticket: a check that tip verification found failing,
/// which holds `master` back until it is fixed.
const FIREFIGHTING: &str = "🚒";
/// The emoji once its ticket is landed or closed.
const DONE: &str = "✅";
/// The emoji once it has given its ticket back unfinished.
const STOPPED: &str = "⛔";

/// The most named files the brief lists; the rest are counted.
const MAX_FILES: usize = 6;
/// The most dependencies the brief lists; the rest are counted.
const MAX_NEEDS: usize = 8;

/// The labels that call for a guide topic, and the topic. The first match wins.
const GUIDES: &[(&str, &str)] = &[
    ("regression", "regression"),
    ("review", "review"),
    ("review:inline", "review"),
    ("audit", "audit"),
    ("docs", "docs"),
    ("machine", "machine"),
    ("checks", "checks"),
    ("flaky", "checks"),
];

/// The tree as the brief shows it.
pub struct TreeView<'a> {
    pub tree: &'a TreeId,
    pub path: &'a Path,
    pub branch: &'a str,
    /// The tip the tree was leased at, when known.
    pub at: Option<&'a Commit>,
    pub resumed: Option<&'a Resumed>,
    /// The integration branch's name, from `ignite.toml`.
    pub staging: &'a str,
}

/// Aligns each label to one column.
fn lines(rows: &[(&str, String)]) -> String {
    let mut text = String::new();
    for (label, value) in rows {
        let line = format!("{label:<8} {value}");
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

fn short(commit: &Commit) -> &str {
    let text = commit.as_str();
    &text[..text.len().min(7)]
}

/// The first line of a text, trimmed.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

/// The repository paths a ticket names in backquotes, in its body and then its acceptance.
pub fn named_files(ticket: &Ticket) -> Vec<String> {
    let mut files = ignite_hub::routes::dispatch::named_files(&ticket.body);
    for file in ignite_hub::routes::dispatch::named_files(&ticket.acceptance) {
        if !files.contains(&file) {
            files.push(file);
        }
    }
    files
}

/// The session title for a ticket: `[<ticket>] <title>` without the emoji.
pub fn title_of(brief: Option<&TicketBrief>, ticket: &TicketId) -> String {
    match brief {
        Some(brief) => format!("[{ticket}] {}", first_line(&brief.ticket.title)),
        None => format!("[{ticket}]"),
    }
}

/// The emoji of the title while `ticket` is worked: 🚒 for a regression ticket, else ⚒️.
fn working(ticket: &Ticket) -> &'static str {
    if ticket.labels.iter().any(|l| l == "regression") {
        FIREFIGHTING
    } else {
        WORKING
    }
}

/// The guide topic the ticket's labels call for.
fn guide(ticket: &Ticket) -> Option<&'static str> {
    GUIDES
        .iter()
        .find(|(label, _)| ticket.labels.iter().any(|l| l == label))
        .map(|(_, topic)| *topic)
}

fn need_line(note: &DependencyNote) -> (String, Option<String>) {
    match &note.landed {
        Some(work) => {
            let commit = work.commits.last().map_or("", short);
            let files = (!work.files.is_empty()).then(|| {
                let shown: Vec<&str> = work
                    .files
                    .iter()
                    .take(MAX_FILES)
                    .map(String::as_str)
                    .collect();
                let more = work.files.len().saturating_sub(MAX_FILES);
                let more = if more > 0 {
                    format!(" (+{more})")
                } else {
                    String::new()
                };
                format!("  touched {}{more}", shown.join(", "))
            });
            (
                format!(
                    "{} landed {commit}: \"{}\"",
                    note.ticket,
                    first_line(&work.note)
                ),
                files,
            )
        }
        None => match &note.close_reason {
            Some(reason) => (
                format!("{} closed: \"{}\"", note.ticket, first_line(reason)),
                None,
            ),
            None => (format!("{} {}", note.ticket, first_line(&note.title)), None),
        },
    }
}

fn resumed_line(resumed: &Resumed) -> String {
    let commits = if resumed.commits == 1 {
        "commit"
    } else {
        "commits"
    };
    let mut line = format!("{} {commits}", resumed.commits);
    if !resumed.authors.is_empty() {
        let _ = write!(line, " by {}", resumed.authors.join(", "));
    }
    if resumed.last_saved {
        line.push_str("; the last is unfinished work saved at release");
    }
    line
}

/// The brief that `ignite start` and `ignite brief` print.
pub fn render(brief: &TicketBrief, tree: &TreeView) -> String {
    let ticket = &brief.ticket;
    let mut rows: Vec<(&str, String)> = Vec::new();
    rows.push((
        "ticket",
        format!("{}  {}", ticket.id, first_line(&ticket.title)),
    ));
    let at = match tree.at {
        Some(at) => format!("  at {} {}", tree.staging, short(at)),
        None => String::new(),
    };
    rows.push(("tree", format!("{}{at}", tree.path.display())));
    if let Some(resumed) = tree.resumed {
        rows.push((
            "resumes",
            format!("{}: {}", tree.branch, resumed_line(resumed)),
        ));
    }
    rows.push((
        "title",
        format!("{} {}", working(ticket), title_of(Some(brief), &ticket.id)),
    ));
    let files = named_files(ticket);
    for (i, file) in files.iter().take(MAX_FILES).enumerate() {
        rows.push((if i == 0 { "files" } else { "" }, file.clone()));
    }
    if files.len() > MAX_FILES {
        let more = files.len() - MAX_FILES;
        rows.push(("", format!("(+{more} more in the ticket)")));
    }
    for note in brief.needs.iter().take(MAX_NEEDS) {
        let (line, touched) = need_line(note);
        rows.push(("needs", line));
        if let Some(touched) = touched {
            rows.push(("", touched));
        }
    }
    if brief.needs.len() > MAX_NEEDS {
        let more = brief.needs.len() - MAX_NEEDS;
        rows.push((
            "needs",
            format!("(+{more} more: ignite ticket show {})", ticket.id),
        ));
    }
    if let Some(topic) = guide(ticket) {
        rows.push((
            "guide",
            format!("ignite guide {topic}   (read it before you change anything)"),
        ));
    }
    rows.push((
        "next",
        "cd to the tree, set the title, read the files, then edit".to_owned(),
    ));
    lines(&rows)
}

/// The brief as one JSON object, for `--json`.
pub fn json(brief: &TicketBrief, tree: &TreeView) -> Value {
    json!({
        "ticket": brief.ticket,
        "needs": brief.needs,
        "tree": tree.tree,
        "path": tree.path,
        "branch": tree.branch,
        "at": tree.at,
        "resumed": tree.resumed.map(|r| json!({
            "commits": r.commits, "authors": r.authors, "last_saved": r.last_saved,
        })),
        "files": named_files(&brief.ticket),
        "guide": guide(&brief.ticket),
        "title": format!(
            "{} {}",
            working(&brief.ticket),
            title_of(Some(brief), &brief.ticket.id)
        ),
    })
}

/// The reply when every ticket tree is leased.
pub fn waiting_for_tree(handle: Handle) -> Reply {
    let text = lines(&[
        (
            "waiting",
            "every ticket tree is leased; one is freed at the next landing or release".to_owned(),
        ),
        (
            "next",
            format!("ignite wait {handle}, then run this ignite start again"),
        ),
    ]);
    Reply {
        exit: Exit::Running,
        text,
        json: Some(json!({"waiting": "tree", "handle": handle})),
        handle: Some(handle),
    }
}

fn saved_line(saved: &Saved) -> String {
    let commits = if saved.commits == 1 {
        "commit"
    } else {
        "commits"
    };
    let mut line = format!("{} {commits}", saved.commits);
    if !saved.uncommitted.is_empty() {
        let files = if saved.uncommitted.len() == 1 {
            "file"
        } else {
            "files"
        };
        let _ = write!(line, " and {} uncommitted {files}", saved.uncommitted.len());
    }
    format!("{line} as {}", saved.branch)
}

/// The reply to a release that went through.
pub fn released(
    ticket: &TicketId,
    tree: &TreeId,
    saved: &Saved,
    title: &str,
    reset: Option<Error>,
) -> Reply {
    let mut rows = vec![
        ("saved", saved_line(saved)),
        (
            "released",
            format!("{ticket}; {tree} is free; do not use this path again"),
        ),
    ];
    if let Some(error) = reset {
        rows.push(("reset", format!("{tree} failed: {error}")));
    }
    rows.push(("title", format!("{STOPPED} {title}")));
    Reply {
        exit: Exit::Success,
        text: lines(&rows),
        json: Some(json!({
            "released": ticket, "tree": tree, "branch": saved.branch,
            "commits": saved.commits, "uncommitted": saved.uncommitted,
        })),
        handle: None,
    }
}

/// The reply to a close that went through.
pub fn closed(
    ticket: &TicketId,
    tree: &TreeId,
    reason: &str,
    title: &str,
    dropped: Option<Error>,
    reset: Option<Error>,
) -> Reply {
    let mut rows = vec![(
        "closed",
        format!(
            "{ticket}: {}; {tree} is free; do not use this path again",
            first_line(reason)
        ),
    )];
    if let Some(error) = dropped {
        rows.push((
            "branch",
            format!("the saved branch was not deleted: {error}"),
        ));
    }
    if let Some(error) = reset {
        rows.push(("reset", format!("{tree} failed: {error}")));
    }
    rows.push(("title", format!("{DONE} {title}")));
    rows.push(("next", "nothing; wait for your next ticket".to_owned()));
    Reply {
        exit: Exit::Success,
        text: lines(&rows),
        json: Some(json!({"closed": ticket, "tree": tree, "reason": reason})),
        handle: None,
    }
}
