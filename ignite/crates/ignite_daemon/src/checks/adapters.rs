//! The two adapters that turn a family's commands into checks and outcomes (see "Reporting
//! results" in `docs/ignite.md`).
//!
//! - `jsonl`: the family's `list` command prints one JSON line per check,
//!   `{"check": "<family>:<name>", "inputs": [...], "digest": "..."}`, where `inputs` are path
//!   patterns the check reads beyond the family's own and the optional `digest` stands for what
//!   it reads that no file holds (see [`Listed::digest`]). Its `run` command, with `{checks}`
//!   replaced by the names, writes one JSON line per check to the file named by
//!   `IGNITE_RESULTS`: `{"check", "outcome", "at", "message", "reason", "partial"}`. An
//!   `incomplete` line may give the `reason` `cannot-run` when a tool, a virtual machine or a
//!   download this machine lacks stops the check; the check is then not run again here. A tool
//!   that judges a check in several steps marks each line but the last `"partial": true`, so a
//!   check whose last line is a partial pass is `incomplete`: its later steps never reported.
//! - `nextest`: cargo-nextest's own output. `cargo nextest list --message-format json` gives the
//!   tests of each test binary, and `cargo nextest run --message-format libtest-json-plus` one
//!   JSON line per test event. A test's check is named `<family>:<binary id>::<test>`, so a
//!   library's unit test reads `rust:<package>::<module path>::<test>`.
//!
//! Commands are split on whitespace and run without a shell, so a check name is always one
//! argument however it is spelled.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use ignite_core::ids::CheckName;
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::IncompleteReason;

/// The variable that names the file a `jsonl` family's `run` command writes its results to.
pub const RESULTS_ENV: &str = "IGNITE_RESULTS";

/// The placeholder in a `jsonl` family's `run` command for the names of the checks to run.
const CHECKS: &str = "{checks}";

/// The variable cargo-nextest needs before it writes libtest JSON.
pub const NEXTEST_JSON_ENV: (&str, &str) = ("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1");

/// cargo-nextest's list command, when the family names none.
const NEXTEST_LIST: &str = "cargo nextest list --message-format json";

/// cargo-nextest's run command, when the family names none.
const NEXTEST_RUN: &str = "cargo nextest run --no-fail-fast --message-format libtest-json-plus";

/// One check a family's `list` command printed. It is stored as JSON with the family's listing
/// (see `checks::listings`), so the fields that are usually empty are left out there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
    pub check: CheckName,
    /// Path patterns the check reads beyond its family's `inputs`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    /// What the check reads that no file in the tree holds, as the family's `list` command
    /// summarizes it, such as the commit subjects since the branch left staging. It enters the
    /// input hash, so a pass is reused only while it is unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// The workspace package that holds the check, which `crate-closure` starts from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// How cargo-nextest names the test, for a `nextest` family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    /// Why the listing is incomplete, on the one entry named `<family>:listing` that stands
    /// for the checks a listing named but Ignite refused (see [`jsonl_list`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
}

/// A test as cargo-nextest names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    /// Such as `game_rules` for a library or `game_cli::cli` for an integration test.
    pub binary_id: String,
    /// The part before `$` in a libtest event's name: `<package>::<binary name>`.
    pub libtest: String,
    /// The test's path inside its binary, such as `tests::entering::aura`.
    pub test: String,
}

/// What a run said about one check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reported {
    pub check: CheckName,
    pub outcome: Outcome,
    /// The file and line of a failure, such as `src/a.rs:88`.
    pub at: Option<String>,
    /// The first lines of a failure's message.
    pub message: Option<String>,
    /// Why an `incomplete` check has no verdict, if the command said.
    pub reason: Option<IncompleteReason>,
}

/// A command as arguments: split on whitespace.
pub fn words(command: &str) -> Vec<String> {
    command.split_whitespace().map(str::to_owned).collect()
}

/// A `jsonl` family's `run` command for `checks`: each name is its own argument where the
/// command says `{checks}`, or after the command if it does not.
pub fn jsonl_run(run: &str, checks: &[&CheckName]) -> Vec<String> {
    let names = checks.iter().map(|c| c.as_str().to_owned());
    let mut args = Vec::new();
    let mut placed = false;
    for word in words(run) {
        if word == CHECKS {
            args.extend(names.clone());
            placed = true;
        } else {
            args.push(word);
        }
    }
    if !placed {
        args.extend(names);
    }
    args
}

