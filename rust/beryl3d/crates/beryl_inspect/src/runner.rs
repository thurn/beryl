//! The interface test runner behind `just b3d test` (docs/beryl-3d.md, "Test speed").
//!
//! An interface test is one recording played from one seat in one window, with every interface
//! check applied at its check times, or on every tick in the sweep. Tests run on every core in
//! this one process, each thread taking the next test from a shared index: no engine, no
//! renderer and no process per test. The recordings come from the pack ([`crate::recordings`]),
//! so the engine runs only when the pack for the present sources is missing.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use app_client::driver::{Mode, Page};
use beryl_protocol::Seat;
use beryl_protocol::protocol::Side;
use kit_probe::agent::{Count, Fault, Judged, Layout, Rule};
use kit_ui::input::Size;

use crate::cli::options::side_of;
use crate::pages::{self, InspectionProfile, Windows};
use crate::{recordings, stress};

/// The prefix of every test's name.
pub const PREFIX: &str = "b3d:";

/// One recording, one seat, one window.
#[derive(Clone, Debug, PartialEq)]
pub struct Test {
    /// The recording's name as `just b3d list` prints it.
    pub recording: String,
    pub seat: Side,
    pub size: Size,
    /// Physical pixels to a logical point (`Page::with_scale`).
    pub scale: f32,
    pub profile: Option<&'static InspectionProfile>,
}

impl Test {
    /// `b3d:<recording>/<seat>/<width>x<height>`, with `@<scale>` after the size when the scale
    /// is not 1, which every other `just b3d` command takes in place of a recording's name.
    #[must_use]
    pub fn name(&self) -> String {
        let seat = match self.seat {
            Side::You => "you",
            Side::Opponent => "opponent",
        };
        let mut name = format!(
            "{PREFIX}{}/{seat}/{}x{}",
            self.recording, self.size.width, self.size.height
        );
        if (self.scale - 1.0).abs() > f32::EPSILON {
            let _ = write!(name, "@{}", self.scale);
        }
        name
    }

    /// The profile's layout for this window.
    #[must_use]
    pub fn layout(&self) -> Layout {
        self.profile.map_or_else(
            || pages::layout_for_size(self.size),
            |profile| Layout::for_size_class(profile.definition().size_class),
        )
    }

    /// The test's recording at its start, from its seat in its window, in its layout with the
    /// layout's safe area (`pages::safe_area`).
    ///
    /// # Errors
    ///
    /// When the recording cannot be read or the client cannot open it.
    pub fn page(&self) -> Result<Page, String> {
        let page = pages::page_with_profile(
            &self.recording,
            self.seat,
            self.layout(),
            self.size,
            self.profile,
        )?;
        Ok(page.with_scale(self.scale))
    }
}

/// What one test found.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub test: Test,
    /// Every fault, each once (see `Page::check_all`).
    pub faults: Vec<Fault>,
    /// How many frames the frame checks ran on.
    pub frames: usize,
    /// How much each rule judged.
    pub judged: Judged,
    /// Why the test could not run: its recording could not be read or recorded, or the client
    /// panicked playing it.
    pub error: Option<String>,
    /// How long it took on the clock of the thread that ran it.
    pub seconds: f64,
}

impl Outcome {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.error.is_none() && self.faults.is_empty()
    }
}

