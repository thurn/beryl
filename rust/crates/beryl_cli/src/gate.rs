//! The ladder's blunder suite gate (docs/ai-strategy.md §4.3).

use std::collections::BTreeMap;
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

    let first = Round::run(&entries, &champion, &challenger, 0)?;
    first.save(&out, None)?;
    first.check_drift()?;
    let close: Vec<Entry> = entries
        .iter()
        .zip(first.scores())
        .filter(|(_, (name, score))| score.is_close(claims.contains(name)))
        .map(|(entry, _)| entry.clone())
        .collect();
    let second = if close.is_empty() {
        None
    } else {
        let second = Round::run(&close, &champion, &challenger, SEEDS)?;
        first.save(&out, Some(&second))?;
        second.check_drift()?;
        Some(second)
    };
    let reruns = second.iter().flat_map(Round::scores).collect();
    println!("Reports saved to {}", out.display());
    println!("Champion {champion}\nChallenger {challenger}");
    let (table, pass) = compare(first.scores(), &claims, &reruns);
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

struct Round {
    champion: Report,
    challenger: Report,
}

impl Round {
    fn run(
        entries: &[Entry],
        champion: &str,
        challenger: &str,
        first: u32,
    ) -> Result<Self, String> {
        Ok(Self {
            champion: suite(entries, champion, "champion", first)?,
            challenger: suite(entries, challenger, "challenger", first)?,
        })
    }

    fn scores(&self) -> impl Iterator<Item = (&str, Score)> {
        self.champion
            .entries
            .iter()
            .zip(&self.challenger.entries)
            .map(|(before, after)| (before.name.as_str(), Score(passes(before), passes(after))))
    }

    fn check_drift(&self) -> Result<(), String> {
        if self.champion.drifted == 0 && self.challenger.drifted == 0 {
            return Ok(());
        }
        let reasons = self
            .champion
            .entries
            .iter()
            .zip(&self.challenger.entries)
            .filter_map(|(before, after)| before.drift.as_deref().or(after.drift.as_deref()))
            .collect::<Vec<_>>()
            .join("\n");
        Err(format!(
            "rebuild or retire the drifted entries (docs/ai-strategy.md §5)\n{reasons}"
        ))
    }

    fn save(&self, out: &Path, rerun: Option<&Self>) -> Result<(), String> {
        save(
            &out.join("champion.json"),
            &self.champion,
            rerun.map(|r| &r.champion),
        )?;
        save(
            &out.join("challenger.json"),
            &self.challenger,
            rerun.map(|r| &r.challenger),
        )
    }
}

fn suite(entries: &[Entry], family: &str, role: &str, first: u32) -> Result<Report, String> {
    eprintln!("{role}: {family}");
    blunders::run(entries, family, &BUDGETS, first, SEEDS, |line| {
        eprintln!("{role}: {line}");
    })
}

fn save(path: &Path, first: &Report, rerun: Option<&Report>) -> Result<(), String> {
    #[derive(serde::Serialize)]
    struct Saved<'a> {
        #[serde(flatten)]
        first: &'a Report,
        #[serde(skip_serializing_if = "Option::is_none")]
        rerun: Option<&'a Report>,
    }

    let json =
        serde_json::to_string_pretty(&Saved { first, rerun }).map_err(|e| e.to_string())? + "\n";
    files::write_whole(path, json.as_bytes())
}

fn passes(entry: &EntryReport) -> usize {
    entry.runs.iter().filter(|run| run.pass).count()
}

#[derive(Clone, Copy)]
struct Score(usize, usize);

#[derive(PartialEq, Eq)]
enum Mark {
    ClaimedFixed,
    Reopened,
    Worse,
    None,
}

impl Score {
    fn first_mark(self, claimed: bool) -> Mark {
        let Self(before, after) = self;
        if claimed && before < RUNS && after == RUNS {
            Mark::ClaimedFixed
        } else if before == RUNS && after < RUNS {
            Mark::Reopened
        } else if before.saturating_sub(after) >= 4 {
            Mark::Worse
        } else {
            Mark::None
        }
    }

    fn is_close(self, claimed: bool) -> bool {
        self.first_mark(claimed) == Mark::Reopened || (claimed && (7..RUNS).contains(&self.1))
    }

    fn rerun_mark(self, first: Mark) -> Mark {
        let Self(before, after) = self;
        match first {
            Mark::Reopened if before.saturating_sub(after) < 3 => Mark::None,
            Mark::None if after >= 18 && after.saturating_sub(before) >= 6 => Mark::ClaimedFixed,
            mark => mark,
        }
    }
}

