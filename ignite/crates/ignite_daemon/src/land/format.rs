//! Formatting at landing: before its first gate or express run, `ignite land` runs the
//! formatters of `[format]` in `ignite.toml` on the files the landing changes, and commits what
//! they changed as a commit of its own (see "Formatting at landing" in the design document).
//!
//! On 2026-11-18 workers on one epic lost about 25 minutes to formatting failures found one
//! Ignite round trip at a time: two `ignite test` runs failed `cargo fmt --check` after 512 and
//! 759 s, and a landing failed the Markdown format check on the design document. A formatter
//! fixes in a second what those checks only report, so the landing runs it first.
//!
//! The formatters run in the ticket tree, at the rebased commit, once per landing: a later pass
//! rebases the format commit with the rest of the branch. A rule runs when any file the landing
//! changes matches it, and its `{files}` word becomes those files. A formatter that fails
//! (missing from the machine, or unable to parse a file) is reported on the landing's `format`
//! line and the landing goes on: the gate or the express commands then say what is wrong, as
//! they did before formatting at landing existed. Ignite's code names no formatter; every
//! command comes from `ignite.toml`.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use ignite_core::config::Config;
use ignite_core::formatting::FormatRule;
use ignite_core::ids::Commit;
use ignite_core::protocol::client::Reply;

use super::env::TicketTree;
use super::{Landing, Rebased, Stop, line, short};
use crate::checks::verbs::render;
use crate::server::{Context, error_reply};
use crate::steps;

/// The subject of the commit that holds what the formatters changed.
pub const SUBJECT: &str = "style: format";

/// How many lines of a failed formatter's output its warning keeps.
const TAIL_LINES: usize = 5;

/// The formatters as a landing uses them. Tests replace them with a fake.
pub trait Formatter: Send + Sync {
    /// Runs every rule that matches one of `changed` in `tree`, then commits what they changed
    /// to the tree's branch.
    fn format(&self, tree: &Path, changed: &[String]) -> Formatted;
}

/// What the formatters did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Formatted {
    /// The commands that ran, in order.
    pub ran: Vec<String>,
    /// The commands that failed, each with the end of its output.
    pub failed: Vec<(String, String)>,
    /// The commit that holds what they changed; `None` when they changed nothing.
    pub commit: Option<Commit>,
}

impl Formatted {
    /// The landing's `format` line, or `None` when no rule matched.
    pub fn line(&self) -> Option<(String, String)> {
        if self.ran.is_empty() {
            return None;
        }
        let what = match &self.commit {
            Some(commit) => format!("committed {} `{SUBJECT}`", short(commit)),
            None => "nothing to change".to_owned(),
        };
        Some(line(
            "format",
            format!("{} formatters ran: {what}", self.ran.len()),
        ))
    }

    /// A `warning` line for each formatter that failed.
    pub fn warnings(&self) -> Vec<(String, String)> {
        self.failed
            .iter()
            .map(|(command, why)| {
                line(
                    "warning",
                    format!("the formatter `{command}` failed, so the checks may: {why}"),
                )
            })
            .collect()
    }

    /// `reply` with the `format` line and warnings added, for a landing that ends early: the
    /// format commit stays on the tree's branch, and the agent needs to know it is there.
    pub fn onto(&self, mut reply: Reply) -> Reply {
        let mut lines: Vec<_> = self.line().into_iter().collect();
        lines.extend(self.warnings());
        if !lines.is_empty() {
            reply.text = format!("{}\n{}", render(&lines).trim_end(), reply.text);
        }
        reply
    }
}

/// The formatters of `[format]`, run as child processes of the landing.
pub struct MachineFormat {
    pub rules: Vec<FormatRule>,
}

impl MachineFormat {
    /// The repository's formatters, or `None` when `[format]` has no rules.
    pub fn for_config(config: &Config) -> Option<Arc<dyn Formatter>> {
        if config.format.rules.is_empty() {
            return None;
        }
        Some(Arc::new(MachineFormat {
            rules: config.format.rules.clone(),
        }))
    }
}

