//! What one tip verification round claims and what it reports (see "How long a round takes" in
//! `docs/ignite.md`).
//!
//! - **Claims sized by cost.** A round claims one family's checks from the hub in batches, by
//!   family and batch number, so two machines never run the same checks. Every machine must
//!   therefore cut the same batches. The hub answers `POST /tips/costs` with the costs and the
//!   claim length a round at that tip uses, frozen the first time any machine asks, and
//!   [`cut`] divides the family's checks from those alone. A batch holds about
//!   [`TipCosts::claim_seconds`] of recorded cost. A check with no recorded cost counts as the
//!   family's median; a family with none starts at [`UNMEASURED_BATCH`] checks a claim and
//!   doubles with each claim.
//! - **The tip budget.** A round runs the best-scored checks within `[tip] seconds` of running
//!   checks first, and moves `master` once they pass; the rest run afterwards
//!   ([`RoundBudget`]). See "Tip verification" in `docs/ignite.md`.
//! - **Where a round's time went.** Each round sends one [`TipRound`] when it ends. A round in
//!   which more than [`OVERHEAD_SHARE`] of the time went to listing families and Ignite's own
//!   work between commands, and that took at least [`SMALLEST_ROUND_S`], asks the hub for an
//!   improvement ticket naming the larger part ([`TipRound::overhead`]). Only listing that
//!   nothing else ran beside counts: families are listed, built and run at once, so a listing
//!   that overlaps a build delays nothing. Time spent waiting for
//!   cores is reported apart and never files a ticket: the round runs in the `tip` class, so it
//!   waits by design whenever landings or agents hold the cores, and nothing in the round can
//!   cut that wait.
//! - **How a culprit search ended.** A regression report says whether the search found one
//!   landing, ran out of runs, or never ran ([`CulpritSearch`]), so its ticket never presents a
//!   range as if it were the culprit.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::Os;
use crate::ids::{CheckName, Commit, MachineId};
use crate::time::Timestamp;

/// The commits that bound a platform's unverified changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeferredPlatform {
    /// The first tip ready to advance with a deferral.
    pub since: Commit,
    /// Where master was before that deferral, for the returning round's culprit search.
    pub last_good: Commit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformVerification {
    pub through: Commit,
    /// Empty when this platform verified the tip at `through`.
    pub deferred: Option<DeferredPlatform>,
}

/// `GET /tips/coverage`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TipCoverage {
    pub platforms: BTreeMap<Os, PlatformVerification>,
    /// Machines on platforms where every registered machine is off.
    pub off_machines: BTreeMap<Os, Vec<String>>,
}

impl TipCoverage {
    pub fn warnings(&self) -> impl Iterator<Item = String> + '_ {
        self.platforms
            .iter()
            .filter_map(|(platform, verification)| {
                let gap = verification.deferred.as_ref()?;
                let since = &gap.since;
                Some(match self.off_machines.get(platform) {
                    Some(machines) => {
                        let names = machines.join(", ");
                        let verb = if machines.len() == 1 { "is" } else { "are" };
                        format!("{platform} checks have not run since {since}: {names} {verb} off")
                    }
                    None => format!("{platform} checks have been unverified since {since}"),
                })
            })
    }
}

/// `POST /tips/coverage/update`: replace a platform's coverage only if it still matches the read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipCoverageUpdate {
    pub platform: Os,
    pub expected: Option<PlatformVerification>,
    pub replacement: PlatformVerification,
}

/// How many checks the first claim of a family with no recorded cost holds. Each later claim
/// of that family holds twice as many as the one before.
pub const UNMEASURED_BATCH: usize = 50;

/// The share of a round's time that may go to listing and to Ignite's own work between
/// commands before the round asks for an improvement ticket.
pub const OVERHEAD_SHARE: f64 = 0.10;

/// A round shorter than this files nothing, whatever its shares.
pub const SMALLEST_ROUND_S: f64 = 120.0;

/// `POST /tips/costs`: the costs to cut one family's batches by at one tip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TipCostsRequest {
    pub commit: Commit,
    pub family: String,
    /// The asking machine's `tip.claim_seconds`. The first request for a tip and family fixes
    /// it, with the costs, for every machine.
    pub claim_seconds: f64,
}

/// The reply to `POST /tips/costs`, the same for every request about one tip and family.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TipCosts {
    pub claim_seconds: f64,
    /// Each check of the family the hub has a recorded cost for, in CPU seconds: the median of
    /// its recent costs, each an equal share of one command with the family's start-up taken
    /// out. The hub shortens `claim_seconds` by the start-up.
    pub costs: BTreeMap<String, f64>,
}

