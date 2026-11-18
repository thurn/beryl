//! The signature of a fault the hub files by itself: which check, what kind of failure, on which
//! platform, and the diagnostic part of its output.
//!
//! Two reports with one signature are one fault, so the second adds a comment to the first's
//! ticket. The diagnostic keeps what tells faults apart, such as the values in a failed
//! assertion, and drops what changes from run to run: host names, thread numbers, source line
//! and column numbers, addresses and colour codes. An output in a format it does not recognize
//! is kept whole, which errs towards two tickets rather than merging two faults.

use sha2::{Digest, Sha256};

/// How many lines after a panic or failed assertion belong to it.
const PANIC_LINES: usize = 4;

/// What identifies one fault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    /// The check, or another subject such as an epic or a machine.
    pub subject: String,
    /// The kind of failure: `regression`, `flaky`, `flaky-always`, `machine-flaky`,
    /// `disagreement`, `incomplete`, `cannot-run`, `audit`, `found`, `unreachable` or
    /// `stuck-epic`.
    pub category: String,
    /// The operating system, empty for a portable check.
    pub platform: String,
    pub diagnostic: String,
}

impl Signature {
    /// A signature whose diagnostic is taken from `output`, with `host` left out.
    pub fn of_output(
        subject: &str,
        category: &str,
        platform: &str,
        output: &str,
        host: &str,
    ) -> Self {
        Signature {
            subject: subject.to_owned(),
            category: category.to_owned(),
            platform: platform.to_owned(),
            diagnostic: diagnostic(output, host),
        }
    }

    /// A signature with no output, for a fault named by its subject alone.
    pub fn of_subject(subject: &str, category: &str) -> Self {
        Signature {
            subject: subject.to_owned(),
            category: category.to_owned(),
            platform: String::new(),
            diagnostic: String::new(),
        }
    }

    /// The key stored on the ticket: a SHA-256 over the four parts.
    pub fn key(&self) -> String {
        let parts = [
            "1",
            &self.subject,
            &self.category,
            &self.platform,
            &self.diagnostic,
        ];
        let mut hasher = Sha256::new();
        for part in parts {
            hasher.update(part.len().to_le_bytes());
            hasher.update(part.as_bytes());
        }
        super::hex(&hasher.finalize())
    }
}

/// The lines of `output` that describe the fault, with run-to-run noise replaced.
pub fn diagnostic(output: &str, host: &str) -> String {
    let plain = strip_colour(output);
    let lines: Vec<&str> = plain.lines().collect();
    let mut selected: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("FAULT ") {
            selected.push(line);
            for detail in &lines[i + 1..] {
                if ["FAULT ", "OVER ", "  show:", "..."]
                    .iter()
                    .any(|start| detail.starts_with(start))
                {
                    break;
                }
                selected.push(detail);
            }
        } else if line.contains("panicked at") || is_failed_assertion(line) {
            selected.extend(&lines[i..lines.len().min(i + PANIC_LINES)]);
        } else if line.starts_with("error[")
            || line.starts_with("error:")
            || line.starts_with("OVER ")
            || line.contains("Error:")
            || line.contains("AssertionError")
        {
            selected.push(line);
        }
        i += 1;
    }
    let chosen = if selected.is_empty() { lines } else { selected };
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<&str> = chosen
        .into_iter()
        .filter(|line| seen.insert(*line))
        .collect();
    let mut text = unique.join("\n").trim().to_owned();
    if !host.is_empty() {
        text = text.replace(host, "<host>");
    }
    let text: Vec<String> = text
        .lines()
        .filter(|line| !line.starts_with("note: run with"))
        .map(|line| addresses(&locations(&thread_names(line))))
        .collect();
    let text = text.join("\n");
    if text.is_empty() {
        "(no diagnostic output)".to_owned()
    } else {
        text
    }
}

fn is_failed_assertion(line: &str) -> bool {
    line.find("assertion")
        .is_some_and(|start| line[start..].contains("failed"))
}

