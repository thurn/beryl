//! Command output: every verb's text follows one contract. Changes someone else made to the
//! ticket come first, then the verdict as the first result line, then failures with a file, a
//! line and the command that reruns each alone. The whole is at most 40 lines, with the full
//! output in a file whose path is printed, and a `next` line ends it.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The most lines a command prints by default.
pub const MAX_LINES: usize = 40;

/// The most `changed` entries printed before one line points at the rest.
pub const MAX_CHANGED: usize = 5;

/// The most lines one `changed` entry takes.
const CHANGED_LINES: usize = 2;

/// The most lines of a failure's message that are printed.
const MESSAGE_LINES: usize = 3;

/// The narrowest label column: the width of `released ` and `building `.
const MIN_LABEL_WIDTH: usize = 9;

/// The longest a printed line of message text may be.
const MAX_TEXT_WIDTH: usize = 100;

/// The exit codes every verb shares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// Success.
    Success = 0,
    /// A usage or configuration error.
    Usage = 1,
    /// A check failed; the agent can fix it.
    CheckFailed = 2,
    /// A rebase conflict or a lost claim.
    Conflict = 3,
    /// The hub or the git remote cannot be reached.
    Unreachable = 4,
    /// Still running; the output names the handle to wait on.
    Running = 5,
}

impl Exit {
    pub fn code(self) -> i32 {
        self as i32
    }
}

/// One labelled line, such as `landed   3a9e4c1 on staging`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub label: String,
    pub value: String,
}

impl Line {
    pub fn new(label: &str, value: &str) -> Self {
        Self {
            label: label.to_string(),
            value: value.to_string(),
        }
    }
}

/// A comment or edit that someone else made to the session's ticket since its last command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changed {
    /// `comment` or `edit`.
    pub kind: String,
    pub author: String,
    pub text: String,
}

/// One failed check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// The check's name, such as `viewer-test:scene::badges_manual::lock_sits_top_left`.
    pub check: String,
    /// A file and line, such as `tests/all/badges_manual.rs:88`.
    pub location: Option<String>,
    /// The failure's message; only its first lines are printed.
    pub message: String,
    /// The command that runs this check alone.
    pub rerun: String,
}

/// Everything a verb has to say. [`Reply::render`] gives the text and [`Reply::to_json`] the
/// same data as one object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub exit: Exit,
    /// The ticket the reply is about, named by the pointer that follows an overflow of
    /// `changed` entries.
    pub ticket: Option<String>,
    pub changed: Vec<Changed>,
    /// The verdict first, then the rest of the result.
    pub lines: Vec<Line>,
    pub failures: Vec<Failure>,
    /// A guide topic that applies now, such as `regression`.
    pub guide: Option<String>,
    /// What to do next.
    pub next: Option<String>,
    /// The file that holds the full output.
    pub full: Option<PathBuf>,
    /// The handle of an operation still running.
    pub handle: Option<String>,
}

impl Reply {
    pub fn new(exit: Exit, lines: Vec<Line>) -> Self {
        Self {
            exit,
            ticket: None,
            changed: Vec::new(),
            lines,
            failures: Vec::new(),
            guide: None,
            next: None,
            full: None,
            handle: None,
        }
    }

    /// A reply for an operation that outlives the agent's tool timeout: the `waiting` line and
    /// `ignite wait <handle>` as the next step.
    pub fn waiting(what: &str, handle: &str) -> Self {
        let mut reply = Self::new(Exit::Running, vec![Line::new("waiting", what)]);
        reply.next = Some(format!("ignite wait {handle}"));
        reply.handle = Some(handle.to_string());
        reply
    }

    /// The reply of `ignite release`: what was saved and where, that the tree's path must not be
    /// used again, and the title to set.
    pub fn released(saved: &str, ticket: &str, tree: &str, title: &str) -> Self {
        Self::new(
            Exit::Success,
            vec![
                Line::new("saved", saved),
                Line::new(
                    "released",
                    &format!("{ticket}; {tree} is free; do not use this path again"),
                ),
                Line::new("title", title),
            ],
        )
    }