/// The checks in a `jsonl` family's listing. Lines that are not a check of `family` are ignored,
/// so a tool may print progress beside them.
///
/// A line that names a check of `family` that Ignite refuses (a name with whitespace in it, or
/// a line that is not otherwise a check) would lose that check without a word, so the listing
/// ends with one more entry, `<family>:listing`, whose [`Listed::refused`] names those lines.
/// The runner counts that entry as an incomplete check.
pub fn jsonl_list(text: &str, family: &str) -> Vec<Listed> {
    #[derive(Deserialize)]
    struct Line {
        check: CheckName,
        #[serde(default)]
        inputs: Vec<String>,
        digest: Option<String>,
        /// The workspace package the check belongs to, for a family whose products build per
        /// package (`crate-closure` in a product's inputs, `{packages}` in its build).
        #[serde(default)]
        package: Option<String>,
    }
    let mut listed = Vec::new();
    let mut refused = Vec::new();
    for text in text.lines().map(str::trim) {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        let Some(name) = value.get("check").and_then(Value::as_str) else {
            continue;
        };
        if name.split_once(':').map(|(f, _)| f) != Some(family) {
            continue;
        }
        match serde_json::from_value::<Line>(value.clone()) {
            Ok(line) => listed.push(Listed {
                check: line.check,
                inputs: line.inputs,
                digest: line.digest,
                package: line.package,
                target: None,
                refused: None,
            }),
            Err(error) => refused.push(format!("{name:?} ({error})")),
        }
    }
    if !refused.is_empty()
        && let Ok(check) = CheckName::new(format!("{family}:listing"))
    {
        listed.push(Listed {
            check,
            inputs: Vec::new(),
            digest: None,
            package: None,
            target: None,
            refused: Some(refused_message(family, &refused)),
        });
    }
    listed
}

/// How many refused names [`refused_message`] quotes; the rest are only counted, so that one
/// family with many bad names does not write a log line of tens of kilobytes.
const REFUSED_QUOTED: usize = 3;

/// The message of a family's `<family>:listing` check: how many listed names Ignite refused,
/// and the first few of them with the reason.
fn refused_message(family: &str, refused: &[String]) -> String {
    let quoted = refused[..refused.len().min(REFUSED_QUOTED)].join(", ");
    let more = refused.len().saturating_sub(REFUSED_QUOTED);
    let rest = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    format!(
        "the listing of {family} named {} check(s) Ignite refuses, which never run: {quoted}{rest}",
        refused.len()
    )
}

/// The results a `jsonl` family's `run` command wrote. A later line for a check replaces an
/// earlier one, except that nothing replaces a failure: a tool that judges one check in several
/// steps, such as a page in each layout, may write a line per step, and a pass from the last
/// step must not hide a failure from an earlier one. A pass marked `partial` is not a verdict:
/// a check whose last line is one is `incomplete` with no reason, which the runner retries.
pub fn jsonl_results(text: &str) -> Vec<Reported> {
    parsed_results(text).0
}

/// The results of a `jsonl` run that was cut short: it hung, or ended by a signal or by an
/// error that no failing check explains. A check whose last line is a `partial` pass is already
/// `incomplete` (see [`jsonl_results`]). A tool that does not mark its steps may still have been
/// in the middle of the check of the last line written, and its pass says nothing about the
/// steps that never ran, so that line also becomes `incomplete` with no reason, which the
/// runner retries on its own.
pub fn jsonl_results_cut_short(text: &str) -> Vec<Reported> {
    let (mut reported, last) = parsed_results(text);
    for r in &mut reported {
        if Some(&r.check) == last.as_ref() && r.outcome == Outcome::Pass {
            r.outcome = Outcome::Incomplete;
            r.message = Some("the command ended after this check's last passing step".to_owned());
        }
    }
    reported
}