fn compare<'a>(
    scores: impl Iterator<Item = (&'a str, Score)>,
    claims: &[&str],
    reruns: &BTreeMap<&str, Score>,
) -> (String, bool) {
    let mut table = format!(
        "{:<36} {:>8} {:>10}  mark\n",
        "entry", "champion", "challenger"
    );
    let (mut claimed_fixed, mut reopened, mut worse) = (0, 0, 0);
    for (name, score) in scores {
        let first = score.first_mark(claims.contains(&name));
        let (score, mark, runs, suffix) = match reruns.get(name) {
            Some(&Score(before, after)) => {
                let total = Score(score.0 + before, score.1 + after);
                (total, total.rerun_mark(first), 2 * RUNS, " (rerun)")
            }
            None => (score, first, RUNS, ""),
        };
        let mark = match mark {
            Mark::ClaimedFixed => {
                claimed_fixed += 1;
                "claimed-fixed"
            }
            Mark::Reopened => {
                reopened += 1;
                "re-opened"
            }
            Mark::Worse => {
                worse += 1;
                "worse"
            }
            Mark::None => "",
        };
        let Score(before, after) = score;
        let _ = writeln!(
            table,
            "{name:<36} {before:>5}/{runs} {after:>7}/{runs}  {mark}{suffix}"
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

    fn judge(
        scores: &[(&str, usize, usize)],
        claims: &[&str],
        reruns: &[(&str, usize, usize)],
    ) -> (String, bool) {
        let scores = scores
            .iter()
            .map(|&(name, before, after)| (name, Score(before, after)));
        let reruns = reruns
            .iter()
            .map(|&(name, before, after)| (name, Score(before, after)));
        compare(scores, claims, &reruns.collect())
    }

    #[test]
    fn one_claimed_fix_is_enough() {
        let claims = ["first", "second"];
        let (table, pass) = judge(&[("first", 6, 10), ("second", 3, 3)], &claims, &[]);
        assert!(pass);
        assert!(table.contains("1 claimed-fixed, 0 re-opened, 0 worse"));
        assert!(!judge(&[("first", 6, 6)], &claims, &[]).1);
        assert!(!judge(&[("other", 6, 10)], &claims, &[]).1);
    }

    #[test]
    fn a_reopened_entry_fails_the_gate_if_it_still_trails_after_the_rerun() {
        let (table, pass) = judge(
            &[("fix", 0, 10), ("regression", 10, 7)],
            &["fix"],
            &[("regression", 10, 9)],
        );
        assert!(!pass);
        assert!(table.contains("20/20      16/20  re-opened (rerun)"));
        assert!(table.contains("1 claimed-fixed, 1 re-opened, 0 worse"));
        assert!(!judge(&[("regression", 10, 7)], &[], &[("regression", 10, 10)]).1);
    }

    #[test]
    fn a_rerun_can_clear_a_reopened_entry() {
        assert!(Score(10, 7).is_close(false));
        let (table, pass) = judge(&[("regression", 10, 9)], &[], &[("regression", 10, 10)]);
        assert!(pass);
        assert!(table.contains("20/20      19/20   (rerun)"));
        assert!(judge(&[("regression", 10, 7)], &[], &[("regression", 9, 10)]).1);
    }

    #[test]
    fn a_close_claim_can_be_fixed_after_the_rerun() {
        assert!(Score(1, 7).is_close(true));
        assert!(Score(1, 8).is_close(true));
        assert!(!Score(1, 8).is_close(false));
        assert!(!Score(1, 6).is_close(true));
        assert!(!Score(1, 10).is_close(true));
        let first = [("fix", 1, 8)];
        let (table, pass) = judge(&first, &["fix"], &[("fix", 1, 10)]);
        assert!(pass);
        assert!(table.contains("2/20      18/20  claimed-fixed (rerun)"));
        assert!(!judge(&first, &["fix"], &[("fix", 1, 9)]).1);
        assert!(judge(&[("fix", 5, 8)], &["fix"], &[("fix", 7, 10)]).1);
        assert!(!judge(&[("fix", 5, 8)], &["fix"], &[("fix", 8, 10)]).1);
    }

    #[test]
    fn with_no_claims_four_fewer_passes_fails_and_three_passes() {
        let (table, pass) = judge(&[("regression", 6, 2)], &[], &[]);
        assert!(!pass);
        assert!(table.contains("6/10       2/10  worse"));
        assert!(table.contains("0 claimed-fixed, 0 re-opened, 1 worse"));
        assert!(judge(&[("regression", 6, 3)], &[], &[]).1);
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