    /// The reply of `ignite close`: the ticket is closed, the tree's path must not be used
    /// again, and the title to set.
    pub fn closed(reason: &str, ticket: &str, tree: &str, title: &str) -> Self {
        Self::new(
            Exit::Success,
            vec![
                Line::new("closed", &format!("{ticket}: {reason}")),
                Line::new(
                    "released",
                    &format!("{tree} is free; do not use this path again"),
                ),
                Line::new("title", title),
            ],
        )
    }

    /// The text to print: at most [`MAX_LINES`] lines.
    pub fn render(&self) -> String {
        let width = self.label_width();
        let head = self.head_lines(width);
        let mut tail = Vec::new();
        if let Some(topic) = &self.guide {
            tail.push(labelled(
                "guide",
                &format!("ignite guide {topic}   (read it before you change anything)"),
                width,
            ));
        }
        if let Some(next) = &self.next {
            tail.extend(labelled_lines("next", next, width));
        }
        let failures: Vec<Vec<String>> = self
            .failures
            .iter()
            .map(|failure| failure_lines(failure, width))
            .collect();

        // Room for failures: what is left after the head and tail, minus one line for the
        // omission notice or the path of the full output, which is printed whenever one exists.
        let reserved = head.len() + tail.len();
        let overflow_possible = failures.iter().map(Vec::len).sum::<usize>() + reserved > MAX_LINES;
        let notice_lines = usize::from(overflow_possible || self.full.is_some());
        let mut room = MAX_LINES.saturating_sub(reserved + notice_lines);
        let mut shown = 0;
        let mut out = head;
        for block in &failures {
            if block.len() > room {
                break;
            }
            room -= block.len();
            out.extend(block.iter().cloned());
            shown += 1;
        }
        let left_out = failures.len() - shown;
        if left_out > 0 || self.full.is_some() {
            out.push(notice(left_out, self.full.as_deref(), width));
        }
        out.extend(tail);
        out.truncate(MAX_LINES);
        let mut text = out.join("\n");
        text.push('\n');
        text
    }

    /// The same data as one JSON object, with nothing cut.
    pub fn to_json(&self) -> Value {
        json!({
            "exit": self.exit.code(),
            "ticket": self.ticket,
            "changed": self.changed.iter().map(|change| json!({
                "kind": change.kind,
                "author": change.author,
                "text": change.text,
            })).collect::<Vec<_>>(),
            "lines": self.lines.iter().map(|line| json!({
                "label": line.label,
                "value": line.value,
            })).collect::<Vec<_>>(),
            "failures": self.failures.iter().map(|failure| json!({
                "check": failure.check,
                "location": failure.location,
                "message": failure.message,
                "rerun": failure.rerun,
            })).collect::<Vec<_>>(),
            "guide": self.guide,
            "next": self.next,
            "full": self.full.as_ref().map(|path| path.display().to_string()),
            "handle": self.handle,
        })
    }

    fn label_width(&self) -> usize {
        self.lines
            .iter()
            .map(|line| line.label.chars().count() + 1)
            .max()
            .unwrap_or(0)
            .max(MIN_LABEL_WIDTH)
    }

    /// The `changed` entries, then the result lines.
    fn head_lines(&self, width: usize) -> Vec<String> {
        let mut out = Vec::new();
        for change in self.changed.iter().take(MAX_CHANGED) {
            let text = format!("{} by {}: {}", change.kind, change.author, change.text);
            let mut lines: Vec<String> = text.lines().map(cut).collect();
            if lines.len() > CHANGED_LINES {
                lines.truncate(CHANGED_LINES);
                if let Some(last) = lines.last_mut() {
                    last.push('…');
                }
            }
            out.extend(labelled_lines("changed", &lines.join("\n"), width));
        }
        if self.changed.len() > MAX_CHANGED {
            let ticket = self.ticket.as_deref().unwrap_or("<ticket>");
            out.push(labelled(
                "changed",
                &format!(
                    "{} more; ignite ticket show {ticket} --comments",
                    self.changed.len() - MAX_CHANGED
                ),
                width,
            ));
        }
        for line in &self.lines {
            out.extend(labelled_lines(&line.label, &line.value, width));
        }
        out
    }
}