/// Every test whose name starts with `filter`, which may leave out the `b3d:` prefix: every
/// recording in the pack and every stress board, from each seat it is recorded for, in each of
/// `windows`, ordered by recording, then seat (yours first), then window.
///
/// # Errors
///
/// When the pack cannot be built or read.
pub fn tests(filter: &str, windows: Windows) -> Result<Vec<Test>, String> {
    let filter = full_name(filter);
    let prefix = filter.strip_prefix(PREFIX).unwrap_or_default();
    let recording_windows = windows.windows();
    let (profile, names) = if let Windows::Profile(profile) = windows {
        let fixtures = crate::corpus::fixtures()?;
        let names = recordings::names()?
            .into_iter()
            .filter(|(recording, _)| fixtures.contains(recording))
            .collect();
        (Some(profile), names)
    } else {
        (None, crate::scene::names(prefix)?)
    };
    let mut tests = Vec::new();
    for (recording, seat) in names {
        let story_windows = recording.starts_with(crate::stories::PREFIX).then(|| {
            let layout = crate::stories::open(&recording)
                .map_or(Layout::Desktop, |story| story.header().layout);
            match (layout, windows) {
                (Layout::Desktop, Windows::Phone) => Vec::new(),
                (Layout::Desktop, _) => Windows::Desktop.windows(),
                (Layout::Phone, _) => Windows::Phone.windows(),
            }
        });
        for window in story_windows.as_deref().unwrap_or(&recording_windows) {
            let test = Test {
                recording: recording.clone(),
                seat: side_of(seat),
                size: window.size,
                scale: window.scale,
                profile,
            };
            if test.name().starts_with(&filter) {
                tests.push(test);
            }
        }
    }
    Ok(tests)
}

/// `filter` with the `b3d:` prefix, which a filter may leave out.
#[must_use]
pub fn full_name(filter: &str) -> String {
    if filter.starts_with(PREFIX) {
        filter.to_owned()
    } else {
        format!("{PREFIX}{filter}")
    }
}

/// Runs `tests`, checking the frames `mode` names, on as many threads as the machine has cores,
/// and returns their outcomes in the order given. The longest are started first (by
/// [`recordings::weight`]), so that no thread is left with a long test after the rest finish.
#[must_use]
pub fn run(tests: &[Test], mode: Mode) -> Vec<Outcome> {
    run_until(tests, mode, None)
}

/// [`run`], except that no test starts after `stop`: the outcomes are those of the tests that
/// started before it, in the order given. The tests running at `stop` finish, and since the
/// longest start first, those are short.
#[must_use]
pub fn run_until(tests: &[Test], mode: Mode, stop: Option<Instant>) -> Vec<Outcome> {
    let mut order: Vec<usize> = (0..tests.len()).collect();
    order.sort_by_cached_key(|&at| {
        let test = &tests[at];
        let seat = match test.seat {
            Side::You => Seat::P0,
            Side::Opponent => Seat::P1,
        };
        std::cmp::Reverse(if test.recording.starts_with(crate::stories::PREFIX) {
            usize::MAX
        } else {
            recordings::weight(&test.recording, seat)
        })
    });
    let mut outcomes: Vec<Option<Outcome>> = (0..tests.len()).map(|_| None).collect();
    let played = each(&order, |&at| {
        let late = stop.is_some_and(|stop| Instant::now() >= stop);
        (!late).then(|| play(tests[at].clone(), mode))
    });
    for (at, outcome) in order.into_iter().zip(played) {
        outcomes[at] = outcome;
    }
    outcomes.into_iter().flatten().collect()
}

/// Plays one test.
fn play(test: Test, mode: Mode) -> Outcome {
    let started = Instant::now();
    let ran = caught(|| {
        if test.recording.starts_with(crate::stories::PREFIX) {
            crate::scene::Scene::open(
                &test.recording,
                test.seat,
                test.layout(),
                test.size,
                test.scale,
            )
            .map(|mut scene| scene.check_all(mode))
        } else {
            test.page().map(|mut page| page.check_all(mode))
        }
    })
    .and_then(|ran| ran);
    let (faults, frames, judged, error) = match ran {
        Ok(checked) => {
            let mut judged = checked.judged;
            for rule in Rule::ALL {
                if !stress::audited(&test.recording, rule) {
                    judged.0[rule.index()] = Count::default();
                }
            }
            (
                (checked.faults.into_iter())
                    .filter(|fault| stress::audited(&test.recording, fault.rule))
                    .collect(),
                checked.frames,
                judged,
                None,
            )
        }
        Err(why) => (Vec::new(), 0, Judged::default(), Some(why)),
    };
    Outcome {
        test,
        faults,
        frames,
        judged,
        error,
        seconds: started.elapsed().as_secs_f64(),
    }
}

