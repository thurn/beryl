//! The differential test of the interface test runner (docs/beryl-3d.md, "Test speed"): on a
//! fixed sample of tests, each kind of fault is planted at a seeded random tick, and every fault
//! that checking every tick finds must be found at the check times too. A fault only every tick
//! finds means a check time is missing.

use beryl_inspect::runner::{self, Test};
use beryl_scene::agent::{Fault, Rule};
use beryl_scene::driver::{Checked, Mode, StepMoment, TICK};
use beryl_scene::play::record::Planted;

/// How many tests the sample holds.
const SAMPLE: usize = 200;

/// The seed of the sample and of the ticks. Changing it changes which tests and ticks are
/// planted, never whether the check times are enough.
const SEED: u64 = 0x00b3_d7e5_7000_0001;

const KINDS: [(Planted, Rule); 5] = [
    (Planted::Jump, Rule::Jump),
    (Planted::OffPlacement, Rule::RestPlacement),
    (Planted::Duration, Rule::StepTime),
    (Planted::Unseen, Rule::StepShown),
    (Planted::Crowded, Rule::Crowded),
];

/// A small seeded generator (the `SplitMix64` algorithm), so that the sample needs no dependency.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number below `n`, which is not zero.
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).expect("below a usize")
    }
}

/// One planting: a test, a kind of fault and the tick it is planted at.
struct Planting {
    test: Test,
    kind: Planted,
    rule: Rule,
    at: u64,
}

/// What one planting found: the faults only every tick found, and whether the check times
/// found a fault of the planted kind.
struct Compared {
    missed: Vec<String>,
    found: bool,
}

fn keys(checked: &Checked) -> Vec<(Rule, Vec<String>)> {
    checked.faults.iter().map(key).collect()
}

fn key(fault: &Fault) -> (Rule, Vec<String>) {
    (
        fault.rule,
        fault.elements.iter().map(|e| e.locator.clone()).collect(),
    )
}

/// The sample: `SAMPLE` tests drawn from every test with the seeded generator, each with one
/// tick for each kind of fault, drawn inside one of its steps.
fn plantings() -> Vec<Planting> {
    // The stress boards are left out: they exist to count a frame's work at the engine's
    // capacity, and checking one on every tick takes hours.
    let tests: Vec<Test> = runner::tests("")
        .expect("the recordings pack opens")
        .into_iter()
        .filter(|test| !beryl_inspect::stress::NAMES.contains(&test.recording.as_str()))
        .collect();
    let mut random = Random(SEED);
    let mut drawn: Vec<usize> = Vec::new();
    while drawn.len() < SAMPLE * 4 && drawn.len() < tests.len() {
        let at = random.below(tests.len());
        if !drawn.contains(&at) {
            drawn.push(at);
        }
    }
    // Each drawn test's steps that begin before its recording ends, from its first play, with
    // the time of that end; a test with none has nowhere to plant. A step may run past the end
    // (an announcement that moves nothing), where no check looks.
    let steps = runner::each(&drawn, |&at| {
        runner::caught(|| {
            tests[at].page().map(|mut page| {
                let end = page.moments().last().map_or(0, |(_, at)| at.0);
                let steps: Vec<StepMoment> = page
                    .steps()
                    .into_iter()
                    .filter(|step| step.from <= end)
                    .collect();
                (steps, end)
            })
        })
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default()
    });
    let mut plantings = Vec::new();
    for (&at_test, (steps, end)) in drawn.iter().zip(steps) {
        if steps.is_empty() {
            continue;
        }
        if plantings.len() == SAMPLE * KINDS.len() {
            break;
        }
        for (kind, rule) in KINDS {
            let step = &steps[random.below(steps.len())];
            // A tick of the clock inside the step and the recording, or the step's start if
            // that span holds no tick.
            let first = step.from.div_ceil(TICK.0);
            let last = step.to.min(end) / TICK.0;
            let at = if last < first {
                step.from
            } else {
                (first + random.next() % (last - first + 1)) * TICK.0
            };
            plantings.push(Planting {
                test: tests[at_test].clone(),
                kind,
                rule,
                at,
            });
        }
    }
    assert_eq!(
        plantings.len(),
        SAMPLE * KINDS.len(),
        "fewer than {SAMPLE} of the drawn tests have a step to plant on"
    );
    plantings
}

/// Plants one fault, checks the test both ways and compares them.
fn plant(planting: &Planting) -> Result<Compared, String> {
    let mut page = planting.test.page()?;
    page.plant(planting.kind, planting.at)
        .map_err(|e| e.to_string())?;
    runner::caught(|| {
        let sparse = page.check_all(Mode::AtCheckTimes);
        let every = page.check_all(Mode::EveryTick);
        let found = keys(&sparse);
        let missed = every
            .faults
            .iter()
            .filter(|fault| !found.contains(&key(fault)))
            .map(|fault| {
                format!(
                    "{} at t={}ms: {}",
                    fault.rule.word(),
                    fault.at / 1_000_000,
                    fault.message
                )
            })
            .collect();
        Compared {
            missed,
            found: sparse
                .faults
                .iter()
                .any(|fault| fault.rule == planting.rule),
        }
    })
}

#[test]
fn the_check_times_find_every_planted_fault() {
    let plantings = plantings();
    let results = runner::each(&plantings, plant);
    let mut misses = Vec::new();
    let mut found = [0_usize; KINDS.len()];
    for (planting, result) in plantings.iter().zip(&results) {
        let kind = KINDS
            .iter()
            .position(|&(k, _)| k == planting.kind)
            .expect("a known kind");
        match result {
            Err(why) => misses.push(format!(
                "{} with {:?} planted at t={}ms could not be checked: {why}",
                planting.test.name(),
                planting.kind,
                planting.at / 1_000_000
            )),
            Ok(result) => {
                found[kind] += usize::from(result.found);
                for missed in &result.missed {
                    misses.push(format!(
                        "{} with {:?} planted at t={}ms: only every tick finds {missed}",
                        planting.test.name(),
                        planting.kind,
                        planting.at / 1_000_000
                    ));
                }
            }
        }
    }
    eprintln!(
        "{} plantings over {SAMPLE} tests; the check times found the planted kind {found:?} times \
         ({:?})",
        plantings.len(),
        KINDS.map(|(kind, _)| kind)
    );
    assert!(misses.is_empty(), "{}", misses.join("\n"));
    for ((kind, _), count) in KINDS.iter().zip(found) {
        assert!(
            count > 0,
            "no planted {kind:?} was found anywhere: the planting does nothing on these tests"
        );
    }
}
