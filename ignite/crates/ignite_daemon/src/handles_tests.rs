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

fn test_request() -> Request {
    Request {
        verb: "test".to_owned(),
        args: vec!["rust".to_owned()],
        cwd: std::path::PathBuf::from("/repo/trees/tree-01"),
        session: Some(session("local_a")),
        wait_s: 540,
        pid: Some(7),
    }
}

#[test]
fn a_restarted_daemon_still_answers_for_a_handle_from_before() {
    let path = temp_db("restart");
    let reply = Reply::text(Exit::CheckFailed, "failed   rust:a");
    let (finished, unfinished, unrecorded) = {
        let handles = open(&path);
        let finished = handles.create("land", Some(&session("local_a")), None);
        handles.finish(&finished, reply.clone());
        let request = test_request();
        let unfinished = handles.create_for("test", None, None, Some(&request));
        (finished, unfinished, handles.create("start", None, None))
    };
    let handles = open(&path);
    assert_eq!(handles.wait(&finished, 0, None), Waited::Done(reply));
    // The handles left running are handed over with their requests (oldest first, then by
    // name), and still read as running until the server finishes them.
    let orphans = handles.take_orphans();
    assert_eq!(
        orphans,
        [
            Orphan {
                handle: unrecorded.clone(),
                verb: "start".to_owned(),
                request: None,
            },
            Orphan {
                handle: unfinished.clone(),
                verb: "test".to_owned(),
                request: Some(test_request()),
            },
        ]
    );
    assert!(handles.take_orphans().is_empty());
    assert_eq!(
        handles.wait(&unfinished, 0, None),
        Waited::Running("test".to_owned())
    );
    handles.finish_incomplete(
        &unrecorded,
        incomplete("start", &unrecorded, "it was cut off"),
    );
    let Waited::Done(ended) = handles.wait(&unrecorded, 0, None) else {
        panic!("an operation that could not be run again is answered as incomplete");
    };
    assert_eq!(ended.exit, Exit::Conflict);
    assert!(ended.text.starts_with("incomplete "), "{}", ended.text);
    // The restarted daemon numbers above every stored handle (`L-1`, `T-2` and `S-3`).
    assert_eq!(handles.create("land", None, None).as_str(), "L-4");
    // Windows deletes no file that the database still holds open.
    drop(handles);
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

#[test]
fn a_reply_that_was_read_leaves_memory_but_the_database_still_answers() {
    let path = temp_db("prune-read");
    let handles = open(&path);
    let handle = handles.create("land", None, None);
    let reply = Reply::text(Exit::Success, "landed");
    handles.finish(&handle, reply.clone());
    assert_eq!(handles.wait(&handle, 0, None), Waited::Done(reply.clone()));
    // The next handle's creation drops the reply that was read.
    let next = handles.create("land", None, None);
    let in_memory: Vec<Handle> = lock(&handles.state).replies.keys().cloned().collect();
    assert_eq!(in_memory, [next]);
    assert_eq!(handles.wait(&handle, 0, None), Waited::Done(reply));
    // Windows deletes no file that the database still holds open.
    drop(handles);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_finished_reply_nobody_read_is_dropped_after_a_day_without_a_database() {
    let clock = Arc::new(FakeClock::new(Timestamp::EPOCH));
    let handles = Handles::new(clock.clone());
    let kept = handles.create("land", None, None);
    handles.finish(&kept, Reply::text(Exit::Success, "landed"));
    // A read does not drop it: with no database it would be gone for good.
    assert!(matches!(handles.wait(&kept, 0, None), Waited::Done(_)));
    handles.create("land", None, None);
    assert!(lock(&handles.state).replies.contains_key(&kept));
    clock.advance(KEEP_REPLY_S + 1.0);
    handles.create("land", None, None);
    assert!(!lock(&handles.state).replies.contains_key(&kept));
    assert_eq!(handles.wait(&kept, 0, None), Waited::Unknown);
}

/// A worker reported that `ignite wait` on a running landing answered "still running" at once.
/// The session's own transcript showed each call took the full 540 seconds; this keeps it so,
/// with the database the daemon uses, for a session waiting from the landing's tree.
#[test]
fn a_wait_on_a_running_operation_blocks_for_its_whole_wait() {
    let path = temp_db("blocks");
    let handles = open(&path);
    let tree = TreeId::new("tree-01").unwrap();
    let handle = handles.create("land", Some(&session("local_a")), Some(&tree));
    let started = Instant::now();
    assert_eq!(
        handles.wait(&handle, 1, Some(&session("local_a"))),
        Waited::Running("land".to_owned())
    );
    assert!(started.elapsed() >= Duration::from_secs(1));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// A landing the daemon was running when it stopped still holds its tree after the restart,
/// until it is resumed and finishes.
#[test]
fn a_handle_found_running_after_a_restart_still_names_its_tree() {
    let db = Arc::new(Mutex::new(ignite_store::local::open_in_memory().unwrap()));
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(Timestamp::EPOCH));
    let tree = TreeId::new("tree-05").unwrap();
    let before = Handles::open(clock.clone(), db.clone()).unwrap();
    let handle = before.create("land", None, Some(&tree));
    drop(before);

    let after = Handles::open(clock, db).unwrap();
    assert_eq!(
        after.running_in(&tree),
        [(handle.clone(), "land".to_owned())]
    );
    after.finish(&handle, Reply::text(Exit::Success, "landed"));
    assert!(after.running_in(&tree).is_empty());
}

#[test]
fn a_restarted_daemons_first_handle_is_numbered_above_every_stored_one() {
    let path = temp_db("numbers-grow");
    {
        let handles = open(&path);
        for _ in 0..5 {
            handles.create("land", None, None);
        }
        handles.create("test", None, None);
    }
    let handles = open(&path);
    assert_eq!(handles.create("land", None, None).as_str(), "L-7");
    assert_eq!(handles.create("test", None, None).as_str(), "T-8");
}

#[test]
fn a_sessions_operation_counts_as_running_until_it_finishes_even_across_a_restart() {
    let db = Arc::new(Mutex::new(ignite_store::local::open_in_memory().unwrap()));
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(Timestamp::EPOCH));
    let worker = session("local_w1");
    let before = Handles::open(clock.clone(), db.clone()).unwrap();
    let handle = before.create("land", Some(&worker), None);
    assert!(before.running_for(&worker, &["land"]));
    assert!(!before.running_for(&worker, &["test"]));
    assert!(!before.running_for(&session("local_other"), &["land"]));
    drop(before);

    let after = Handles::open(clock, db).unwrap();
    assert!(after.running_for(&worker, &["land"]));
    after.finish(&handle, Reply::text(Exit::Success, "landed"));
    assert!(!after.running_for(&worker, &["land"]));
}