/// `f()`, or the message it panicked with.
///
/// # Errors
///
/// When `f` panics.
pub fn caught<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    panic::catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .map_or_else(
                || "the client panicked".to_owned(),
                |why| format!("the client panicked: {why}"),
            )
    })
}

/// How many threads a command spreads its work over: the cores Ignite granted the command that
/// started it (`IGNITE_CORES`), or the CPU tokens the `by` command that started it leased from
/// the machine's pool (`BERYL_CPU_TOKENS`, set by `tools/src/beryl_tools/tokens.py`), or the
/// machine's cores when it runs outside both. With many sessions on one machine, each command
/// taking every core made them all slower at once.
#[must_use]
pub fn threads() -> usize {
    LEASE_SIZES
        .iter()
        .find_map(|name| {
            std::env::var(name)
                .ok()
                .and_then(|n| n.parse::<usize>().ok())
                .filter(|&n| n > 0)
        })
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, std::num::NonZero::get))
}

/// The variables that give a command's grant when it starts, Ignite's first.
const LEASE_SIZES: [&str; 2] = ["IGNITE_CORES", "BERYL_CPU_TOKENS"];

/// The variables naming the file in which a grant that changes while its command runs keeps its
/// size now: the one Ignite's scheduler writes, then the one a growing lease of
/// `tools/src/beryl_tools/tokens.py` writes (`run_program` with `grow`).
const GRANT_FILES: [&str; 2] = ["IGNITE_CORES_FILE", "BERYL_CPU_TOKENS_FILE"];

/// The file that [`GRANT_FILES`] names, the first one set.
fn grant_file() -> Option<std::ffi::OsString> {
    grant_file_in(|name| std::env::var_os(name))
}

/// The file that the first of [`GRANT_FILES`] set in `var` names.
fn grant_file_in(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<std::ffi::OsString> {
    GRANT_FILES.iter().find_map(|name| var(name))
}

/// The grant written in the file at `path`, when it holds a number above zero.
fn read_grant(path: Option<std::ffi::OsString>) -> Option<usize> {
    path.and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| text.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
}

/// How often [`spread`] reads the lease's size again while items remain.
const GROW_POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// How many threads a command may run now: the size its grant last wrote to the file one of
/// [`GRANT_FILES`] names, when its grant changes while it runs, or else [`threads`].
#[must_use]
pub fn granted() -> usize {
    read_grant(grant_file()).unwrap_or_else(threads)
}

/// Runs `work` on as many threads as [`granted`] allows, at most one per job, where each `work`
/// takes jobs from the shared index `next` until all `jobs` are taken. When the lease grows
/// while jobs remain, it starts more threads. A lease is otherwise fixed when the program
/// starts, so a program that started while the machine was busy ran on one or two threads
/// after the cores freed up: `b3d test` took 150 s instead of 11-13 s.
pub fn spread(jobs: usize, next: &AtomicUsize, work: impl Fn() + Sync) {
    spread_by(jobs, next, grant_file().is_some(), granted, work);
}

/// [`spread`], reading the threads allowed from `granted`, and again while jobs remain only
/// when `growing`.
fn spread_by(
    jobs: usize,
    next: &AtomicUsize,
    growing: bool,
    granted: impl Fn() -> usize,
    work: impl Fn() + Sync,
) {
    let jobs = jobs.max(1);
    let work = &work;
    std::thread::scope(|scope| {
        let mut started = 0;
        loop {
            while started < granted().min(jobs) {
                scope.spawn(work);
                started += 1;
            }
            if !growing || started >= jobs || next.load(Ordering::Relaxed) >= jobs {
                break;
            }
            std::thread::sleep(GROW_POLL);
        }
    });
}

/// `f` applied to every item, on as many threads as [`granted`] allows (see [`spread`]), each
/// thread taking the next item from a shared index, with the results in the order of `items`.
/// A panic in `f` is not reported by the default hook while it runs, since `f` reports its
/// own.
pub fn each<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    spread(items.len(), &next, || {
        loop {
            let at = next.fetch_add(1, Ordering::Relaxed);
            let Some(item) = items.get(at) else {
                break;
            };
            let result = f(item);
            results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)[at] = Some(result);
        }
    });
    panic::set_hook(hook);
    results
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .into_iter()
        .map(|result| result.expect("every item was run"))
        .collect()
}

