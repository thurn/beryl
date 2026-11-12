//! The ladder's blunder suite gate (docs/ai-strategy.md §4.3).

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use beryl_search::DEEP_SEARCHER;
use beryl_session::corpus::today;

use crate::blunders::{self, BUDGETS, Entry, EntryReport, Report, SEEDS};
use crate::ladder::resolve;
use crate::match_log::{cache, files};

const USAGE: &str = "usage: beryl_cli gate <challenger family> --claims NAME,...|none \
                     [--vs CHAMPION FAMILY]";
const RUNS: usize = BUDGETS.len() * SEEDS as usize;

pub fn main(args: &[String]) -> Result<bool, String> {
    let mut challenger = None;
    let mut champion = None;
    let mut claims = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--claims" if claims.is_none() => claims = Some(value()?),
            "--vs" if champion.is_none() => champion = Some(value()?),
            name if !name.starts_with('-') && challenger.is_none() => challenger = Some(name),
            _ => return Err(USAGE.to_owned()),
        }
    }
    let challenger = family(challenger.ok_or(USAGE)?)?;
    let champion = match champion {
        Some(name) => family(name)?,
        None => DEEP_SEARCHER.to_string(),
    };
    let entries = blunders::entries()?;
    let claims = claims_list(claims.ok_or(USAGE)?, &entries)?;
    let name = challenger.replace('/', "_");
    let out = cache()
        .join("ladder")
        .join("gates")
        .join(format!("{}-{name}", today()));
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let _lock = files::lock(&out)?;

    let champion = suite(&entries, &champion, "champion")?;
    let challenger = suite(&entries, &challenger, "challenger")?;
    save(&out.join("champion.json"), &champion)?;
    save(&out.join("challenger.json"), &challenger)?;
    println!("Reports saved to {}", out.display());
    if champion.drifted > 0 || challenger.drifted > 0 {
        let reasons = champion
            .entries
            .iter()
            .zip(&challenger.entries)
            .filter_map(|(before, after)| before.drift.as_deref().or(after.drift.as_deref()))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "rebuild or retire the drifted entries (docs/ai-strategy.md §5)\n{reasons}"
        ));
    }
    println!(
        "Champion {}\nChallenger {}",
        champion.agent, challenger.agent
    );
    let scores = champion
        .entries
        .iter()
        .zip(&challenger.entries)
        .map(|(before, after)| (before.name.as_str(), passes(before), passes(after)));
    let (table, pass) = compare(scores, &claims);
    print!("{table}");
    println!("GATE: {}", if pass { "PASS" } else { "FAIL" });
    Ok(pass)
}

/// A gate always uses both suite budgets, even for a family supplied with a budget.
fn family(name: &str) -> Result<String, String> {
    let resolved = resolve(name, BUDGETS[0])?;
    let (family, _) = resolved
        .split_once('@')
        .ok_or_else(|| format!("{name:?}: the gate needs a search family at both budgets"))?;
    Ok(family.to_owned())
}

fn claims_list<'a>(names: &'a str, entries: &[Entry]) -> Result<Vec<&'a str>, String> {
    if names == "none" {
        return Ok(Vec::new());
    }
    names
        .split(',')
        .map(|name| {
            entries
                .iter()
                .any(|entry| entry.name == name)
                .then_some(name)
                .ok_or_else(|| format!("--claims: no entry {name:?} in data/blunders.json"))
        })
        .collect()
}

fn suite(entries: &[Entry], family: &str, role: &str) -> Result<Report, String> {
    eprintln!("{role}: {family}");
    blunders::run(entries, family, &BUDGETS, SEEDS, |line| {
        eprintln!("{role}: {line}");
    })
}

fn save(path: &Path, report: &Report) -> Result<(), String> {
    let json = serde_json::to_string_pretty(report).map_err(|e| e.to_string())? + "\n";
    files::write_whole(path, json.as_bytes())
}

fn passes(entry: &EntryReport) -> usize {
    entry.runs.iter().filter(|run| run.pass).count()
}

fn compare<'a>(
    scores: impl Iterator<Item = (&'a str, usize, usize)>,
    claims: &[&str],
) -> (String, bool) {
    let mut table = format!(
        "{:<36} {:>8} {:>10}  mark\n",
        "entry", "champion", "challenger"
    );
    let (mut claimed_fixed, mut reopened, mut worse) = (0, 0, 0);
    for (name, before, after) in scores {
        let mark = if before < RUNS && after == RUNS && claims.contains(&name) {
            claimed_fixed += 1;
            "claimed-fixed"
        } else if before == RUNS && after < RUNS {
            reopened += 1;
            "re-opened"
        } else if before.saturating_sub(after) >= 4 {
            worse += 1;
            "worse"
        } else {
            ""
        };
        let _ = writeln!(
            table,
            "{name:<36} {before:>5}/{RUNS} {after:>7}/{RUNS}  {mark}"
        );
    }
    let _ = writeln!(
        table,
        "{claimed_fixed} claimed-fixed, {reopened} re-opened, {worse} worse"
    );
    let pass = (claims.is_empty() || claimed_fixed > 0) && reopened == 0 && worse == 0;
    (table, pass)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_claimed_fix_is_enough() {
        let claims = ["first", "second"];
        let (table, pass) = compare([("first", 6, 10), ("second", 3, 3)].into_iter(), &claims);
        assert!(pass);
        assert!(table.contains("1 claimed-fixed, 0 re-opened, 0 worse"));
        assert!(!compare([("first", 6, 9)].into_iter(), &claims).1);
        assert!(!compare([("other", 6, 10)].into_iter(), &claims).1);
    }

    #[test]
    fn a_reopened_entry_fails_the_gate() {
        let (table, pass) = compare(
            [("fix", 0, 10), ("regression", 10, 9)].into_iter(),
            &["fix"],
        );
        assert!(!pass);
        assert!(table.contains("1 claimed-fixed, 1 re-opened, 0 worse"));
    }

    #[test]
    fn with_no_claims_four_fewer_passes_fails_and_three_passes() {
        let (table, pass) = compare([("regression", 6, 2)].into_iter(), &[]);
        assert!(!pass);
        assert!(table.contains("0 claimed-fixed, 0 re-opened, 1 worse"));
        assert!(compare([("regression", 6, 3)].into_iter(), &[]).1);
        assert!(!compare([("reopened", 10, 9)].into_iter(), &[]).1);
    }

    #[test]
    fn claims_must_name_suite_entries_or_none() {
        let entries = blunders::entries().unwrap();
        assert!(claims_list("none", &entries).unwrap().is_empty());
        assert_eq!(
            claims_list(&entries[0].name, &entries).unwrap(),
            [entries[0].name.as_str()]
        );
        assert!(claims_list("missing-entry", &entries).is_err());
    }
}
