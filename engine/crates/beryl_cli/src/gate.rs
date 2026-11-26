//! The ladder's blunder suite results for the champion and a challenger.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use beryl_search::DEEP_SEARCHER;
use beryl_session::corpus::today;

use crate::Catalog;
use crate::blunders::{self, BUDGETS, Entry, EntryReport, Report};
use crate::game::parse_work;
use crate::ladder::{dropped_row, resolve};
use crate::match_log::{cache, files};

const USAGE: &str = "usage: beryl_cli gate <challenger family> --claims NAME,...|none \
                     [--vs CHAMPION FAMILY] [--out DIR] [--seeds N] [--work A,B]\n\
                     Reports both suites at 3M and 30M from 10 seeds, unless told otherwise. \
                     --out replaces the reports in DIR.";
const SEEDS: u32 = 10;

pub fn main(catalog: &'static Catalog, args: &[String]) -> Result<(), String> {
    let mut challenger = None;
    let mut champion = None;
    let mut claims = None;
    let mut out = None;
    let mut seeds = SEEDS;
    let mut budgets = BUDGETS.to_vec();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--claims" if claims.is_none() => claims = Some(value()?),
            "--vs" if champion.is_none() => champion = Some(value()?),
            "--out" if out.is_none() => out = Some(PathBuf::from(value()?)),
            "--seeds" => seeds = crate::number(arg, value()?)?,
            "--work" => {
                budgets = value()?
                    .split(',')
                    .map(|w| parse_work(w).ok_or_else(|| format!("--work: no budget {w:?}")))
                    .collect::<Result<_, _>>()?;
            }
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
    let date = today();
    let name = challenger.replace('/', "_");
    let out = out.unwrap_or_else(|| {
        cache()
            .join("ladder")
            .join("gates")
            .join(format!("{date}-{name}"))
    });
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let _lock = files::lock(&out)?;

    let champion = suite(catalog, &entries, &champion, "champion", &budgets, seeds)?;
    let challenger = suite(
        catalog,
        &entries,
        &challenger,
        "challenger",
        &budgets,
        seeds,
    )?;
    save(&out.join("champion.json"), &champion)?;
    save(&out.join("challenger.json"), &challenger)?;
    println!("Reports saved to {}", out.display());

    check_drift(&champion, &challenger)?;
    println!(
        "Champion {}\nChallenger {}",
        champion.agent, challenger.agent
    );
    print!("{}", compare(&date, &champion, &challenger, &claims));
    Ok(())
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

fn check_drift(champion: &Report, challenger: &Report) -> Result<(), String> {
    if champion.drifted == 0 && challenger.drifted == 0 {
        return Ok(());
    }
    let reasons = champion
        .entries
        .iter()
        .zip(&challenger.entries)
        .filter_map(|(before, after)| before.drift.as_deref().or(after.drift.as_deref()))
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "rebuild each drifted entry as a board in data/blunders/ or retire it\n{reasons}"
    ))
}