/// The results in `text` (see [`jsonl_results`]) and the check the last line names.
fn parsed_results(text: &str) -> (Vec<Reported>, Option<CheckName>) {
    #[derive(Deserialize)]
    struct Line {
        check: CheckName,
        outcome: Outcome,
        at: Option<String>,
        message: Option<String>,
        #[serde(default)]
        reason: Option<IncompleteReason>,
        /// More steps of this check follow.
        #[serde(default)]
        partial: bool,
    }
    let mut reported: Vec<Reported> = Vec::new();
    let mut last = None;
    for line in text
        .lines()
        .filter_map(|l| serde_json::from_str::<Line>(l.trim()).ok())
    {
        last = Some(line.check.clone());
        if reported
            .iter()
            .any(|r| r.check == line.check && r.outcome == Outcome::Fail)
        {
            continue;
        }
        reported.retain(|r| r.check != line.check);
        let unfinished = line.partial && line.outcome == Outcome::Pass;
        reported.push(Reported {
            check: line.check,
            outcome: if unfinished {
                Outcome::Incomplete
            } else {
                line.outcome
            },
            at: line.at,
            message: if unfinished {
                Some("the command ended before this check's last step reported".to_owned())
            } else {
                line.message
            },
            reason: if unfinished { None } else { line.reason },
        });
    }
    (reported, last)
}

/// A `nextest` family's list command.
pub fn nextest_list_command(list: Option<&str>) -> Vec<String> {
    words(list.unwrap_or(NEXTEST_LIST))
}

/// A `nextest` family's run command for `targets`, selected with a filterset.
pub fn nextest_run_command(run: Option<&str>, targets: &[&Target]) -> Vec<String> {
    let mut args = words(run.unwrap_or(NEXTEST_RUN));
    let filter: Vec<String> = targets
        .iter()
        .map(|t| format!("(binary_id(={}) & test(={}))", t.binary_id, t.test))
        .collect();
    args.push("-E".to_owned());
    args.push(filter.join(" | "));
    args
}

/// The tests in cargo-nextest's JSON listing, skipping ignored tests and tests its filter left
/// out. The listing is the one line of the output that is a JSON object with `rust-suites`.
pub fn nextest_list(text: &str, family: &str) -> Vec<Listed> {
    let Some(listing) = text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .find(|value| value.get("rust-suites").is_some())
    else {
        return Vec::new();
    };
    let mut listed = Vec::new();
    let suites = listing["rust-suites"].as_object().into_iter().flatten();
    for suite in suites.map(|(_, suite)| suite) {
        let text = |key: &str| suite[key].as_str().unwrap_or_default().to_owned();
        let (binary_id, package) = (text("binary-id"), text("package-name"));
        let libtest = format!("{package}::{}", text("binary-name"));
        let cases = suite["testcases"].as_object().into_iter().flatten();
        for (test, case) in cases {
            let matches = case["filter-match"]["status"].as_str() == Some("matches");
            if !matches || case["ignored"].as_bool() == Some(true) {
                continue;
            }
            let Ok(check) = CheckName::new(format!("{family}:{binary_id}::{test}")) else {
                continue;
            };
            listed.push(Listed {
                check,
                inputs: Vec::new(),
                digest: None,
                package: Some(package.clone()),
                target: Some(Target {
                    binary_id: binary_id.clone(),
                    libtest: libtest.clone(),
                    test: test.clone(),
                }),
                refused: None,
            });
        }
    }
    listed
}

/// The outcomes in cargo-nextest's libtest JSON for the tests in `listed`. A test that started
/// and never finished is not reported.
pub fn nextest_results(text: &str, listed: &[Listed]) -> Vec<Reported> {
    let mut reported = Vec::new();
    for event in text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
    {
        if event["type"] != "test" {
            continue;
        }
        let outcome = match event["event"].as_str() {
            Some("ok") => Outcome::Pass,
            Some("failed") => Outcome::Fail,
            _ => continue,
        };
        let Some((libtest, test_name)) = event["name"].as_str().and_then(|n| n.split_once('$'))
        else {
            continue;
        };
        let Some(found) = listed.iter().find(|l| {
            l.target
                .as_ref()
                .is_some_and(|t| t.libtest == libtest && t.test == test_name)
        }) else {
            continue;
        };
        let (at, message) = match outcome {
            Outcome::Fail => panic_location(event["stdout"].as_str().unwrap_or_default()),
            _ => (None, None),
        };
        reported.push(Reported {
            check: found.check.clone(),
            outcome,
            at,
            message,
            reason: None,
        });
    }
    reported
}