impl TipCosts {
    /// Costs that cut every family as one nobody has measured. A round that cannot reach the
    /// hub uses these, which every machine in the same position cuts alike.
    pub fn unmeasured(claim_seconds: f64) -> Self {
        Self {
            claim_seconds,
            costs: BTreeMap::new(),
        }
    }

    /// The median of the recorded costs among `names`, or `None` when none is recorded.
    fn median(&self, names: &[CheckName]) -> Option<f64> {
        let mut known: Vec<f64> = names
            .iter()
            .filter_map(|name| self.costs.get(name.as_str()).copied())
            .filter(|cost| cost.is_finite() && *cost >= 0.0)
            .collect();
        if known.is_empty() {
            return None;
        }
        known.sort_by(f64::total_cmp);
        Some(known[known.len() / 2])
    }
}

/// Cuts one family's checks into batches, in the order given, which callers sort by name. The
/// cut depends only on `names` and `costs`, so every machine that lists the same checks and is
/// given the same costs cuts the same batches. Each batch holds at least one check.
pub fn cut(names: &[CheckName], costs: &TipCosts) -> Vec<Vec<CheckName>> {
    let Some(median) = costs.median(names) else {
        let mut batches = Vec::new();
        let mut size = UNMEASURED_BATCH;
        let mut rest = names;
        while !rest.is_empty() {
            let (batch, after) = rest.split_at(size.min(rest.len()));
            batches.push(batch.to_vec());
            rest = after;
            size = size.saturating_mul(2);
        }
        return batches;
    };
    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut total = 0.0;
    for name in names {
        let cost = costs
            .costs
            .get(name.as_str())
            .copied()
            .filter(|cost| cost.is_finite() && *cost >= 0.0)
            .unwrap_or(median);
        batch.push(name.clone());
        total += cost;
        if total >= costs.claim_seconds {
            batches.push(std::mem::take(&mut batch));
            total = 0.0;
        }
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    batches
}

/// One finished tip verification round, as the daemon that ran it measured it with the `Clock`
/// trait (`POST /tips/rounds`). Times are seconds summed over every family the round ran, so
/// with families running at once they can add up to more than the round's wall time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TipRound {
    pub commit: Commit,
    pub machine: MachineId,
    /// When the round ended.
    pub at: Timestamp,
    /// The round's wall time, from listing the tip to its verdict.
    pub wall_s: f64,
    /// Checks the round ran, counting each rerun once.
    pub ran: u32,
    /// Checks the result cache answered.
    pub cached: u32,
    /// Time the check commands ran, apart from waiting for cores.
    pub checks_s: f64,
    /// Time the families' `list` commands ran, summed over families listed at once.
    pub listing_s: f64,
    /// Wall time during which some family's `list` command ran and no check command and no
    /// build did: the listing the round waited on. Listing that overlaps a build or another
    /// family's checks costs the round nothing, so only this part counts towards
    /// [`TipRound::overhead`]. `None` from a daemon built before it was measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listing_alone_s: Option<f64>,
    /// Time spent making build products current, waiting for them included.
    pub building_s: f64,
    /// Time commands waited for cores before they started.
    pub waiting_s: f64,
    /// How the round split its checks by the tip budget; `None` from a daemon built before the
    /// budget.
    #[serde(default)]
    pub budget: Option<RoundBudget>,
}

/// How a round split its checks by the tip budget. Costs are in billions of instructions, from
/// each check's recorded median cost.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RoundBudget {
    /// `[tip] seconds`.
    pub seconds: f64,
    /// Billions of instructions per second of wall time that seconds converted at: the hub's
    /// rate for `tip`-class check commands.
    pub rate: f64,
    /// Checks that ran before `master` moved whatever the budget: cached failures run again,
    /// and checks with an open regression ticket.
    pub forced: u32,
    pub forced_billions: f64,
    /// Checks chosen by score within the budget, which ran before `master` moved.
    pub within: u32,
    pub within_billions: f64,
    /// Checks past the budget, which ran after `master` moved, or wait for a later round when
    /// it did not move.
    pub after: u32,
    pub after_billions: f64,
    /// Seconds from the start of the round until `master` moved; `None` when it did not.
    pub master_s: Option<f64>,
    /// For each family, how many of the checks the round set out to run it never ran: those a
    /// newer tip kept from running ([`RoundBudget::ended_by`]), or, when `master` did not move,
    /// the checks past the budget, which wait for a later round. Empty when the round ran every
    /// check it set out to. The hub reads it to say why tip verification missed a check that a
    /// landing then found failing (see "Tip verification" in `docs/ignite.md`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unrun: BTreeMap<String, u32>,
    /// The newer `staging` tip that ended the round before it ran every check it set out to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_by: Option<Commit>,
}

