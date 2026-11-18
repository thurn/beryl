use super::*;
use ignite_core::fakes::FakeClock;
use ignite_core::time::Timestamp;

fn clock() -> Arc<dyn Clock> {
    Arc::new(FakeClock::new(Timestamp::EPOCH))
}

fn session(name: &str) -> SessionId {
    SessionId::new(name).unwrap()
}

fn temp_db(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ignite-handles-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("local.db")
}

fn open(path: &std::path::Path) -> Handles {
    let conn = ignite_store::local::open(path).unwrap();
    Handles::open(clock(), Arc::new(Mutex::new(conn))).unwrap()
}

#[test]
fn a_handle_names_its_verb_and_is_never_reused() {
    let handles = Handles::new(clock());
    let first = handles.create("land", None, None);
    let second = handles.create("land", None, None);
    assert_eq!((first.as_str(), second.as_str()), ("L-1", "L-2"));
    assert_eq!(handles.create("test", None, None).as_str(), "T-3");
}

#[test]
fn a_wait_returns_the_reply_once_the_operation_finishes() {
    let handles = Handles::new(clock());
    let handle = handles.create("land", None, None);
    assert_eq!(
        handles.wait(&handle, 0, None),
        Waited::Running("land".to_owned())
    );
    let reply = Reply::text(Exit::Success, "landed");
    handles.finish(&handle, reply.clone());
    assert_eq!(handles.wait(&handle, 0, None), Waited::Done(reply));
    let unknown = Handle::new("L-99").unwrap();
    assert_eq!(handles.wait(&unknown, 0, None), Waited::Unknown);
}

#[test]
fn a_restarted_daemon_still_answers_for_a_handle_from_before() {
    let path = temp_db("restart");
    let reply = Reply::text(Exit::CheckFailed, "failed   rust:a");
    let (finished, unfinished) = {
        let handles = open(&path);
        let finished = handles.create("land", Some(&session("local_a")), None);
        handles.finish(&finished, reply.clone());
        (finished, handles.create("test", None, None))
    };
    let handles = open(&path);
    assert_eq!(handles.wait(&finished, 0, None), Waited::Done(reply));
    let Waited::Done(lost) = handles.wait(&unfinished, 0, None) else {
        panic!("an operation the daemon stopped in is answered as lost");
    };
    assert_eq!(lost.exit, Exit::Conflict);
    assert!(lost.text.starts_with("lost "), "{}", lost.text);
    // A name used before the restart is skipped: `L-1` is taken, `L-2` was never used.
    assert_eq!(handles.create("land", None, None).as_str(), "L-2");
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn an_interrupt_ends_only_that_sessions_waits_with_its_reply() {
    let handles = Arc::new(Handles::new(clock()));
    let handle = handles.create("land", None, None);
    let waiter = {
        let (handles, handle) = (handles.clone(), handle.clone());
        std::thread::spawn(move || handles.wait(&handle, 3600, Some(&session("local_a"))))
    };
    while handles.waits_of(&session("local_a")) == 0 {
        std::thread::yield_now();
    }
    let message = Reply::text(Exit::Success, "message  Land when the gate passes.");
    assert_eq!(handles.interrupt(&session("local_b"), &message), 0);
    assert_eq!(handles.interrupt(&session("local_a"), &message), 1);
    assert_eq!(waiter.join().unwrap(), Waited::Interrupted(message));
    assert_eq!(handles.waits_of(&session("local_a")), 0);
}
