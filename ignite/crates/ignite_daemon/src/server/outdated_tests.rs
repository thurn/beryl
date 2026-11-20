//! A daemon restarted into a new build runs again, under the same handle, a landing whose verb
//! process still runs the old build and has not begun its push (see `server/outdated.rs`),
//! against the verb processes of `process_tests.rs`, which run build `87631b5`.

use super::*;

/// The build the daemon restarts into.
const NEW_BUILD: &str = "eda21bd0000000000000000000000000000000000";

/// `ignite wait <handle>` with no time to wait, as a session whose wait ends first.
fn wait_now(server: &Server, handle: &Handle) -> Reply {
    let mut wait = request("wait", 0);
    wait.args = vec![handle.to_string()];
    server.handle(&wait)
}

/// The first daemon, running a landing and a test in verb processes of build `87631b5`, each
/// in its command.
fn landing_and_test(machine: &Machine) -> (Arc<Server>, Handle, Handle) {
    let first = machine.daemon_at(Some("87631b5"));
    machine.switch.set(Some(first.clone()));
    let land = handed_off(&first.handle(&request("land", 0)));
    let test = handed_off(&first.handle(&request("test", 0)));
    assert!(
        until(|| machine.commands.begun() == 2),
        "both commands began"
    );
    (first, land, test)
}

/// The daemon restarts into [`NEW_BUILD`] while both verb processes run.
fn restart(machine: &Machine, first: Arc<Server>) -> Arc<Server> {
    machine.switch.set(None);
    drop(first);
    let second = machine.daemon_at(Some(NEW_BUILD));
    machine.switch.set(Some(second.clone()));
    second
}

/// The acceptance test: a landing waiting under build A is stopped and run again under build B
/// with the same handle, which its waiter keeps; a test started under build A finishes there.
#[test]
fn a_landing_waiting_under_an_old_build_runs_again_under_the_new_one() {
    let machine = Machine::new("outdated-land", false);
    let (first, land, test) = landing_and_test(&machine);
    let second = restart(&machine, first);

    // A wait that ends first says what happened, as a fact.
    let early = wait_now(&second, &land);
    assert_eq!(early.exit, Exit::Running, "{}", early.text);
    assert!(
        early.text.starts_with(&format!(
            "restart  {land} was restarted under ignite eda21bd: it had not begun its push under \
             ignite 87631b5\n"
        )),
        "{}",
        early.text
    );
    assert!(
        !wait_now(&second, &test).text.contains("restart"),
        "the test is not restarted"
    );

    // The old landing hears the stop in its process before its command ends. Its verb process
    // asks again only after acting on an answer, so a second answer that says to stop means it
    // has heard the first.
    assert!(until(|| machine.switch.stops.load(Ordering::SeqCst) >= 2));
    machine.commands.finish_all();

    assert_eq!(waited(&second, &land).text, "land: Some(0)");
    assert_eq!(waited(&second, &test).text, "test: Some(0)");
    assert_eq!(
        (
            machine.ran_in_process("land"),
            machine.ran_in_daemon("land")
        ),
        (2, 0),
        "the landing ran again in a new verb process, which outlives the next restart"
    );
    assert_eq!(
        (
            machine.ran_in_process("test"),
            machine.ran_in_daemon("test")
        ),
        (1, 0)
    );
    let row = handle_store::get(&lock(&machine.db), land.as_str())
        .unwrap()
        .unwrap();
    // The new verb process recorded its own executable; the fake processes all report the
    // old build's commit, so only the state is compared.
    assert_eq!(row.state.as_str(), "done");
}

/// A landing whose push has begun under build A refuses the stop and finishes there.
#[test]
fn a_landing_in_its_push_is_left_to_finish_under_its_build() {
    let machine = Machine::new("outdated-push", false);
    let (first, land, _test) = landing_and_test(&machine);
    // What the landing's verb process asks of the daemon as its push begins.
    let refused = machine
        .switch
        .refuse_stop(&RefuseStop {
            handle: land.clone(),
            reason: "its push to staging has begun".to_owned(),
        })
        .unwrap();
    assert!(refused.refused);
    let second = restart(&machine, first);

    assert!(!wait_now(&second, &land).text.contains("restart"));
    machine.commands.finish_all();
    assert_eq!(waited(&second, &land).text, "land: Some(0)");
    assert_eq!(
        (
            machine.ran_in_process("land"),
            machine.ran_in_daemon("land")
        ),
        (1, 0)
    );
    let row = handle_store::get(&lock(&machine.db), land.as_str())
        .unwrap()
        .unwrap();
    assert_eq!(row.executable.as_deref(), Some("87631b5"));
}