impl MachineFormat {
    /// Runs every rule that matches one of `changed` in `tree` and leaves what they changed in
    /// the working tree, uncommitted. `Formatted::commit` stays `None`.
    pub fn run_rules(&self, tree: &Path, changed: &[String]) -> Formatted {
        let mut done = Formatted::default();
        for rule in &self.rules {
            // A deleted file matches the rule but cannot be given to the formatter.
            let files: Vec<String> = changed
                .iter()
                .filter(|file| rule.matches(file) && tree.join(file).is_file())
                .cloned()
                .collect();
            if files.is_empty() {
                continue;
            }
            steps::step(format!("formatting with `{}`", rule.command));
            done.ran.push(rule.command.clone());
            if let Err(why) = run_one(&rule.words(&files), &tree.join(&rule.dir)) {
                done.failed.push((rule.command.clone(), why));
            }
        }
        done
    }

    /// Runs the rules as [`Self::run_rules`] does, in an agent's tree before `ignite test` or
    /// `ignite check` runs the express commands, and returns which of `changed` the formatters
    /// rewrote. A rule for a whole workspace (`cargo fmt`) may also rewrite files outside
    /// `changed`, which are not listed.
    ///
    /// On 2026-11-19 two Ignite fixes each lost a round trip or two to `ignite test` and
    /// `ignite check` failing on `cargo fmt --check` or the Markdown check, though the landing
    /// would have formatted the same files itself.
    pub fn format_in_place(&self, tree: &Path, changed: &[String]) -> (Formatted, Vec<String>) {
        let read = |file: &String| std::fs::read(tree.join(file)).ok();
        let before: Vec<_> = changed.iter().map(read).collect();
        let done = self.run_rules(tree, changed);
        let rewritten = changed
            .iter()
            .zip(before)
            .filter(|(file, before)| read(file) != *before)
            .map(|(file, _)| file.clone())
            .collect();
        (done, rewritten)
    }
}

impl Formatter for MachineFormat {
    fn format(&self, tree: &Path, changed: &[String]) -> Formatted {
        let mut done = self.run_rules(tree, changed);
        if done.ran.is_empty() {
            return done;
        }
        match ignite_git::commit::tracked_changes(tree, SUBJECT) {
            Ok(commit) => done.commit = commit,
            Err(error) => done.failed.push((
                "committing what the formatters changed".to_owned(),
                error.to_string(),
            )),
        }
        done
    }
}

/// Runs one formatter in `dir`; `Err` holds the end of its output.
fn run_one(words: &[String], dir: &Path) -> Result<(), String> {
    let (program, args) = words.split_first().ok_or("an empty command")?;
    let out = Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot start it: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let text =
        String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(TAIL_LINES)..].join(" / ");
    Err(if tail.is_empty() {
        format!("it exited with {}", out.status)
    } else {
        tail
    })
}

