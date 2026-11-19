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
        (1, 1),
        "the landing ran again in the new daemon"
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
    assert_eq!(
        (row.state.as_str(), row.executable.as_deref()),
        ("done", None)
    );
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
