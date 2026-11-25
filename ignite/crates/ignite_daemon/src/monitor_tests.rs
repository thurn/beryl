use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use ignite_core::ids::Commit;

use super::*;

const GOOD: &str = "aaaaaaa";
const BAD: &str = "bbbbbbb";

pub(crate) fn at(seconds: f64) -> Timestamp {
    Timestamp::from_millis(1_800_000_000_000).plus_seconds(seconds)
}

fn commit(hex: &str) -> Commit {
    Commit::new(hex).unwrap()
}

/// A daemon whose process state and answers a test sets, recording what the monitor does.
pub(crate) struct FakeMachine {
    pub(crate) running: bool,
    pub(crate) answers: bool,
    /// On the hub's machine, whether its hub answers; `None` elsewhere.
    hub: Option<bool>,
    /// Whether an older build wrote the daemon's login service; a refresh makes it current.
    stale_service: Cell<bool>,
    /// The app the machine runs in CLI mode; `None` on a machine with none.
    pub(crate) cli_app: Option<justiciar::JusticiarApp>,
    /// Whether a headless justiciar turn holds its lock. A started turn holds it.
    pub(crate) turn_lock: Cell<bool>,
    /// The headless justiciar turns started.
    pub(crate) turns: RefCell<Vec<justiciar::JusticiarTurn>>,
    /// The trees roots the daemon recorded.
    pub(crate) roots: Vec<PathBuf>,
    /// Whether the trees roots answer their probe.
    pub(crate) storage_answers: Cell<bool>,
    /// This machine's `ignite.toml` policy for a stuck root.
    pub(crate) policy: RebootPolicy,
    /// When the machine last started.
    pub(crate) booted: Cell<Option<Timestamp>>,
    /// Whether the hub takes a report.
    pub(crate) hub_takes_reports: Cell<bool>,
    log: RefCell<Vec<String>>,
}

impl FakeMachine {
    pub(crate) fn new(running: bool, answers: bool) -> Self {
        Self {
            running,
            answers,
            hub: None,
            stale_service: Cell::new(true),
            cli_app: None,
            turn_lock: Cell::new(false),
            turns: RefCell::default(),
            roots: Vec::new(),
            storage_answers: Cell::new(true),
            policy: RebootPolicy::default(),
            booted: Cell::new(None),
            hub_takes_reports: Cell::new(true),
            log: RefCell::default(),
        }
    }

    /// A machine whose daemon answers and whose trees root `/trees` does not, under `policy`.
    pub(crate) fn stuck(policy: RebootPolicy) -> Self {
        let machine = Self {
            roots: vec![PathBuf::from("/trees")],
            policy,
            ..Self::new(true, true)
        };
        machine.storage_answers.set(false);
        machine
    }

    fn with_hub(mut self, answers: bool) -> Self {
        self.hub = Some(answers);
        self
    }

