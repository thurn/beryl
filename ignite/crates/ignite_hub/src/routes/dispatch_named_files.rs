//! Resolves the file paths named by tickets for dispatch and admission.

use ignite_core::admission;

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
pub(super) fn areas(files: &[String], known: &[String]) -> Vec<String> {
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

#[cfg(test)]
#[path = "dispatch_named_file_tests.rs"]
mod named_file_tests;