fn suite(
    catalog: &'static Catalog,
    entries: &[Entry],
    family: &str,
    role: &str,
    budgets: &[u64],
    seeds: u32,
) -> Result<Report, String> {
    eprintln!("{role}: {family}");
    blunders::run(catalog, entries, family, budgets, seeds, 1, |line| {
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

fn compare(date: &str, champion: &Report, challenger: &Report, claims: &[&str]) -> String {
    let mut table = format!(
        "{:<36} {:>8} {:>10} {:>5}  flags\n",
        "entry", "champion", "challenger", "delta"
    );
    let (mut champion_passes, mut challenger_passes) = (0, 0);
    let runs = champion.budgets.len() * champion.seeds as usize;
    for (before, after) in champion.entries.iter().zip(&challenger.entries) {
        let name = before.name.as_str();
        let (before, after) = (passes(before), passes(after));
        champion_passes += before;
        challenger_passes += after;
        let difference = before.abs_diff(after);
        let sign = if after >= before { '+' } else { '-' };
        let delta = format!("{sign}{difference}");
        let claim = if claims.contains(&name) {
            "  claim"
        } else {
            ""
        };
        let look = if difference >= 6 { "  look" } else { "" };
        let _ = writeln!(
            table,
            "{name:<36} {before:>5}/{runs} {after:>7}/{runs} {delta:>5}{claim}{look}"
        );
    }
    let total = champion.entries.len() * runs;
    let _ = writeln!(
        table,
        "Champion {champion_passes}/{total}, {} fixed\n\
         Challenger {challenger_passes}/{total}, {} fixed",
        champion.fixed, challenger.fixed
    );
    let suite = format!("{champion_passes}/{total} → {challenger_passes}/{total}");
    let _ = writeln!(
        table,
        "{}",
        dropped_row(date, &challenger.agent, &champion.agent, &suite)
    );
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNS: usize = BUDGETS.len() * SEEDS as usize;
    use crate::blunders::{Run, Standing};

    fn report(agent: &str, scores: &[(&str, usize)]) -> Report {
        let entries: Vec<_> = scores
            .iter()
            .map(|&(name, passed)| EntryReport {
                name: name.to_owned(),
                incident: String::new(),
                state: if passed >= 19 {
                    Standing::Fixed
                } else {
                    Standing::Open
                },
                runs: (0..RUNS)
                    .map(|run| Run {
                        budget: String::new(),
                        seed: 0,
                        chosen: Vec::new(),
                        pass: run < passed,
                    })
                    .collect(),
                drift: None,
            })
            .collect();
        let fixed = u32::try_from(
            entries
                .iter()
                .filter(|entry| entry.state == Standing::Fixed)
                .count(),
        )
        .unwrap();
        Report {
            agent: agent.to_owned(),
            budgets: vec![String::new(); BUDGETS.len()],
            seeds: SEEDS,
            open: u32::try_from(entries.len()).unwrap() - fixed,
            fixed,
            drifted: 0,
            entries,
        }
    }

    #[test]
    fn reports_suite_totals_and_a_dropped_row() {
        let mut before = vec![("unchanged", 20); 19];
        let mut after = before.clone();
        before.push(("claim", 18));
        after.push(("claim", 19));
        let champion = report("champion", &before);
        let challenger = report("challenger", &after);

        let table = compare("2026-11-14", &champion, &challenger, &["claim"]);
        assert!(table.contains("20/20      20/20    +0"));
        assert!(table.contains("18/20      19/20    +1  claim\n"));
        assert!(table.contains("Champion 398/400, 19 fixed\nChallenger 399/400, 20 fixed"));
        assert!(table.ends_with(
            "| 2026-11-14 | challenger | champion | - | - | - | - | dropped | 398/400 → 399/400 | ? |\n"
        ));
        assert!(!table.contains("GATE:"));
    }

    #[test]
    fn totals_are_out_of_the_runs_of_the_chosen_seeds_and_budgets() {
        let mut one = report("a", &[("e", 1)]);
        one.budgets.truncate(1);
        one.seeds = 1;
        let other = Report {
            agent: "b".into(),
            ..report("b", &[("e", 1)])
        };
        let other = Report {
            budgets: one.budgets.clone(),
            seeds: 1,
            ..other
        };
        let table = compare("2026-11-14", &one, &other, &[]);
        assert!(table.contains("1/1"), "{table}");
        assert!(table.contains("Champion 1/1,"), "{table}");
    }

    #[test]
    fn flags_differences_of_six_or_more_in_either_direction() {
        let champion = report("champion", &[("up", 0), ("down", 20), ("smaller", 13)]);
        let challenger = report("challenger", &[("up", 6), ("down", 13), ("smaller", 8)]);

        let table = compare("2026-11-14", &champion, &challenger, &["up"]);
        assert!(table.contains("+6  claim  look\n"));
        assert!(table.contains("-7  look\n"));
        assert!(table.contains("-5\n"));
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