impl RoundBudget {
    /// The budget in billions of instructions.
    pub fn billions(&self) -> f64 {
        self.seconds * self.rate
    }

    /// The split in one line, as the round's log line and `ignite status` show it.
    pub fn summary(&self) -> String {
        use crate::budget::billions_text as b;
        let master = self.master_s.map_or_else(
            || "master did not move".to_owned(),
            |s| format!("master moved after {:.1} minutes", s / 60.0),
        );
        format!(
            "budget {:.0} s at {:.1} billion/s = {} billion: {} checks within ({} billion), {} \
             forced ({} billion), {} after ({} billion); {master}",
            self.seconds,
            self.rate,
            b(self.billions()),
            self.within,
            b(self.within_billions),
            self.forced,
            b(self.forced_billions),
            self.after,
            b(self.after_billions),
        )
    }
}

/// A part of a round's time that Ignite could cut: neither running checks, nor building, nor
/// waiting for cores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overhead {
    Listing,
    /// Everything else: keying checks, asking the hub, claiming batches.
    Other,
}

impl Overhead {
    /// The part in words, as the improvement ticket names it.
    pub fn words(self) -> &'static str {
        match self {
            Overhead::Listing => "listing families",
            Overhead::Other => "Ignite's own work between commands",
        }
    }
}

impl TipRound {
    /// The round's time not accounted for by commands and builds: its wall time beyond the
    /// sum of the measured parts, or zero when families running at once add up to more.
    pub fn other_s(&self) -> f64 {
        (self.wall_s - self.checks_s - self.listing_s - self.building_s - self.waiting_s).max(0.0)
    }

    /// The whole of the round's time: the measured parts and the rest.
    pub fn total_s(&self) -> f64 {
        self.checks_s + self.listing_s + self.building_s + self.waiting_s + self.other_s()
    }

    /// The larger of listing and Ignite's own work, and its share of the round, when the two
    /// come to more than [`OVERHEAD_SHARE`] of a round of at least [`SMALLEST_ROUND_S`].
    /// `None` otherwise.
    ///
    /// Listing counts only while nothing else of the round ran ([`TipRound::listing_alone_s`]),
    /// as a share of the round's wall time. The summed `listing_s` once filed a ticket every
    /// hour or so: 3.4 minutes of `list` commands, mostly beside builds, were 13% of the 26.2
    /// minutes the parts added up to, although the round took 19.8 minutes and may not have
    /// waited on any listing. A record from before the measure falls back to the summed time
    /// over the summed parts.
    ///
    /// Waiting for cores is left out. The round's commands ask for cores in the `tip` class,
    /// which yields to `land` and `agent` by design, so a round that runs beside landings
    /// waits whatever the round does. The wait still shows in [`TipRound::summary`].
    pub fn overhead(&self) -> Option<(Overhead, f64)> {
        let (listing, whole) = match self.listing_alone_s {
            Some(alone) => (alone, self.wall_s),
            None => (self.listing_s, self.total_s()),
        };
        if self.wall_s < SMALLEST_ROUND_S || whole <= 0.0 {
            return None;
        }
        let parts = [
            (Overhead::Listing, listing),
            (Overhead::Other, self.other_s()),
        ];
        let overhead: f64 = parts.iter().map(|(_, s)| s).sum();
        if overhead / whole <= OVERHEAD_SHARE {
            return None;
        }
        let (largest, seconds) = parts
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((Overhead::Other, 0.0));
        Some((largest, seconds / whole))
    }

    /// The round's figures in one line, in minutes, as `ignite status` and the dashboard show
    /// them.
    pub fn summary(&self) -> String {
        let minutes = |seconds: f64| format!("{:.1}", seconds / 60.0);
        let listing = match self.listing_alone_s {
            Some(alone) => format!(
                "{} listing ({} with nothing else running)",
                minutes(self.listing_s),
                minutes(alone)
            ),
            None => format!("{} listing", minutes(self.listing_s)),
        };
        format!(
            "{} checks run, {} cached; minutes: {} checks, {listing}, {} building, {} waiting \
             for cores, {} other, {} in all",
            self.ran,
            self.cached,
            minutes(self.checks_s),
            minutes(self.building_s),
            minutes(self.waiting_s),
            minutes(self.other_s()),
            minutes(self.wall_s),
        ) + &self
            .budget
            .as_ref()
            .map(|budget| format!("; {}", budget.summary()))
            .unwrap_or_default()
    }
}