/// The limits on the frames a test evaluates at its check times: `tests` in
/// `data/beryl3d/gates.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The median over the tests of the recorded fixtures ([`fixture_median`]).
    pub median_frames: usize,
    /// The most any one test evaluates.
    pub most_frames: usize,
}

impl Limits {
    /// The limits as `data/beryl3d/gates.json` holds them.
    ///
    /// # Errors
    ///
    /// When the file cannot be read, or its `tests` object lacks either limit.
    pub fn read() -> Result<Self, String> {
        let path = recordings::root().join("data/beryl3d/gates.json");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let gates: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let limit = |key: &str| {
            gates["tests"][key]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| format!("{}: `tests.{key}` is not a count", path.display()))
        };
        Ok(Self {
            median_frames: limit("median_frames")?,
            most_frames: limit("most_frames")?,
        })
    }

    /// What is over the limits in `outcomes`, one sentence each; empty when nothing is. Tests
    /// that could not run are left out. The median is held only when `outcomes` are every test.
    #[must_use]
    pub fn over(&self, outcomes: &[Outcome], every_test: bool) -> Vec<String> {
        let Some(frames) = Frames::of(outcomes) else {
            return Vec::new();
        };
        let mut over = Vec::new();
        let median = fixture_median(outcomes);
        if let Some(median) = median.filter(|m| every_test && *m > self.median_frames) {
            over.push(format!(
                "the median test of a recorded fixture evaluates {median} frames, over the \
                 limit of {} in data/beryl3d/gates.json (tests.median_frames)",
                self.median_frames
            ));
        }
        if frames.most > self.most_frames {
            over.push(format!(
                "{} evaluates {} frames, over the limit of {} in data/beryl3d/gates.json \
                 (tests.most_frames)",
                frames.costliest, frames.most, self.most_frames
            ));
        }
        over
    }
}

/// The median frames over the tests of the recorded fixtures that ran, or none when none ran.
/// A fixture's recording name has no `/`; a card's spec moment, `<card>/<moment>`, does. Spec
/// moments are left out because a change to cards alone adds them, and the fast check runs no
/// Beryl 3D lane for such a change. Over every test, the median moved with each new card
/// scenario, and the next Beryl 3D change to be pushed failed on a limit it had not moved.
#[must_use]
pub fn fixture_median(outcomes: &[Outcome]) -> Option<usize> {
    let mut counts: Vec<usize> = outcomes
        .iter()
        .filter(|o| o.error.is_none() && !o.test.recording.contains('/'))
        .map(|o| o.frames)
        .collect();
    counts.sort_unstable();
    counts.get(counts.len() / 2).copied()
}

/// The frames the tests that ran evaluated: the median, the most, and the test that evaluated
/// the most.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frames {
    pub median: usize,
    pub most: usize,
    pub costliest: String,
}

impl Frames {
    #[must_use]
    pub fn of(outcomes: &[Outcome]) -> Option<Self> {
        let ran: Vec<&Outcome> = outcomes.iter().filter(|o| o.error.is_none()).collect();
        let costliest = ran.iter().max_by_key(|o| o.frames)?;
        let mut counts: Vec<usize> = ran.iter().map(|o| o.frames).collect();
        counts.sort_unstable();
        Some(Self {
            median: counts[counts.len() / 2],
            most: costliest.frames,
            costliest: costliest.test.name(),
        })
    }
}