/// Removes terminal colour codes, `ESC [ digits-and-semicolons m`.
fn strip_colour(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("\x1b[") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let code = after
            .find(|c: char| !(c.is_ascii_digit() || c == ';'))
            .unwrap_or(after.len());
        if after[code..].starts_with('m') {
            rest = &after[code + 1..];
        } else {
            out.push_str("\x1b[");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// `thread 'name' (1234)` becomes `thread`.
fn thread_names(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find("thread '") {
        let after = &rest[start + "thread '".len()..];
        let replaced = after.find('\'').and_then(|close| {
            let tail = after[close + 1..].strip_prefix(" (")?;
            let digits = tail.find(|c: char| !c.is_ascii_digit())?;
            (digits > 0 && tail[digits..].starts_with(')')).then(|| &tail[digits + 1..])
        });
        if let Some(tail) = replaced {
            out.push_str(&rest[..start]);
            out.push_str("thread");
            rest = tail;
        } else {
            out.push_str(&rest[..start + "thread '".len()]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// `:12:34` followed by a colon, a space or the end of the line becomes `:<location>`.
fn locations(line: &str) -> String {
    let bytes = line.as_bytes();
    let digits_from = |at: usize| {
        bytes[at..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    let mut copied = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let first = digits_from(i + 1);
            let second_colon = i + 1 + first;
            if first > 0 && bytes.get(second_colon) == Some(&b':') {
                let second = digits_from(second_colon + 1);
                let end = second_colon + 1 + second;
                let next = bytes.get(end);
                if second > 0 && next.is_none_or(|b| *b == b':' || b.is_ascii_whitespace()) {
                    out.push_str(&line[copied..i]);
                    out.push_str(":<location>");
                    copied = end;
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
    out.push_str(&line[copied..]);
    out
}

/// A whole word `0x` followed by hexadecimal digits becomes `<address>`.
fn addresses(line: &str) -> String {
    let bytes = line.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    let mut copied = 0;
    while i + 2 < bytes.len() {
        let starts_word = i == 0 || !word(bytes[i - 1]);
        if starts_word && bytes[i] == b'0' && bytes[i + 1] == b'x' {
            let digits = bytes[i + 2..]
                .iter()
                .take_while(|b| b.is_ascii_hexdigit())
                .count();
            let end = i + 2 + digits;
            if digits > 0 && bytes.get(end).is_none_or(|b| !word(*b)) {
                out.push_str(&line[copied..i]);
                out.push_str("<address>");
                copied = end;
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&line[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_keeps_its_assertion_values_and_drops_threads_lines_and_hosts() {
        let output = "\x1b[32mrunning 3 tests\x1b[0m\n\
            thread 'family::tests::corpora' (81723) panicked at crates/a/src/lib.rs:212:9:\n\
            assertion `left == right` failed\n  left: 3\n right: 4\n\
            note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\
            test result: FAILED on jarvis.local";
        let text = diagnostic(output, "jarvis.local");
        assert_eq!(
            text,
            "thread panicked at crates/a/src/lib.rs:<location>:\n\
             assertion `left == right` failed\n  left: 3\n right: 4"
        );
    }

    #[test]
    fn different_assertion_values_are_different_faults_and_line_moves_are_not() {
        let fault = |line: u32, right: u32| {
            Signature::of_output(
                "rust:a",
                "regression",
                "",
                &format!("thread 't' (1) panicked at src/x.rs:{line}:5:\nassertion failed\n right: {right}"),
                "",
            )
            .key()
        };
        assert_eq!(fault(10, 4), fault(99, 4));
        assert_ne!(fault(10, 4), fault(10, 5));
        assert_ne!(
            Signature::of_subject("a", "flaky").key(),
            Signature::of_subject("a", "found").key()
        );
    }

    #[test]
    fn fault_blocks_end_at_the_next_marker_and_addresses_are_hidden() {
        let output =
            "setup\nFAULT shots:x overlap\n  at 0x7ffee1 and 0x7ffee2\n  show: url\nOVER budget\n";
        assert_eq!(
            diagnostic(output, ""),
            "FAULT shots:x overlap\n  at <address> and <address>\nOVER budget"
        );
    }

    #[test]
    fn an_unknown_format_is_kept_whole_and_empty_output_is_named() {
        assert_eq!(diagnostic("one\ntwo\n", ""), "one\ntwo");
        assert_eq!(diagnostic("", ""), "(no diagnostic output)");
    }
}