/// The reply to `POST /tips/rounds`: the improvement ticket the round filed or found open, if
/// it asked for one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipRoundFiled {
    pub ticket: Option<String>,
    /// True when the ticket was filed for this round, false when an open one was reused.
    #[serde(default)]
    pub created: bool,
}

/// The reply to `GET /tips/rounds/last`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LastTipRound {
    pub round: Option<TipRound>,
}

/// How a culprit search ended, as a regression report carries it (see "Finding the culprit"
/// in `docs/ignite.md`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CulpritSearch {
    /// Only one landing between the commits could have changed the result, so it is the
    /// culprit, and nothing was run.
    OneCandidate,
    /// Running the check at commits between the candidates narrowed them to one landing.
    Bisected { runs: u32 },
    /// The search ended with several landings: `candidates` of them could have changed the
    /// result, and `why` says what stopped it.
    EndedEarly {
        runs: u32,
        candidates: u32,
        why: String,
    },
    /// No search ran, for the reason given; the report names the whole range since the last
    /// verified commit.
    NotRun { why: String },
}

impl CulpritSearch {
    /// The search in one sentence, as the regression ticket says it.
    pub fn words(&self) -> String {
        let runs = |runs: u32| {
            if runs == 1 {
                "1 run".to_owned()
            } else {
                format!("{runs} runs")
            }
        };
        match self {
            CulpritSearch::OneCandidate => {
                "the only landing in the range that could change the check's result; nothing \
                 was run"
                    .to_owned()
            }
            CulpritSearch::Bisected { runs: count } => {
                format!(
                    "found by running the check at earlier commits ({})",
                    runs(*count)
                )
            }
            CulpritSearch::EndedEarly {
                runs: count,
                candidates,
                why,
            } => format!(
                "the search ended early after {}, with {candidates} candidate landings, \
                 because {why}; the culprit is one of the landings between the commits below",
                runs(*count)
            ),
            CulpritSearch::NotRun { why } => format!(
                "no search ran, because {why}; the culprit is one of the commits between the \
                 commits below"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(count: usize) -> Vec<CheckName> {
        (0..count)
            .map(|i| CheckName::new(format!("unit:t{i:05}")).unwrap())
            .collect()
    }

    fn costing(names: &[CheckName], each: f64) -> TipCosts {
        TipCosts {
            claim_seconds: 60.0,
            costs: names.iter().map(|n| (n.to_string(), each)).collect(),
        }
    }

    fn sizes(batches: &[Vec<CheckName>]) -> Vec<usize> {
        batches.iter().map(Vec::len).collect()
    }

    /// A kernel change made tip verification launch the program behind 9,153 interface checks
    /// 183 times, one launch per 50 checks, although one launch runs them all in seconds.
    #[test]
    fn nine_thousand_checks_of_ten_milliseconds_are_one_or_two_claims() {
        let names = names(9_000);
        let batches = cut(&names, &costing(&names, 0.01));
        assert!(batches.len() <= 2, "{:?}", sizes(&batches));
        assert_eq!(batches.concat(), names);
    }

    #[test]
    fn checks_of_thirty_seconds_are_claimed_two_at_a_time() {
        let names = names(7);
        assert_eq!(sizes(&cut(&names, &costing(&names, 30.0))), [2, 2, 2, 1]);
    }

    #[test]
    fn a_check_with_no_cost_counts_as_the_family_median() {
        let names = names(6);
        let mut costs = costing(&names[..3], 20.0);
        costs.costs.insert(names[1].to_string(), 10.0);
        // The median of 20, 10 and 20 is 20, so the three unmeasured checks cost 20 each.
        assert_eq!(sizes(&cut(&names, &costs)), [4, 2]);
    }

    #[test]
    fn a_family_nobody_measured_starts_at_fifty_and_doubles() {
        let names = names(400);
        assert_eq!(
            sizes(&cut(&names, &TipCosts::unmeasured(60.0))),
            [50, 100, 200, 50]
        );
        assert!(cut(&[], &TipCosts::unmeasured(60.0)).is_empty());
    }

    fn round(wall_s: f64, listing_s: f64) -> TipRound {
        TipRound {
            commit: Commit::new("abcd").unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            at: Timestamp::EPOCH,
            wall_s,
            ran: 10,
            cached: 5,
            checks_s: wall_s - listing_s,
            listing_s,
            listing_alone_s: None,
            building_s: 0.0,
            waiting_s: 0.0,
            budget: None,
        }
    }

    /// A round at b363ee27d: 3.4 minutes of `list` commands in a 19.8-minute round whose parts, run
    /// at once, added up to 26.2 minutes. Summed, listing was 13% and filed a ticket; what
    /// counts is the listing nothing else ran beside.
    #[test]
    fn only_listing_with_nothing_else_running_files() {
        let observed = TipRound {
            checks_s: 612.0,
            listing_s: 204.0,
            building_s: 672.0,
            waiting_s: 84.0,
            ..round(1188.0, 0.0)
        };
        let mut overlapped = TipRound {
            listing_alone_s: Some(30.0),
            ..observed.clone()
        };
        assert_eq!(overlapped.overhead(), None);
        assert!(
            overlapped
                .summary()
                .contains("3.4 listing (0.5 with nothing else running)"),
            "{}",
            overlapped.summary()
        );
        overlapped.listing_alone_s = Some(180.0);
        let (part, share) = overlapped.overhead().unwrap();
        assert_eq!(part, Overhead::Listing);
        assert!((share - 180.0 / 1188.0).abs() < 1e-9, "{share}");
        // A record from a daemon that did not measure it keeps the summed rule.
        assert_eq!(observed.overhead().unwrap().0, Overhead::Listing);
    }

    #[test]
    fn a_round_with_much_overhead_names_its_largest_part() {
        let (part, share) = round(600.0, 180.0).overhead().unwrap();
        assert_eq!(part, Overhead::Listing);
        assert!((share - 0.3).abs() < 1e-9, "{share}");
        // Ten percent or less files nothing, nor does a short round.
        assert_eq!(round(600.0, 60.0).overhead(), None);
        assert_eq!(round(100.0, 90.0).overhead(), None);
    }

    /// A round at 08c542cb6 waited 26% of its time for cores while three landings held them
    /// in the higher `land` class, and filed a ticket asking to cut that wait.
    #[test]
    fn waiting_for_cores_never_files_a_ticket() {
        let mut waited = round(600.0, 10.0);
        waited.waiting_s = 300.0;
        waited.checks_s = 290.0;
        assert_eq!(waited.overhead(), None);
        assert!(waited.summary().contains("5.0 waiting for cores"));
        // That round's own figures: its listing and Ignite's own work still come to 26%.
        let observed = TipRound {
            checks_s: 36.0,
            listing_s: 24.0,
            building_s: 42.0,
            waiting_s: 42.0,
            ..round(162.0, 0.0)
        };
        let (part, share) = observed.overhead().unwrap();
        assert_eq!(part, Overhead::Listing);
        assert!((share - 24.0 / 162.0).abs() < 1e-9, "{share}");
    }

    #[test]
    fn a_culprit_search_says_how_it_ended() {
        assert_eq!(
            CulpritSearch::Bisected { runs: 1 }.words(),
            "found by running the check at earlier commits (1 run)"
        );
        let ended = CulpritSearch::EndedEarly {
            runs: 10,
            candidates: 40,
            why: "it reached its limit of 10 runs".to_owned(),
        };
        let json = serde_json::to_string(&ended).unwrap();
        assert!(json.contains(r#""kind":"ended_early""#), "{json}");
        assert_eq!(serde_json::from_str::<CulpritSearch>(&json).unwrap(), ended);
    }

    #[test]
    fn the_summary_names_the_budget_and_when_master_moved() {
        let mut round = round(600.0, 60.0);
        assert!(!round.summary().contains("budget"));
        round.budget = Some(RoundBudget {
            seconds: 300.0,
            rate: 39.5,
            forced: 1,
            forced_billions: 2.0,
            within: 40,
            within_billions: 9_000.0,
            after: 1_200,
            after_billions: 200_000.0,
            master_s: Some(270.0),
            ..RoundBudget::default()
        });
        let summary = round.summary();
        assert!(
            summary.ends_with(
                "budget 300 s at 39.5 billion/s = 11,850 billion: 40 checks within (9,000 \
                 billion), 1 forced (2 billion), 1200 after (200,000 billion); master moved \
                 after 4.5 minutes"
            ),
            "{summary}"
        );
        // A record from before the budget still reads.
        let old: TipRound = serde_json::from_str(
            r#"{"commit":"abcd","machine":"m-1","at":"1970-01-01T00:00:00Z","wall_s":1.0,
                "ran":0,"cached":0,"checks_s":0.0,"listing_s":0.0,"building_s":0.0,
                "waiting_s":0.0}"#,
        )
        .unwrap();
        assert_eq!(old.budget, None);
    }
}