/// The lines the run log is given for `outcomes`, one JSON object each: the test's name,
/// whether it passed, and the seconds it took (runlog.py's results file). A test passes when it
/// is not among the names in `failing`, which the caller derives from the same comparison that
/// decides its exit status, so a fault the list allows is not a failure.
#[must_use]
pub fn results(outcomes: &[Outcome], failing: &BTreeSet<String>) -> String {
    let mut lines = String::new();
    for outcome in outcomes {
        let line = serde_json::json!({
            "check": outcome.test.name(),
            "outcome": if failing.contains(&outcome.test.name()) { "fail" } else { "pass" },
            "seconds": (outcome.seconds * 1000.0).round() / 1000.0,
        });
        let _ = writeln!(lines, "{line}");
    }
    lines
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use super::{Side, Test, Windows, grant_file_in, read_grant, run_until, spread_by};
    use app_client::driver::Mode;

    /// Under Ignite a command reads its grant from the file `IGNITE_CORES_FILE` names, which
    /// wins over the token pool's file when both are set.
    #[test]
    fn the_grant_comes_from_ignites_file_first() {
        let dir = std::env::temp_dir().join(format!("b3d-grant-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ignite = dir.join("ignite");
        let pool = dir.join("pool");
        std::fs::write(&ignite, "5\n").unwrap();
        std::fs::write(&pool, "2\n").unwrap();
        let both = |name: &str| match name {
            "IGNITE_CORES_FILE" => Some(ignite.clone().into_os_string()),
            "BERYL_CPU_TOKENS_FILE" => Some(pool.clone().into_os_string()),
            _ => None,
        };
        let pool_only =
            |name: &str| (name == "BERYL_CPU_TOKENS_FILE").then(|| pool.clone().into_os_string());
        assert_eq!(read_grant(grant_file_in(both)), Some(5));
        assert_eq!(read_grant(grant_file_in(pool_only)), Some(2));
        assert_eq!(read_grant(grant_file_in(|_| None)), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The first job raises the grant from one thread to four and waits until four threads have
    /// taken jobs, which only happens if `spread_by` starts more threads while it runs. Without
    /// growth it would give up after 5 s with one thread.
    #[test]
    fn a_growing_grant_starts_more_threads_while_jobs_remain() {
        let grant = AtomicUsize::new(1);
        let next = AtomicUsize::new(0);
        let threads = Mutex::new(HashSet::new());
        let seen = || threads.lock().map_or(0, |t| t.len());
        spread_by(
            8,
            &next,
            true,
            || grant.load(Ordering::Relaxed),
            || {
                while next.fetch_add(1, Ordering::Relaxed) < 8 {
                    if let Ok(mut t) = threads.lock() {
                        t.insert(std::thread::current().id());
                    }
                    if grant.swap(4, Ordering::Relaxed) == 1 {
                        let deadline = Instant::now() + Duration::from_secs(5);
                        while seen() < 4 && Instant::now() < deadline {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    } else {
                        // Long enough that one new thread cannot take every other job alone.
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
            },
        );
        assert_eq!(seen(), 4);
    }

    /// A stop that has passed starts no test, so the fast check's `--stop-after` names every
    /// test as deferred rather than letting the hard limit kill the run.
    #[test]
    fn no_test_starts_after_the_stop() {
        let tests: Vec<Test> = (0..3)
            .map(|n| Test {
                recording: format!("no-such-recording-{n}"),
                seat: Side::You,
                profile: None,
                size: Windows::Desktop.windows()[0].size,
                scale: 1.0,
            })
            .collect();
        assert!(run_until(&tests, Mode::AtCheckTimes, Some(Instant::now())).is_empty());
    }
}