/// The file and line of the first panic in a test's output, and the two lines after it.
fn panic_location(output: &str) -> (Option<String>, Option<String>) {
    let Some(start) = output.find(" panicked at ") else {
        return (None, first_lines(output));
    };
    let rest = &output[start + " panicked at ".len()..];
    let (location, after) = rest.split_once('\n').unwrap_or((rest, ""));
    let location = location.trim_end_matches(':');
    // `src/a.rs:88:5` becomes `src/a.rs:88`.
    let at = match location.rsplit_once(':') {
        Some((file_line, column)) if column.chars().all(|c| c.is_ascii_digit()) => file_line,
        _ => location,
    };
    (Some(at.to_owned()), first_lines(after))
}

fn first_lines(text: &str) -> Option<String> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(2)
        .collect();
    (!lines.is_empty()).then(|| lines.join("  "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_with_many_refused_names_quotes_only_the_first_few() {
        let refused: Vec<String> = (0..74).map(|n| format!("\"bad {n}\" (spaces)")).collect();
        let message = refused_message("pytest", &refused);
        assert!(message.contains("named 74 check(s)"), "{message}");
        assert!(message.contains("\"bad 2\""), "{message}");
        assert!(!message.contains("\"bad 3\""), "{message}");
        assert!(message.ends_with("and 71 more"), "{message}");
        let few = refused_message("pytest", &refused[..2]);
        assert!(few.ends_with("\"bad 1\" (spaces)"), "{few}");
    }

    /// A `jsonl` family that needs a product built per package failed every check with
    /// "crate-closure: the check belongs to no package", because a listing line could not name
    /// its package. It now can.
    #[test]
    fn a_jsonl_listing_line_names_its_package() {
        let listed = jsonl_list(
            "{\"check\": \"alloc:core\", \"package\": \"core\"}\n",
            "alloc",
        );
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].package.as_deref(), Some("core"));
    }

    #[test]
    fn jsonl_listing_and_results() {
        let listing = "building...\n\
            {\"check\": \"py:t/test_a.py::one\", \"inputs\": [\"t/a.py\"]}\n\
            {\"check\": \"other:x\"}\n\
            {\"check\": \"py:t/test_a.py::two\", \"digest\": \"d1\"}\n";
        let listed = jsonl_list(listing, "py");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].package, None);
        assert_eq!(listed[0].inputs, ["t/a.py"]);
        assert_eq!(listed[0].digest, None);
        assert_eq!(listed[1].digest.as_deref(), Some("d1"));
        let results = "{\"check\": \"py:a\", \"outcome\": \"incomplete\"}\n\
            {\"check\": \"py:a\", \"outcome\": \"fail\", \"at\": \"a.py:3\", \"message\": \"no\"}\n\
            {\"check\": \"py:b\", \"outcome\": \"incomplete\", \"reason\": \"cannot-run\"}\n";
        let reported = jsonl_results(results);
        assert_eq!(reported.len(), 2);
        assert_eq!(reported[0].outcome, Outcome::Fail);
        assert_eq!(reported[0].at.as_deref(), Some("a.py:3"));
        assert_eq!(reported[0].reason, None);
        assert_eq!(reported[1].reason, Some(IncompleteReason::CannotRun));
    }

    #[test]
    fn a_refused_check_name_makes_the_listing_incomplete() {
        let listing = "{\"check\": \"ui:a land is played\"}\n\
            {\"check\": \"ui:fine\"}\n\
            {\"check\": \"other:a b\"}\n\
            {\"progress\": 3}\n";
        let listed = jsonl_list(listing, "ui");
        let names: Vec<&str> = listed.iter().map(|l| l.check.as_str()).collect();
        assert_eq!(names, ["ui:fine", "ui:listing"]);
        assert_eq!(listed[0].refused, None);
        let why = listed[1].refused.as_deref().unwrap();
        assert!(why.contains("\"ui:a land is played\""), "{why}");
        assert!(!why.contains("other:"), "{why}");
    }

    #[test]
    fn a_later_pass_does_not_hide_a_failure_of_the_same_check() {
        let results = "{\"check\": \"py:a\", \"outcome\": \"fail\", \"at\": \"a.py:3\"}\n\
            {\"check\": \"py:a\", \"outcome\": \"pass\"}\n\
            {\"check\": \"py:b\", \"outcome\": \"incomplete\"}\n\
            {\"check\": \"py:b\", \"outcome\": \"pass\"}\n";
        let reported = jsonl_results(results);
        let outcome = |check: &str| reported.iter().find(|r| r.check.as_str() == check).unwrap();
        assert_eq!(outcome("py:a").outcome, Outcome::Fail);
        assert_eq!(outcome("py:a").at.as_deref(), Some("a.py:3"));
        assert_eq!(outcome("py:b").outcome, Outcome::Pass);
    }

    #[test]
    fn a_partial_pass_is_no_verdict_until_the_last_step_reports() {
        // `py:a` passed its first step and the command ended; `py:b` finished both steps;
        // `py:c` failed a partial step, which stands.
        let results = "{\"check\": \"py:a\", \"outcome\": \"pass\", \"partial\": true}\n\
            {\"check\": \"py:b\", \"outcome\": \"pass\", \"partial\": true}\n\
            {\"check\": \"py:c\", \"outcome\": \"fail\", \"partial\": true}\n\
            {\"check\": \"py:b\", \"outcome\": \"pass\"}\n";
        for reported in [jsonl_results(results), jsonl_results_cut_short(results)] {
            let outcome = |c: &str| reported.iter().find(|r| r.check.as_str() == c).unwrap();
            assert_eq!(outcome("py:a").outcome, Outcome::Incomplete);
            assert_eq!(outcome("py:a").reason, None);
            assert_eq!(outcome("py:c").outcome, Outcome::Fail);
        }
        let whole = jsonl_results(results);
        let b = whole.iter().find(|r| r.check.as_str() == "py:b").unwrap();
        assert_eq!(b.outcome, Outcome::Pass);
    }

    #[test]
    fn jsonl_run_puts_each_name_in_its_own_argument() {
        let a = CheckName::new("py:t/a.py::x").unwrap();
        let b = CheckName::new("py:t/b.py::y").unwrap();
        assert_eq!(
            jsonl_run("tool run py {checks} --fast", &[&a, &b]),
            [
                "tool",
                "run",
                "py",
                "py:t/a.py::x",
                "py:t/b.py::y",
                "--fast"
            ]
        );
        assert_eq!(jsonl_run("tool", &[&a]), ["tool", "py:t/a.py::x"]);
    }

    const LISTING: &str = r#"{"rust-build-meta": {}, "test-count": 3, "rust-suites": {
        "core": {"binary-id": "core", "binary-name": "core", "package-name": "core",
          "testcases": {
            "ids::tests::a": {"ignored": false, "filter-match": {"status": "matches"}},
            "ids::tests::slow": {"ignored": true, "filter-match": {"status": "matches"}}}},
        "cli::cli": {"binary-id": "cli::cli", "binary-name": "cli", "package-name": "cli",
          "testcases": {
            "runs": {"ignored": false, "filter-match": {"status": "matches"}}}}}}"#;

    fn listing() -> Vec<Listed> {
        nextest_list(&LISTING.replace('\n', " "), "rust")
    }

    #[test]
    fn nextest_listing_names_checks_by_binary_and_test() {
        let names: Vec<String> = listing().iter().map(|l| l.check.to_string()).collect();
        assert_eq!(names, ["rust:cli::cli::runs", "rust:core::ids::tests::a"]);
        assert_eq!(listing()[0].package.as_deref(), Some("cli"));
    }

    #[test]
    fn nextest_results_match_libtest_names_and_find_the_panic() {
        let output = r#"{"type":"suite","event":"started","test_count":2}
{"type":"test","event":"started","name":"core::core$ids::tests::a"}
{"type":"test","event":"failed","name":"core::core$ids::tests::a","stdout":"thread 'a' panicked at src/ids.rs:88:5:\nleft: 14\nright: 12\n"}
{"type":"test","event":"started","name":"cli::cli$runs"}
"#;
        let reported = nextest_results(output, &listing());
        assert_eq!(reported.len(), 1, "the unfinished test is not reported");
        assert_eq!(reported[0].check.as_str(), "rust:core::ids::tests::a");
        assert_eq!(reported[0].at.as_deref(), Some("src/ids.rs:88"));
        assert_eq!(reported[0].message.as_deref(), Some("left: 14  right: 12"));
    }

    #[test]
    fn nextest_run_selects_each_test_exactly() {
        let listed = listing();
        let targets: Vec<&Target> = listed.iter().filter_map(|l| l.target.as_ref()).collect();
        let args = nextest_run_command(None, &targets);
        assert_eq!(
            args.last().unwrap(),
            "(binary_id(=cli::cli) & test(=runs)) | (binary_id(=core) & test(=ids::tests::a))"
        );
    }
}