    pub(crate) fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

impl Machine for FakeMachine {
    fn daemon_running(&self) -> bool {
        self.running
    }
    fn probe(&self) -> bool {
        self.answers
    }
    fn kickstart(&self) -> std::io::Result<()> {
        self.log.borrow_mut().push("kickstart".to_owned());
        Ok(())
    }
    fn notify(&self, title: &str, _text: &str) {
        self.log.borrow_mut().push(format!("notify {title}"));
    }
    fn hub_here(&self) -> bool {
        self.hub.is_some()
    }
    fn probe_hub(&self) -> bool {
        self.hub
            .expect("the hub is probed only on the hub's machine")
    }
    fn kickstart_hub(&self) -> std::io::Result<()> {
        self.log.borrow_mut().push("kickstart hub".to_owned());
        Ok(())
    }
    fn refresh_daemon(&self) -> std::io::Result<bool> {
        let stale = self.stale_service.replace(false);
        if stale {
            self.log.borrow_mut().push("refresh daemon".to_owned());
        }
        Ok(stale)
    }
    fn justiciar_app(&self) -> Option<justiciar::JusticiarApp> {
        self.cli_app.clone()
    }
    fn locked(&self, _lock: &Path) -> bool {
        self.turn_lock.get()
    }
    fn start_justiciar(&self, turn: &justiciar::JusticiarTurn) -> std::io::Result<()> {
        self.turn_lock.set(true);
        self.turns.borrow_mut().push(turn.clone());
        Ok(())
    }
    fn daemon_log_tail(&self, lines: usize) -> String {
        format!("daemon log, last {lines} lines")
    }
    fn trees_roots(&self) -> Vec<PathBuf> {
        self.roots.clone()
    }
    fn probe_storage(&self, _root: &Path) -> bool {
        self.storage_answers.get()
    }
    fn reboot_policy(&self) -> RebootPolicy {
        self.policy.clone()
    }
    fn booted_at(&self) -> Option<Timestamp> {
        self.booted.get()
    }
    fn report_storage(&self, stuck: &StuckStorage) -> Result<(), String> {
        if !self.hub_takes_reports.get() {
            return Err("the hub is down".to_owned());
        }
        self.log
            .borrow_mut()
            .push(format!("report {}", stuck.root.display()));
        Ok(())
    }
    fn restart(&self) -> std::io::Result<()> {
        self.log.borrow_mut().push("restart".to_owned());
        Ok(())
    }
    fn run_command(&self, command: &[String]) -> std::io::Result<()> {
        self.log
            .borrow_mut()
            .push(format!("run {}", command.join(" ")));
        Ok(())
    }
}

pub(crate) fn home() -> Home {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let root = ignite_core::test_files::temp_dir().join(format!(
        "ignite-monitor-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("bin")).unwrap();
    Home { root }
}

fn seen(running: bool, answered: bool) -> Observation {
    Observation {
        running,
        answered,
        crash_loop: None,
        storage: None,
        reboot: RebootPolicy::default(),
        booted_at: None,
    }
}

#[cfg(windows)]
#[test]
fn the_windows_monitor_treats_a_held_daemon_lock_as_running() {
    let home = home();
    let _lock = crate::machine_lock::hold(&home).unwrap();
    let monitor = NativeMachine {
        socket: home.root.join("unused-socket"),
        home: home.root.clone(),
    };
    assert!(monitor.daemon_running());
}

#[cfg(windows)]
#[test]
fn the_windows_monitor_reads_a_failed_round_after_the_loop_recovers() {
    use std::sync::Arc;

    use ignite_core::fakes::FakeClock;

    let home = home();
    let failed_at = at(30.0);
    {
        let clock = Arc::new(FakeClock::new(at(0.0)));
        let health =
            crate::loop_health::LoopHealth::new(clock.clone()).with_daemon_log(home.root.clone());
        let ticker = health.register("ignite-main", Duration::from_secs(60));
        clock.advance(30.0);
        assert_eq!(
            ticker
                .round(|| Err::<(), _>("main checkout not fast-forwarded: fetch timed out".into())),
            None,
        );
        clock.advance(30.0);
        assert_eq!(ticker.round(|| Ok(())), Some(()));
        assert_eq!(health.reports()[0].consecutive_errors, 0);
    }

    let monitor = NativeMachine {
        socket: home.root.join("unused-socket"),
        home: home.root,
    };
    let tail = monitor.daemon_log_tail(1);
    assert!(
        tail.starts_with(&format!("{failed_at} [ignite-main]")),
        "{tail}"
    );
    assert!(
        tail.ends_with("round failed: main checkout not fast-forwarded: fetch timed out"),
        "{tail}"
    );
}

#[test]
fn a_daemon_that_answers_resets_the_failures() {
    let mut state = MonitorState {
        consecutive_failures: 2,
        failing_since: Some(at(0.0)),
        kickstarts: 1,
        ..MonitorState::default()
    };
    assert!(decide(&mut state, &seen(true, true), at(120.0)).is_empty());
    assert_eq!(
        state,
        MonitorState {
            last_ok: Some(at(120.0)),
            ..MonitorState::default()
        }
    );
}

#[test]
fn the_third_missed_probe_kickstarts_but_the_first_two_do_not() {
    let mut state = MonitorState::default();
    assert!(decide(&mut state, &seen(true, false), at(0.0)).is_empty());
    assert!(decide(&mut state, &seen(true, false), at(120.0)).is_empty());
    assert_eq!(
        decide(&mut state, &seen(true, false), at(240.0)),
        [Action::Kickstart]
    );
    assert_eq!(state.consecutive_failures, 3);
    assert_eq!(state.kickstarts, 1);
}

#[test]
fn a_missing_daemon_process_is_kickstarted_at_once_but_not_every_run() {
    let mut state = MonitorState::default();
    assert_eq!(
        decide(&mut state, &seen(false, false), at(0.0)),
        [Action::Kickstart]
    );
    // The next run, two minutes later, leaves a daemon that may still be starting alone.
    assert!(decide(&mut state, &seen(false, false), at(120.0)).is_empty());
    assert_eq!(
        decide(&mut state, &seen(false, false), at(310.0)),
        [Action::Kickstart]
    );
}

#[test]
fn fifteen_minutes_of_failing_after_a_kickstart_asks_for_a_repair_once() {
    let mut state = MonitorState::default();
    let mut asked = Vec::new();
    for run in 0..40 {
        let now = f64::from(run) * 120.0;
        if decide(&mut state, &seen(true, false), at(now)).contains(&Action::AskForRepair) {
            asked.push(now);
        }
    }
    // Failing since 0 s; the first run at or after 900 s is 960 s. The next request waits an
    // hour after it.
    assert_eq!(asked, [960.0, 4560.0]);
}

#[test]
fn a_crash_loop_rolls_back_whether_or_not_the_daemon_answers() {
    let looped = CrashLoop {
        bad: commit(BAD),
        to: commit(GOOD),
        starts: 3,
    };
    for answered in [false, true] {
        let mut state = MonitorState::default();
        let observation = Observation {
            crash_loop: Some(looped.clone()),
            ..seen(true, answered)
        };
        assert_eq!(
            decide(&mut state, &observation, at(0.0)),
            [Action::RollBack(looped.clone())]
        );
    }
}

#[test]
fn a_run_records_its_state_and_kickstarts_after_three_missed_probes() {
    let home = home();
    start_log(&home).record(at(0.0), &commit(BAD), 5).unwrap();
    let machine = FakeMachine::new(true, false);
    for run in 0..3 {
        run_once(&home, &machine, at(f64::from(run) * 120.0));
    }
    assert_eq!(machine.log(), ["kickstart"]);
    // The run that did not answer is hung: the kickstart's SIGTERM records no clean stop.
    let log = start_log(&home);
    assert!(log.read()[0].hung);
    assert!(
        !log.mark_stopped(5, at(0.0), at(250.0), "asked to stop")
            .unwrap()
    );
    assert!(log.read()[0].counts());
    let state: MonitorState =
        serde_json::from_slice(&fs::read(home.root.join("monitor.json")).unwrap()).unwrap();
    assert_eq!(state.consecutive_failures, 3);
    assert_eq!(state.last_ok, None);

    let machine = FakeMachine::new(true, true);
    run_once(&home, &machine, at(400.0));
    let state: MonitorState =
        serde_json::from_slice(&fs::read(home.root.join("monitor.json")).unwrap()).unwrap();
    assert_eq!(
        (state.consecutive_failures, state.last_ok),
        (0, Some(at(400.0)))
    );
}

#[test]
fn a_daemon_that_stays_down_writes_the_repair_request_and_notifies_and_recovery_removes_it() {
    let home = home();
    let machine = FakeMachine::new(false, false);
    for run in 0..9 {
        run_once(&home, &machine, at(f64::from(run) * 120.0));
    }
    assert!(
        machine
            .log()
            .contains(&"notify Ignite needs a repair".to_owned())
    );
    let request: serde_json::Value =
        serde_json::from_slice(&fs::read(home.root.join("justiciar-needed.json")).unwrap())
            .unwrap();
    assert!(request["at"].is_string());
    assert!(request["reason"].as_str().unwrap().contains("kickstarting"));
    assert!(request["evidence"].as_str().unwrap().contains("kickstarts"));

    run_once(&home, &FakeMachine::new(true, true), at(2000.0));
    assert!(!home.root.join("justiciar-needed.json").exists());
}

#[test]
fn a_run_rolls_back_a_crash_looping_current_build_and_kickstarts() {
    let home = home();
    let store = home.store();
    for hex in [GOOD, BAD] {
        let built = home.root.join(format!("built-{hex}"));
        fs::write(&built, hex).unwrap();
        store.store(&commit(hex), &built).unwrap();
    }
    store.mark_last_good(&commit(GOOD)).unwrap();
    store.point_current_at(&commit(BAD)).unwrap();
    for seconds in [0.0, 40.0, 80.0] {
        start_log(&home)
            .record(at(seconds), &commit(BAD), 5)
            .unwrap();
    }
    let machine = FakeMachine::new(true, true);
    run_once(&home, &machine, at(100.0));
    assert_eq!(store.current_commit(), Some(commit(GOOD)));
    assert!(store.is_rejected(&commit(BAD)));
    assert_eq!(machine.log(), ["kickstart"]);
    assert_eq!(fs::read_dir(home.root.join("crash")).unwrap().count(), 1);
}

#[test]
fn an_unreadable_state_file_starts_afresh() {
    let home = home();
    fs::write(home.root.join("monitor.json"), "{ not json").unwrap();
    assert_eq!(load(&home), MonitorState::default());
    let _: PathBuf = state_file(&home);
}

#[test]
fn a_hub_that_stops_answering_is_kickstarted_after_two_runs_and_then_rests() {
    let home = home();
    let silent = FakeMachine::new(true, true).with_hub(false);
    assert!(run_once(&home, &silent, at(0.0)).is_empty());
    let done = run_once(&home, &silent, at(120.0));
    assert_eq!(done, ["the hub did not answer; kickstarted it"]);
    assert_eq!(silent.log(), ["kickstart hub"]);
    // Not again while it may still be starting.
    assert!(run_once(&home, &silent, at(240.0)).is_empty());
    assert_eq!(
        run_once(&home, &silent, at(120.0 + KICKSTART_GAP_S)),
        ["the hub did not answer; kickstarted it"]
    );
    // The daemon answered throughout and was left alone.
    assert!(!silent.log().contains(&"kickstart".to_owned()));
}

#[test]
fn a_hub_that_answers_again_starts_the_count_over() {
    let mut state = MonitorState::default();
    assert!(!decide_hub(&mut state, true, false, at(0.0)));
    assert!(!decide_hub(&mut state, true, true, at(120.0)));
    assert!(!decide_hub(&mut state, true, false, at(240.0)));
    assert!(decide_hub(&mut state, true, false, at(360.0)));
}

#[test]
fn a_machine_without_the_hub_service_never_probes_or_kicks_a_hub() {
    let home = home();
    // `probe_hub` panics if it is called here.
    let plain = FakeMachine::new(true, true);
    for run in 0..4 {
        assert!(run_once(&home, &plain, at(f64::from(run) * 120.0)).is_empty());
    }
    assert!(plain.log().is_empty());
}

#[test]
fn the_hub_probe_takes_any_http_answer_and_not_a_closed_port() {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let address = server.server_addr().to_ip().unwrap();
    let answering = std::thread::spawn(move || {
        let request = server.recv().unwrap();
        let _ = request.respond(tiny_http::Response::from_string("no").with_status_code(404));
    });
    assert!(hub_answers(&address.to_string()));
    answering.join().unwrap();
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    assert!(!hub_answers(&closed.to_string()));
}

/// A home whose store holds `GOOD` and `BAD`, with `current` current and `last_good` marked good.
fn home_running(current: &str, last_good: &str) -> Home {
    let home = home();
    let store = home.store();
    for hex in [GOOD, BAD] {
        let built = home.root.join(format!("built-{hex}"));
        fs::write(&built, hex).unwrap();
        store.store(&commit(hex), &built).unwrap();
    }
    store.mark_last_good(&commit(last_good)).unwrap();
    store.point_current_at(&commit(current)).unwrap();
    home
}

#[test]
fn a_service_an_older_build_wrote_is_rewritten_once_while_the_daemon_answers() {
    let home = home_running(GOOD, GOOD);
    let machine = FakeMachine::new(true, true);
    let done = run_once(&home, &machine, at(0.0));
    assert_eq!(
        done,
        ["an older build wrote the daemon's login service; rewrote and reloaded it"]
    );
    assert!(run_once(&home, &machine, at(120.0)).is_empty());
    assert_eq!(machine.log(), ["refresh daemon"]);
}

#[test]
fn the_service_is_not_rewritten_by_a_monitor_older_than_the_current_build() {
    // The monitor runs the last-good build, GOOD, while the daemon runs BAD, which is newer and
    // not marked good yet: GOOD would write the service as it did, undoing BAD's install.
    let home = home_running(BAD, GOOD);
    let machine = FakeMachine::new(true, true);
    assert!(run_once(&home, &machine, at(0.0)).is_empty());
    assert!(machine.log().is_empty());
}

#[test]
fn the_service_is_not_rewritten_while_the_daemon_does_not_answer() {
    let home = home_running(GOOD, GOOD);
    let machine = FakeMachine::new(false, false);
    run_once(&home, &machine, at(0.0));
    assert_eq!(machine.log(), ["kickstart"]);
}

#[test]
fn a_run_that_blocks_gives_up_at_its_deadline() {
    let (_keep, blocked) = mpsc::channel::<()>();
    assert_eq!(
        with_deadline(Duration::ZERO, move || blocked.recv().is_ok()),
        None
    );
    assert_eq!(with_deadline(RUN_DEADLINE, || 7), Some(7));
}

#[test]
fn a_kickstart_ends_every_daemon_that_did_not_stop_newest_first() {
    let start = |seconds: f64, pid: u32, stopped: bool| StartRecord {
        at: at(seconds),
        commit: GOOD.to_owned(),
        pid,
        stopped: stopped.then(|| at(seconds + 1.0)),
        hung: !stopped,
        reason: None,
        adopted: Vec::new(),
    };
    // 24900 hung, then each kickstarted daemon hung too; 20580 stopped for a release.
    let starts = [
        start(0.0, 20580, true),
        start(10.0, 24900, false),
        start(20.0, 31892, false),
        start(30.0, 1828, false),
        start(40.0, 31892, false),
    ];
    assert_eq!(daemons_to_end(&starts), [31892, 1828, 24900]);
}