/// A restart that keeps the build, such as a drill, leaves the landing running.
#[test]
fn a_restart_into_the_same_build_restarts_no_landing() {
    let machine = Machine::new("outdated-same", false);
    let (first, land, _test) = landing_and_test(&machine);
    machine.switch.set(None);
    drop(first);
    let second = machine.daemon_at(Some("87631b5"));
    machine.switch.set(Some(second.clone()));
    machine.commands.finish_all();
    assert_eq!(waited(&second, &land).text, "land: Some(0)");
    assert_eq!(machine.ran_in_daemon("land"), 0);
}

/// A release that changed none of the landing code leaves the landing running under its build,
/// so the daemon need not wait for landings before it restarts (see `main/selfupdate.rs`).
#[test]
fn a_landing_goes_on_under_its_build_when_the_release_changed_no_landing_code() {
    let machine = Machine::new("outdated-unchanged", false);
    let git = Arc::new(FakeGit::default());
    git.set_changed(
        &Commit::new("87631b5").unwrap(),
        &Commit::new(NEW_BUILD).unwrap(),
        &["ignite/crates/ignite_daemon/src/main/loops.rs"],
    );
    *lock(&machine.git) = Some(git.clone());
    let (first, land, _test) = landing_and_test(&machine);
    let second = restart(&machine, first);

    assert!(!wait_now(&second, &land).text.contains("restart"));
    machine.commands.finish_all();
    assert_eq!(waited(&second, &land).text, "land: Some(0)");
    assert_eq!(
        (
            machine.ran_in_process("land"),
            machine.ran_in_daemon("land")
        ),
        (1, 0),
        "the landing finished in its verb process"
    );
}

/// A release that changed the landing code still runs the landing again under it.
#[test]
fn a_landing_runs_again_when_the_release_changed_landing_code() {
    let machine = Machine::new("outdated-changed", false);
    let git = Arc::new(FakeGit::default());
    git.set_changed(
        &Commit::new("87631b5").unwrap(),
        &Commit::new(NEW_BUILD).unwrap(),
        &["ignite/crates/ignite_daemon/src/land/lease.rs"],
    );
    *lock(&machine.git) = Some(git.clone());
    let (first, land, _test) = landing_and_test(&machine);
    let second = restart(&machine, first);

    assert!(wait_now(&second, &land).text.starts_with("restart  "));
    assert!(until(|| machine.switch.stops.load(Ordering::SeqCst) >= 2));
    machine.commands.finish_all();
    assert_eq!(waited(&second, &land).text, "land: Some(0)");
    assert_eq!(machine.ran_in_process("land"), 2);
}

/// Ends a verb process as the native ender would, by letting its command finish so that it
/// writes its `stopped` reply and lets go of its lock, and counts the calls.
struct FinishesCommands(Arc<Commands>, AtomicUsize);

impl EndVerbProcess for FinishesCommands {
    fn end(&self, _handle: &Handle) -> std::result::Result<Option<String>, String> {
        self.1.fetch_add(1, Ordering::SeqCst);
        self.0.finish_all();
        Ok(Some(
            "its verb process 7 and its group (1 processes), with `SIGTERM`".to_owned(),
        ))
    }
}

/// A landing whose verb process agreed to stop but never looks at its stop where it is, as one
/// held in a loop on the CPU, is ended after the stop wait and run again under the new build,
/// without anyone finishing its command.
#[test]
fn a_landing_that_does_not_stop_when_asked_is_ended_and_runs_again() {
    let machine = Machine::new("outdated-unstopped", false);
    let ender = Arc::new(FinishesCommands(
        machine.commands.clone(),
        AtomicUsize::new(0),
    ));
    *lock(&machine.ender) = (ender.clone(), Duration::from_millis(100));
    let (first, land, _test) = landing_and_test(&machine);
    let second = restart(&machine, first);

    assert!(wait_now(&second, &land).text.starts_with("restart  "));
    assert_eq!(waited(&second, &land).text, "land: Some(0)");
    assert_eq!(
        ender.1.load(Ordering::SeqCst),
        1,
        "the verb process was ended once"
    );
    assert_eq!(
        (
            machine.ran_in_process("land"),
            machine.ran_in_daemon("land")
        ),
        (2, 0),
        "the landing ran again in a new verb process, which outlives the next restart"
    );
}
