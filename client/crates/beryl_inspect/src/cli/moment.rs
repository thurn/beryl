//! `moment`: the moment of a recording whose element tree is nearest a saved one, such as the
//! `tree.txt` of a bug report. A report saved in a live game holds the latest views and the tree
//! of the frozen frame, but not how long after the last view the player froze the table, so
//! `just client bug` finds the frame by comparing trees.

use std::collections::BTreeMap;

use app_client::driver::{Duration, Moment, Name, Offset, TICK};
use kit_probe::agent::{TreeOptions, tree};
use serde_json::json;

use super::options::{Options, Usage};
use super::{Printed, milliseconds, open, time_of};

/// What an element of the saved tree that the frame lacks, or an element of the frame that the
/// saved tree lacks, adds to the distance: as much as a card that is 400 points out of place.
const MISSING: u64 = 400;
/// What each state word (`tapped`, `selected`, `covered`, ...) that one tree has and the other
/// lacks adds to the distance.
const STATE: u64 = 40;

/// One element line of a printed tree.
#[derive(Debug, PartialEq, Eq)]
struct Line<'a> {
    /// Everything before the element's box: its role, name, number and place.
    key: String,
    /// Its box in whole logical points: left, top, width and height.
    rect: [i64; 4],
    /// Its state words, without the amounts in parentheses.
    states: Vec<&'a str>,
}

/// The element lines of `text`, a tree as `just client tree` prints it, by key. Lines with no
/// box, such as the header and the containers, are left out.
fn lines(text: &str) -> BTreeMap<String, Vec<Line<'_>>> {
    let mut out: BTreeMap<String, Vec<Line<'_>>> = BTreeMap::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some(at) = (0..words.len().saturating_sub(1))
            .find(|&at| point(words[at]).is_some() && size(words[at + 1]).is_some())
        else {
            continue;
        };
        let (Some((x, y)), Some((width, height))) = (point(words[at]), size(words[at + 1])) else {
            continue;
        };
        let key = words[..at].join(" ");
        let mut states: Vec<&str> = words[at + 2..]
            .iter()
            .map(|word| word.split('(').next().unwrap_or(word))
            .collect();
        states.sort_unstable();
        let rect = [x, y, width, height];
        out.entry(key.clone())
            .or_default()
            .push(Line { key, rect, states });
    }
    out
}

fn point(word: &str) -> Option<(i64, i64)> {
    let (x, y) = word.split_once(',')?;
    Some((x.parse().ok()?, y.parse().ok()?))
}

fn size(word: &str) -> Option<(i64, i64)> {
    let (width, height) = word.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

/// How far the tree `found` is from `saved`: the sum, over the elements of either, of how far
/// each is moved or resized in points and how many state words differ, with [`MISSING`] for an
/// element only one of them has. Elements with the same key are paired in tree order.
fn distance(
    saved: &BTreeMap<String, Vec<Line<'_>>>,
    found: &BTreeMap<String, Vec<Line<'_>>>,
) -> u64 {
    let mut total = 0;
    for (key, wanted) in saved {
        let had = found.get(key).map_or(&[][..], Vec::as_slice);
        for (index, line) in wanted.iter().enumerate() {
            total += had.get(index).map_or(MISSING, |other| apart(line, other));
        }
        total += MISSING * had.len().saturating_sub(wanted.len()) as u64;
    }
    for (key, had) in found {
        if !saved.contains_key(key) {
            total += MISSING * had.len() as u64;
        }
    }
    total
}

fn apart(a: &Line<'_>, b: &Line<'_>) -> u64 {
    let moved: u64 = (a.rect.iter().zip(b.rect))
        .map(|(x, y)| x.abs_diff(y))
        .sum();
    let only_a = a.states.iter().filter(|s| !b.states.contains(s)).count();
    let only_b = b.states.iter().filter(|s| !a.states.contains(s)).count();
    moved + STATE * (only_a + only_b) as u64
}

pub(super) fn moment(options: &Options) -> Result<Printed, Usage> {
    let Some(path) = options.value("--like") else {
        return Err(Usage(
            "moment needs --like <tree.txt>, the saved tree to match".to_owned(),
        ));
    };
    let text = std::fs::read_to_string(path).map_err(|error| Usage(format!("{path}: {error}")))?;
    let saved = lines(&text);
    if saved.is_empty() {
        return Err(Usage(format!("{path}: no element line with a box")));
    }
    let bound = |flag: &str, name: Name| -> Result<Moment, Usage> {
        options.value(flag).map_or(
            Ok(Moment::Named {
                name,
                offset: Offset::default(),
            }),
            |text| {
                text.parse()
                    .map_err(|error| Usage(format!("{flag} {text}: {error}")))
            },
        )
    };
    let (from, to) = (bound("--from", Name::Start)?, bound("--to", Name::End)?);
    let mut page = open(options)?;
    let from = time_of(&mut page, options, &from)?;
    let to = time_of(&mut page, options, &to)?;
    page.seek(&Moment::At(Duration(from)))
        .map_err(|error| Usage(error.to_string()))?;
    let mut best: Option<(u64, u64)> = None;
    let mut frames = 0;
    loop {
        let at = page.now();
        let printed = tree(page.frame(), &TreeOptions::default())
            .map_err(|error| Usage(error.to_string()))?;
        let far = distance(&saved, &lines(&printed));
        frames += 1;
        if best.is_none_or(|(_, least)| far < least) {
            best = Some((at, far));
        }
        if at >= to {
            break;
        }
        page.step(TICK);
    }
    let (at, far) = best.expect("at least one frame is compared");
    let at_ms = milliseconds(at);
    let elements: usize = saved.values().map(Vec::len).sum();
    let out = if options.json {
        format!(
            "{}\n",
            json!({
                "command": options.command_line(),
                "at_ms": at_ms,
                "distance": far,
                "elements": elements,
                "frames": frames,
            })
        )
    } else {
        format!(
            "{}\n{at_ms}ms is nearest the {elements} elements of {path}, at a distance of {far} \
             ({frames} frames compared from {}ms to {}ms)\n",
            options.command_line(),
            milliseconds(from),
            milliseconds(to),
        )
    };
    Ok(Printed::text(out))
}

#[cfg(test)]
mod tests {
    use super::{MISSING, STATE, distance, lines};

    const SAVED: &str = "game\n  table\n    card \"Dandân\" #268 battlefield.yours.creatures \
                         1336,349 253x354 interactive\n    card #11 library.yours 34,795 82x91 \
                         face-down covered(100%)\n";

    #[test]
    fn the_same_tree_is_at_no_distance() {
        assert_eq!(distance(&lines(SAVED), &lines(SAVED)), 0);
    }

    #[test]
    fn a_moved_card_is_as_far_as_it_moved_and_a_lost_one_counts_whole() {
        let moved = SAVED.replace("1336,349 253x354", "1300,349 253x350");
        assert_eq!(distance(&lines(SAVED), &lines(&moved)), 36 + 4);
        let lost = SAVED.replace("card #11", "card #12");
        assert_eq!(distance(&lines(SAVED), &lines(&lost)), 2 * MISSING);
        let tapped = SAVED.replace("covered(100%)", "covered(40%) tapped");
        assert_eq!(distance(&lines(SAVED), &lines(&tapped)), STATE);
    }
}
