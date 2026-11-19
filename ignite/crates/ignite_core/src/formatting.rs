//! `[format]` in `ignite.toml`: the formatters `ignite land` runs on the files a landing changes
//! before it runs the gate or the express commands (see "Formatting at landing" in the design
//! document).
//!
//! On 2026-11-18 workers on one epic lost about 25 minutes to formatting failures that the gate
//! or the express commands found one landing at a time: `cargo fmt --check` failed two runs after
//! 512 and 759 s, and `mdformat --check` failed a landing on one Markdown file. A formatter fixes
//! what it finds in a second, so the landing runs it first and commits what it changed.

use serde::{Deserialize, Serialize};

use crate::inputs::path_matches;

/// The `[format]` table: the rules, tried in order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Format {
    #[serde(default)]
    pub rules: Vec<FormatRule>,
}

/// One formatter: the command runs once when any file the landing changes matches `paths` and
/// no pattern of `exclude`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatRule {
    /// Path patterns, relative to the checkout's root, as in a family's `inputs`.
    pub paths: Vec<String>,
    /// Patterns of files that `paths` matches but the command must not be given.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// The directory, relative to the checkout's root, that the command runs in; empty is the
    /// root.
    #[serde(default)]
    pub dir: String,
    /// The command, split on whitespace and run without a shell. A word `{files}` is replaced by
    /// the matching files that still exist, relative to `dir`.
    pub command: String,
}

/// The word in a command that stands for the files it formats.
pub const FILES: &str = "{files}";

impl FormatRule {
    /// Whether the rule formats `path`.
    pub fn matches(&self, path: &str) -> bool {
        self.paths.iter().any(|p| path_matches(p, path))
            && !self.exclude.iter().any(|p| path_matches(p, path))
    }

    /// The command's words for the files `files` (already matched, relative to the root):
    /// `{files}` becomes each of them relative to `dir`.
    pub fn words(&self, files: &[String]) -> Vec<String> {
        let dir = self.dir.trim_matches('/');
        let relative = |file: &String| {
            if dir.is_empty() {
                return file.clone();
            }
            file.strip_prefix(dir)
                .and_then(|rest| rest.strip_prefix('/'))
                .map_or_else(
                    // A file outside `dir` is named from `dir` by climbing to the root.
                    || format!("{}{file}", "../".repeat(dir.split('/').count())),
                    str::to_owned,
                )
        };
        let mut words = Vec::new();
        for word in self.command.split_whitespace() {
            if word == FILES {
                words.extend(files.iter().map(relative));
            } else {
                words.push(word.to_owned());
            }
        }
        words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(paths: &[&str], exclude: &[&str], dir: &str, command: &str) -> FormatRule {
        FormatRule {
            paths: paths.iter().map(|p| (*p).to_owned()).collect(),
            exclude: exclude.iter().map(|p| (*p).to_owned()).collect(),
            dir: dir.to_owned(),
            command: command.to_owned(),
        }
    }

    #[test]
    fn a_rule_matches_its_paths_less_its_exclusions() {
        let markdown = rule(&["**/*.md"], &[".agents/**"], "", "mdformat {files}");
        assert!(markdown.matches("README.md"));
        assert!(markdown.matches("docs/ignite.md"));
        assert!(!markdown.matches(".agents/skills/a/SKILL.md"));
        assert!(!markdown.matches("docs/ignite.txt"));
    }

    #[test]
    fn files_are_named_relative_to_the_rules_directory() {
        let prettier = rule(&["ui/**"], &[], "ui", "prettier --write {files} --end");
        let files = ["ui/src/a.ts".to_owned(), "docs/x.md".to_owned()];
        assert_eq!(
            prettier.words(&files),
            ["prettier", "--write", "src/a.ts", "../docs/x.md", "--end"]
        );
        let root = rule(&["**/*.md"], &[], "", "mdformat {files}");
        assert_eq!(root.words(&files[1..]), ["mdformat", "docs/x.md"]);
        let whole = rule(&["ignite/**/*.rs"], &[], "ignite", "cargo fmt");
        assert_eq!(whole.words(&files), ["cargo", "fmt"]);
    }
}