/// Renders labelled lines in aligned columns, cut to [`MAX_LINES`] with a pointer to `full`.
pub fn render(lines: &[Line], full: Option<&Path>) -> String {
    let mut reply = Reply::new(Exit::Success, lines.to_vec());
    reply.full = full.map(Path::to_path_buf);
    reply.render()
}

fn cut(line: &str) -> String {
    if line.chars().count() <= MAX_TEXT_WIDTH {
        line.to_string()
    } else {
        let mut cut: String = line.chars().take(MAX_TEXT_WIDTH - 1).collect();
        cut.push('…');
        cut
    }
}

fn labelled(label: &str, value: &str, width: usize) -> String {
    format!("{label:<width$}{value}")
}

/// A value of several lines: the label on the first, the rest indented under the value.
fn labelled_lines(label: &str, value: &str, width: usize) -> Vec<String> {
    value
        .lines()
        .enumerate()
        .map(|(index, text)| {
            let label = if index == 0 { label } else { "" };
            labelled(label, text, width).trim_end().to_string()
        })
        .collect()
}

fn failure_lines(failure: &Failure, width: usize) -> Vec<String> {
    let mut out = vec![labelled("FAIL", &failure.check, width)];
    let mut message = failure.message.lines().map(cut);
    let first = message.next().unwrap_or_default();
    let detail = match &failure.location {
        Some(location) if first.is_empty() => location.clone(),
        Some(location) => format!("{location}  {first}"),
        None => first,
    };
    if !detail.is_empty() {
        out.push(labelled("", &detail, width));
    }
    for line in message.take(MESSAGE_LINES - 1) {
        out.push(labelled("", &line, width));
    }
    out.push(labelled("rerun", &failure.rerun, width));
    out
}