impl Landing {
    /// Runs the formatters on `changed` in the ticket tree, at `rebased.head`, and moves
    /// `rebased.head` and `changed` to the format commit when they made one. `None` when the
    /// machine has no formatters.
    pub(super) fn format_changes(
        &self,
        ctx: &Context,
        held: &TicketTree,
        rebased: &mut Rebased,
        changed: &mut Vec<String>,
    ) -> Result<Option<Formatted>, Stop> {
        let Some(formatters) = &self.format else {
            return Ok(None);
        };
        let formatted = formatters.format(&held.path, changed);
        if let Some(commit) = &formatted.commit {
            rebased.head = commit.clone();
            // A formatter of a whole workspace may also change files the landing did not.
            *changed = ctx
                .git
                .changed(&rebased.onto, &rebased.head)
                .map_err(|e| Box::new(error_reply(&e)))?;
        }
        Ok(Some(formatted))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use super::*;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn rule(paths: &str, dir: &str, command: &str) -> FormatRule {
        FormatRule {
            paths: vec![paths.to_owned()],
            exclude: Vec::new(),
            dir: dir.to_owned(),
            command: command.to_owned(),
        }
    }

    /// A repository with a misformatted Rust file under `tool/` and a misformatted Markdown
    /// file, both committed, and formatters that stand in for rustfmt and mdformat by
    /// rewriting each file they are given.
    fn repository(name: &str) -> std::path::PathBuf {
        let repo = ignite_core::test_files::temp_dir()
            .join(format!("ignite-format-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&repo);
        fs::create_dir_all(repo.join("tool/src")).unwrap();
        git(&repo, &["init", "--quiet", "-b", "main"]);
        git(&repo, &["config", "user.name", "t"]);
        git(&repo, &["config", "user.email", "t@example.com"]);
        fs::write(repo.join("tool/src/lib.rs"), "fn  a() {}\n").unwrap();
        fs::write(repo.join("notes.md"), "*  item\n").unwrap();
        // The Unix fixture uses POSIX shell redirection and utilities.
        #[cfg(unix)]
        fs::write(
            repo.join("fmt.sh"),
            "for f in \"$@\"; do tr -s ' ' < \"$f\" > \"$f.tmp\" && mv \"$f.tmp\" \"$f\"; done\n",
        )
        .unwrap();
        #[cfg(windows)]
        fs::write(
            repo.join("fmt.cmd"),
            "@echo off\r\nfor %%F in (%*) do (\r\n  if %%~xF==.rs (\r\n    >\"%%~F\" echo fn a^(^) {}\r\n  ) else (\r\n    >\"%%~F\" echo * item\r\n  )\r\n)\r\n",
        )
        .unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "--quiet", "-m", "misformatted"]);
        repo
    }

    const NOTES: &str = if cfg!(windows) {
        "cmd /D /C fmt.cmd {files}"
    } else {
        "sh fmt.sh {files}"
    };

    #[test]
    fn misformatted_rust_and_markdown_files_are_formatted_and_committed() {
        let repo = repository("dirty");
        let rust = if cfg!(windows) {
            "cmd /D /C ..\\fmt.cmd {files}"
        } else {
            "sh ../fmt.sh {files}"
        };
        let format = MachineFormat {
            rules: vec![
                rule("tool/**/*.rs", "tool", rust),
                rule("**/*.md", "", NOTES),
                rule("ui/**", "ui", "no-such-formatter {files}"),
            ],
        };
        let changed = ["tool/src/lib.rs".to_owned(), "notes.md".to_owned()];

        let formatted = format.format(&repo, &changed);

        assert_eq!(formatted.ran.len(), 2, "{formatted:?}");
        assert!(formatted.failed.is_empty(), "{formatted:?}");
        let commit = formatted.commit.clone().expect("a format commit");
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]), commit.as_str());
        assert_eq!(git(&repo, &["log", "-1", "--format=%s"]), SUBJECT);
        assert_eq!(
            fs::read_to_string(repo.join("tool/src/lib.rs"))
                .unwrap()
                .trim_end(),
            "fn a() {}"
        );
        assert_eq!(
            fs::read_to_string(repo.join("notes.md"))
                .unwrap()
                .trim_end(),
            "* item"
        );
        assert_eq!(git(&repo, &["status", "--porcelain"]), "");
        let (label, text) = formatted.line().unwrap();
        assert_eq!(label, "format");
        assert!(text.starts_with("2 formatters ran: committed"), "{text}");

        // Run again, the formatters find nothing to change and nothing is committed.
        let again = format.format(&repo, &changed);
        assert_eq!(again.ran.len(), 2);
        assert_eq!(again.commit, None);
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]), commit.as_str());
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn formatting_in_place_rewrites_the_files_and_commits_nothing() {
        let repo = repository("in-place");
        let head = git(&repo, &["rev-parse", "HEAD"]);
        let format = MachineFormat {
            rules: vec![rule("**/*.md", "", NOTES)],
        };
        let changed = ["notes.md".to_owned(), "tool/src/lib.rs".to_owned()];

        let (formatted, rewritten) = format.format_in_place(&repo, &changed);

        assert_eq!(formatted.ran.len(), 1, "{formatted:?}");
        assert_eq!(formatted.commit, None);
        assert_eq!(rewritten, ["notes.md"]);
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(&repo, &["status", "--porcelain"]), "M notes.md");
        let (_, again) = format.format_in_place(&repo, &changed);
        assert!(again.is_empty(), "{again:?}");
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn a_failing_formatter_is_reported_and_no_rule_for_other_files_runs() {
        let repo = repository("fail");
        let format = MachineFormat {
            rules: vec![
                rule("**/*.md", "", "git show no-such-file {files}"),
                rule("other/**", "", NOTES),
            ],
        };
        let formatted = format.format(&repo, &["notes.md".to_owned()]);
        assert_eq!(formatted.ran, ["git show no-such-file {files}"]);
        assert_eq!(formatted.failed.len(), 1);
        assert!(
            formatted.failed[0].1.contains("no-such-file"),
            "{formatted:?}"
        );
        assert_eq!(formatted.commit, None);
        assert_eq!(formatted.warnings().len(), 1);
        let _ = fs::remove_dir_all(&repo);
    }
}
