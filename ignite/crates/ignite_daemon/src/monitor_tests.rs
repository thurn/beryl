use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use ignite_core::ids::Commit;

use super::*;

const GOOD: &str = "aaaaaaa";
const BAD: &str = "bbbbbbb";

fn at(seconds: f64) -> Timestamp {
    Timestamp::from_millis(1_800_000_000_000).plus_seconds(seconds)
}

fn commit(hex: &str) -> Commit {
    Commit::new(hex).unwrap()
}

/// A daemon whose process state and answers a test sets, recording what the monitor does.
struct FakeMachine {
    running: bool,
    answers: bool,
    /// On the hub's machine, whether its hub answers; `None` elsewhere.
    hub: Option<bool>,
    log: RefCell<Vec<String>>,
}

impl FakeMachine {
    fn new(running: bool, answers: bool) -> Self {
        Self {
            running,
            answers,
            hub: None,
            log: RefCell::default(),
        }
    }

    fn with_hub(mut self, answers: bool) -> Self {
        self.hub = Some(answers);
        self
    }

    fn log(&self) -> Vec<String> {
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
}

fn home() -> Home {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let root = std::env::temp_dir().join(format!(
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
    }
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
            running: true,
            answered,
            crash_loop: Some(looped.clone()),
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
    let machine = FakeMachine::new(true, false);
    for run in 0..3 {
        run_once(&home, &machine, at(f64::from(run) * 120.0));
    }
    assert_eq!(machine.log(), ["kickstart"]);
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

#[cfg(unix)]
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