fn notice(left_out: usize, full: Option<&Path>, width: usize) -> String {
    let path = full.map(|path| format!("; full output in {}", path.display()));
    let path = path.unwrap_or_default();
    if left_out == 0 {
        labelled("full", path.trim_start_matches("; "), width)
    } else {
        let noun = if left_out == 1 { "failure" } else { "failures" };
        labelled(
            "more",
            &format!("{left_out} more {noun} not shown{path}"),
            width,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(pairs: &[(&str, &str)]) -> Vec<Line> {
        pairs.iter().map(|(l, v)| Line::new(l, v)).collect()
    }

    #[test]
    fn start_example() {
        let mut reply = Reply::new(
            Exit::Success,
            lines(&[
                (
                    "ticket",
                    "Tq44s.9.48  3D manual table: counter names, untap lock",
                ),
                ("tree", "/work/repo/.worktrees/tree-07  at staging 8c1f2ab"),
                ("title", "⚒️ [Tq44s.9.48] Counter names, untap lock"),
                ("files", "crates/scene/src/table/badges.rs  (+4, below)"),
                (
                    "needs",
                    "Tq44s.9.31 landed 2f0c1d9: \"badges draw from one record pool\"",
                ),
            ]),
        );
        reply.next =
            Some("set the title first, then cd to the tree, read the files and edit".into());
        assert_eq!(
            reply.render(),
            "\
ticket   Tq44s.9.48  3D manual table: counter names, untap lock
tree     /work/repo/.worktrees/tree-07  at staging 8c1f2ab
title    ⚒️ [Tq44s.9.48] Counter names, untap lock
files    crates/scene/src/table/badges.rs  (+4, below)
needs    Tq44s.9.31 landed 2f0c1d9: \"badges draw from one record pool\"
next     set the title first, then cd to the tree, read the files and edit
"
        );
    }

    fn badge_failure() -> Failure {
        Failure {
            check: "viewer-test:scene::badges_manual::lock_sits_top_left".into(),
            location: Some("tests/all/badges_manual.rs:88".into()),
            message: "left: 14  right: 12".into(),
            rerun: "ignite test lock_sits_top_left".into(),
        }
    }

    #[test]
    fn test_example() {
        let mut reply = Reply::new(
            Exit::CheckFailed,
            lines(&[
                ("built", "viewer-tests in 41 s (3 crates recompiled)"),
                ("ran", "12 checks: 11 passed, 1 failed"),
            ]),
        );
        reply.failures.push(badge_failure());
        assert_eq!(
            reply.render(),
            "\
built    viewer-tests in 41 s (3 crates recompiled)
ran      12 checks: 11 passed, 1 failed
FAIL     viewer-test:scene::badges_manual::lock_sits_top_left
         tests/all/badges_manual.rs:88  left: 14  right: 12
rerun    ignite test lock_sits_top_left
"
        );
    }

    #[test]
    fn land_example() {
        let mut reply = Reply::new(
            Exit::Success,
            lines(&[
                (
                    "rebased",
                    "onto staging 91d07be (4 commits landed since start)",
                ),
                (
                    "gate",
                    "38 checks: 31 from cache, 7 run, all passed in 52 s",
                ),
                (
                    "landed",
                    "3a9e4c1 on staging; ticket closed; tree-07 released",
                ),
                ("title", "✅ [Tq44s.9.48] Counter names, untap lock"),
            ]),
        );
        reply.next = Some("nothing; wait for your next ticket".into());
        assert_eq!(
            reply.render(),
            "\
rebased  onto staging 91d07be (4 commits landed since start)
gate     38 checks: 31 from cache, 7 run, all passed in 52 s
landed   3a9e4c1 on staging; ticket closed; tree-07 released
title    ✅ [Tq44s.9.48] Counter names, untap lock
next     nothing; wait for your next ticket
"
        );
    }

    #[test]
    fn waiting_example() {
        let reply = Reply::waiting(
            "gate is queued behind 2 landings (9 minutes so far)",
            "L-5521",
        );
        assert_eq!(reply.exit.code(), 5);
        assert_eq!(
            reply.render(),
            "\
waiting  gate is queued behind 2 landings (9 minutes so far)
next     ignite wait L-5521
"
        );
        assert_eq!(reply.to_json()["handle"], "L-5521");
    }

    #[test]
    fn changed_example() {
        let mut reply = Reply::new(
            Exit::Success,
            lines(&[
                ("built", "viewer-tests in 12 s (1 crate recompiled)"),
                ("ran", "12 checks: all passed"),
            ]),
        );
        reply.changed.push(Changed {
            kind: "comment".into(),
            author: "derek".into(),
            text: "Do not wait for verification on another machine.\nLand when the gate passes."
                .into(),
        });
        assert_eq!(
            reply.render(),
            "\
changed  comment by derek: Do not wait for verification on another machine.
         Land when the gate passes.
built    viewer-tests in 12 s (1 crate recompiled)
ran      12 checks: all passed
"
        );
    }

    #[test]
    fn seven_changes_render_five_lines_and_a_pointer() {
        let mut reply = Reply::new(Exit::Success, lines(&[("ran", "ok")]));
        reply.ticket = Some("Tq44s.9.48".into());
        for index in 0..7 {
            reply.changed.push(Changed {
                kind: "comment".into(),
                author: "derek".into(),
                text: format!("note {index}"),
            });
        }
        let text = reply.render();
        let rendered: Vec<&str> = text.lines().collect();
        assert_eq!(rendered.len(), 5 + 1 + 1);
        assert_eq!(rendered[4], "changed  comment by derek: note 4");
        assert_eq!(
            rendered[5],
            "changed  2 more; ignite ticket show Tq44s.9.48 --comments"
        );
        assert_eq!(rendered[6], "ran      ok");
        assert_eq!(reply.to_json()["changed"].as_array().map(Vec::len), Some(7));
    }

    #[test]
    fn a_long_change_is_cut_to_two_lines() {
        let mut reply = Reply::new(Exit::Success, lines(&[("ran", "ok")]));
        reply.changed.push(Changed {
            kind: "edit".into(),
            author: "derek".into(),
            text: "one\ntwo\nthree".into(),
        });
        assert_eq!(
            reply.render(),
            "changed  edit by derek: one\n         two…\nran      ok\n"
        );
    }

    #[test]
    fn release_example() {
        let reply = Reply::released(
            "2 commits and 3 uncommitted files as ignite/Tq44s.7.12",
            "Tq44s.7.12",
            "tree-04",
            "⛔ [Tq44s.7.12] Sign the iOS build",
        );
        assert_eq!(
            reply.render(),
            "\
saved    2 commits and 3 uncommitted files as ignite/Tq44s.7.12
released Tq44s.7.12; tree-04 is free; do not use this path again
title    ⛔ [Tq44s.7.12] Sign the iOS build
"
        );
    }

    #[test]
    fn close_says_the_tree_path_is_not_to_be_used() {
        let text = Reply::closed("obsolete", "T1", "tree-01", "✅ [T1] x").render();
        assert!(text.contains("do not use this path again"));
        assert!(text.contains("title    ✅ [T1] x"));
    }

    #[test]
    fn guide_line_names_the_topic() {
        let mut reply = Reply::new(Exit::Success, lines(&[("ticket", "Tk3fc  Regression")]));
        reply.guide = Some("regression".into());
        assert_eq!(
            reply.render(),
            "ticket   Tk3fc  Regression\nguide    ignite guide regression   (read it before you change anything)\n"
        );
    }

    #[test]
    fn two_hundred_failures_fit_in_forty_lines_and_are_counted() {
        let mut reply = Reply::new(
            Exit::CheckFailed,
            lines(&[("ran", "200 checks: 200 failed")]),
        );
        reply.full = Some(PathBuf::from("/tmp/ignite/out-1.txt"));
        reply.next = Some("fix the first failure".into());
        for index in 0..200 {
            let mut failure = badge_failure();
            failure.check = format!("check-{index}");
            reply.failures.push(failure);
        }
        let text = reply.render();
        let rendered: Vec<&str> = text.lines().collect();
        assert!(rendered.len() <= MAX_LINES, "{} lines", rendered.len());
        assert_eq!(rendered[0], "ran      200 checks: 200 failed");
        assert_eq!(
            rendered.last().copied(),
            Some("next     fix the first failure")
        );
        let notice = rendered[rendered.len() - 2];
        let shown = rendered
            .iter()
            .filter(|line| line.starts_with("FAIL"))
            .count();
        assert_eq!(
            notice,
            format!(
                "more     {} more failures not shown; full output in /tmp/ignite/out-1.txt",
                200 - shown
            )
        );
        assert_eq!(
            reply.to_json()["failures"].as_array().map(Vec::len),
            Some(200)
        );
    }

    #[test]
    fn exit_codes_follow_the_contract() {
        let codes: Vec<i32> = [
            Exit::Success,
            Exit::Usage,
            Exit::CheckFailed,
            Exit::Conflict,
            Exit::Unreachable,
            Exit::Running,
        ]
        .iter()
        .map(|exit| exit.code())
        .collect();
        assert_eq!(codes, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn plain_render_points_at_the_full_output() {
        let text = render(
            &lines(&[("ran", "ok")]),
            Some(Path::new("/tmp/ignite/out.txt")),
        );
        assert_eq!(
            text,
            "ran      ok\nfull     full output in /tmp/ignite/out.txt\n"
        );
    }
}
