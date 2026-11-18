#[cfg(unix)]
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use super::*;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::TicketId;
use ignite_core::protocol::hub::Claim;

use super::wire::read_reply;
#[cfg(unix)]
use super::wire::write_line;
use crate::hubclient::tests::stopped_hub;
use ignite_os::ipc::Ipc;
#[cfg(unix)]
use ignite_os::ipc::{Listener, Stream};

struct Echo;

impl Verb for Echo {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn run(&self, _ctx: &Context, req: &Request) -> Reply {
        Reply::text(Exit::Success, req.args.join(" "))
    }
}

/// A verb that finishes only when the test sends it its reply.
struct Gate(Mutex<mpsc::Receiver<Reply>>);

impl Verb for Gate {
    fn name(&self) -> &'static str {
        "land"
    }

    fn run(&self, _ctx: &Context, _req: &Request) -> Reply {
        self.0.lock().unwrap().recv().unwrap()
    }
}

/// A verb that claims a ticket from the context's hub.
struct Start;

impl Verb for Start {
    fn name(&self) -> &'static str {
        "start"
    }

    fn run(&self, ctx: &Context, _req: &Request) -> Reply {
        let claim = Claim {
            ticket: TicketId::new("a.1").unwrap(),
            machine: ctx.machine.clone(),
            session: ctx.session.clone().unwrap(),
            tree: ctx.tree.clone().unwrap(),
            code: None,
        };
        match ctx.hub.claim(&claim) {
            Ok(_) => Reply::text(Exit::Success, "claimed"),
            Err(error) => error_reply(&error),
        }
    }
}

/// A hook that counts its calls and adds one line.
struct Changed(Arc<AtomicUsize>);

impl Hook for Changed {
    fn run(&self, _ctx: &Context, _req: &Request, tree: &rows::Tree) -> Vec<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        vec![format!(
            "changed  comment on {}",
            tree.ticket.as_deref().unwrap_or("-")
        )]
    }
}

fn context() -> Context {
    let text = r#"
        [repository]
        remote = "origin"
        staging = "staging"
        master = "master"
        trees = "trees"
        checks = "checks"
        [hub]
        url = "http://localhost:1"
        project = "test"
        [gate]
        threshold = 0.01
        cap = 10
        [machine.local]
        ticket_trees = 1
        landing_trees = 1
    "#;
    Context {
        config: Config::parse("ignite.toml", text).unwrap(),
        repo: PathBuf::from("/repo"),
        machine: MachineId::new("m-1").unwrap(),
        clock: Arc::new(FakeClock::new(Timestamp::EPOCH)),
        hub: Arc::new(FakeHub::default()),
        git: Arc::new(FakeGit::default()),
        launcher: Arc::new(FakeLauncher::default()),
        builds: Arc::new(FakeBuilds::default()),
        session: None,
        tree: None,
    }
}

fn request(verb: &str, args: &[&str]) -> Request {
    Request {
        verb: verb.to_owned(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: PathBuf::from("/repo"),
        session: None,
        wait_s: 1,
        pid: None,
    }
}

fn from(mut req: Request, cwd: &str, session: &str) -> Request {
    req.cwd = PathBuf::from(cwd);
    req.session = Some(SessionId::new(session).unwrap());
    req
}

fn gated() -> (Registry, mpsc::Sender<Reply>) {
    let (send, receive) = mpsc::channel();
    let mut registry = Registry::default();
    registry.register(Box::new(Gate(Mutex::new(receive))));
    (registry, send)
}

fn tree(id: &str, state: &str) -> rows::Tree {
    rows::Tree {
        id: id.to_owned(),
        path: format!("/repo/trees/{id}"),
        kind: "ticket".to_owned(),
        ticket: Some("a.1".to_owned()),
        session: Some("local_a".to_owned()),
        branch: None,
        built_commit: None,
        leased_at: None,
        last_used: None,
        owner: None,
        state: state.to_owned(),
    }
}

#[test]
fn dispatches_to_the_named_verb() {
    let mut registry = Registry::default();
    registry.register(Box::new(Echo));
    let reply = registry.handle(&context(), &request("echo", &["a", "b"]));
    assert_eq!((reply.exit, reply.text.as_str()), (Exit::Success, "a b"));
    assert_eq!(registry.names(), ["echo"]);
}

#[test]
fn an_unknown_verb_is_a_usage_error() {
    let server = Server::new(context(), Registry::default(), None).unwrap();
    let reply = server.handle(&request("nope", &[]));
    assert_eq!(reply.exit, Exit::Usage);
    assert_eq!(reply.text, "unknown verb: nope");
}

#[test]
#[should_panic(expected = "registered twice")]
fn a_verb_cannot_be_registered_twice() {
    let mut registry = Registry::default();
    registry.register(Box::new(Echo));
    registry.register(Box::new(Echo));
}

#[test]
fn the_tree_is_the_folder_under_the_trees_folder() {
    let ctx = context();
    assert_eq!(
        ctx.tree_of(Path::new("/repo/trees/tree-07/rust/src"))
            .unwrap()
            .as_str(),
        "tree-07"
    );
    assert_eq!(
        ctx.tree_of(Path::new("/repo/trees/tree-07"))
            .unwrap()
            .as_str(),
        "tree-07"
    );
    assert_eq!(ctx.tree_of(Path::new("/repo/rust")), None);
    assert_eq!(ctx.tree_of(Path::new("/repo/trees")), None);
}

#[test]
fn a_reply_reaches_the_client_over_the_socket() {
    let dir = std::env::temp_dir().join(format!("ignite-server-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let socket = dir.join("d.sock").to_string_lossy().into_owned();
    let mut registry = Registry::default();
    registry.register(Box::new(Echo));
    let server = Arc::new(Server::new(context(), registry, None).unwrap());
    let listener = ignite_os::Native.listen(&socket).unwrap();
    std::thread::spawn(move || serve_listener(&server, listener.as_ref()));

    let reply = send(&socket, &request("echo", &["hello", "there"])).unwrap();
    assert_eq!(
        (reply.exit, reply.text.as_str()),
        (Exit::Success, "hello there")
    );
    let reply = send(&socket, &request("nope", &[])).unwrap();
    assert_eq!(reply.exit, Exit::Usage);
}

/// The daemon serves its socket before it has finished starting. A request sent meanwhile is
/// answered once the daemon is ready, never refused; `status` is answered at once.
#[test]
fn a_request_sent_while_the_daemon_starts_is_answered_once_it_is_ready() {
    let dir = std::env::temp_dir().join(format!("ignite-server-starting-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let socket = dir.join("d.sock").to_string_lossy().into_owned();
    let listener = ignite_os::Native.listen(&socket).unwrap();
    let starting = Starting::new();
    {
        let starting = starting.clone();
        std::thread::spawn(move || serve_starting(&starting, listener.as_ref()));
    }

    let client = {
        let socket = socket.clone();
        std::thread::spawn(move || send(&socket, &request("echo", &["hello", "there"])))
    };
    starting.until_waiting(1);
    let status = send(&socket, &request("status", &[])).unwrap();
    assert_eq!(status.exit, Exit::Success);
    assert!(status.text.starts_with("starting "), "{}", status.text);
    assert_eq!(status.json.unwrap()["starting"], true);
    assert!(
        !client.is_finished(),
        "answered before the daemon was ready"
    );

    let mut registry = Registry::default();
    registry.register(Box::new(Echo));
    starting.ready(Arc::new(Server::new(context(), registry, None).unwrap()));

    let reply = client.join().unwrap().unwrap();
    assert_eq!(
        (reply.exit, reply.text.as_str()),
        (Exit::Success, "hello there")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_request_sent_while_the_daemon_fails_to_start_is_told_why() {
    let dir =
        std::env::temp_dir().join(format!("ignite-server-start-fails-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let socket = dir.join("d.sock").to_string_lossy().into_owned();
    let listener = ignite_os::Native.listen(&socket).unwrap();
    let starting = Starting::new();
    {
        let starting = starting.clone();
        std::thread::spawn(move || serve_starting(&starting, listener.as_ref()));
    }
    let client = {
        let socket = socket.clone();
        std::thread::spawn(move || send(&socket, &request("echo", &["hello"])))
    };
    starting.until_waiting(1);

    starting.failed("ignite.toml declares no [machine.alpha]");

    let reply = client.join().unwrap().unwrap();
    assert_eq!(reply.exit, Exit::Usage);
    assert!(
        reply
            .text
            .contains("did not start: ignite.toml declares no [machine.alpha]"),
        "{}",
        reply.text
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_long_verb_returns_a_handle_and_wait_gets_its_result() {
    let (registry, finish) = gated();
    let server = Server::new(context(), registry, None).unwrap();
    let mut req = request("land", &["-m", "note"]);
    req.wait_s = 0;
    let first = server.handle(&req);
    assert_eq!(first.exit, Exit::Running);
    let handle = first.handle.clone().unwrap();
    assert_eq!(
        first.text,
        format!("waiting  `ignite land` is still running\nnext     ignite wait {handle}")
    );

    let mut wait = request("wait", &[handle.as_str()]);
    wait.wait_s = 0;
    assert_eq!(server.handle(&wait).exit, Exit::Running);

    let landed = Reply::text(Exit::Success, "landed   abc on staging");
    finish.send(landed.clone()).unwrap();
    wait.wait_s = 3600;
    assert_eq!(server.handle(&wait), landed);
    // The reply stays with the handle for a second wait.
    assert_eq!(server.handle(&wait), landed);
    assert_eq!(
        server.handle(&request("wait", &["L-404"])).exit,
        Exit::Usage
    );
}

#[test]
fn a_verb_that_finishes_within_the_wait_answers_directly() {
    let (registry, finish) = gated();
    let server = Server::new(context(), registry, None).unwrap();
    let reply = Reply::text(Exit::CheckFailed, "failed   rust:a");
    finish.send(reply.clone()).unwrap();
    let mut req = request("land", &[]);
    req.wait_s = 3600;
    assert_eq!(server.handle(&req), reply);
}

#[test]
fn a_claim_while_the_hub_is_stopped_exits_with_code_four() {
    let mut ctx = context();
    ctx.hub = Arc::new(stopped_hub());
    let mut registry = Registry::default();
    registry.register(Box::new(Start));
    let server = Server::new(ctx, registry, None).unwrap();
    let reply = server.handle(&from(
        request("start", &[]),
        "/repo/trees/tree-01",
        "local_a",
    ));
    assert_eq!(reply.exit, Exit::Unreachable);
    assert!(
        reply.text.starts_with("offline  unreachable: "),
        "{}",
        reply.text
    );
}

#[test]
fn a_session_inside_wait_shows_it_and_an_interrupt_ends_it() {
    let (registry, _finish) = gated();
    let server = Arc::new(Server::new(context(), registry, None).unwrap());
    let mut req = from(request("land", &[]), "/repo", "local_a");
    req.wait_s = 0;
    let handle = server.handle(&req).handle.unwrap();
    let ran = server
        .current_command(&SessionId::new("local_a").unwrap())
        .unwrap();
    assert_eq!(
        (ran.verb.as_str(), ran.ended_at),
        ("land", Some(Timestamp::EPOCH))
    );

    let mut wait = from(request("wait", &[handle.as_str()]), "/repo", "local_a");
    wait.wait_s = 3600;
    let waiting = {
        let server = server.clone();
        std::thread::spawn(move || server.handle(&wait))
    };
    let session = SessionId::new("local_a").unwrap();
    while server.handles().waits_of(&session) == 0 {
        std::thread::yield_now();
    }
    let now = server.current_command(&session).unwrap();
    assert_eq!(
        now,
        CurrentCommand {
            verb: "wait".to_owned(),
            arg: Some(handle.to_string()),
            started_at: Timestamp::EPOCH,
            ended_at: None,
        }
    );

    let message = Reply::text(Exit::Success, "message  Land when the gate passes.");
    assert_eq!(server.handles().interrupt(&session, &message), 1);
    assert_eq!(waiting.join().unwrap(), message);
    assert!(server.current_command(&session).unwrap().ended_at.is_some());
}

#[test]
fn hooks_run_once_per_request_from_a_leased_tree() {
    let conn = ignite_store::local::open_in_memory().unwrap();
    trees::upsert(&conn, &tree("tree-01", "leased")).unwrap();
    trees::upsert(&conn, &tree("tree-02", "free")).unwrap();
    let db = Arc::new(Mutex::new(conn));
    let (mut registry, finish) = gated();
    registry.register(Box::new(Echo));
    let mut server = Server::new(context(), registry, Some(db)).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    server.add_hook(Box::new(Changed(calls.clone())));

    let reply = server.handle(&from(
        request("echo", &["ran"]),
        "/repo/trees/tree-01/src",
        "local_a",
    ));
    assert_eq!(reply.text, "changed  comment on a.1\nran");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // A verb handed to a handle runs the hooks once, on the reply that names the handle.
    let mut long = from(request("land", &[]), "/repo/trees/tree-01", "local_a");
    long.wait_s = 0;
    let reply = server.handle(&long);
    assert!(
        reply.text.starts_with("changed  comment on a.1\nwaiting  "),
        "{}",
        reply.text
    );
    finish.send(Reply::text(Exit::Success, "landed")).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    server.handle(&from(
        request("echo", &[]),
        "/repo/trees/tree-02",
        "local_a",
    ));
    server.handle(&from(request("echo", &[]), "/repo", "local_a"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

/// A verb that counts its runs; `exempt` makes it one that needs no tree lease.
struct Counted {
    name: &'static str,
    exempt: bool,
    runs: Arc<AtomicUsize>,
}

impl Verb for Counted {
    fn name(&self) -> &'static str {
        self.name
    }

    fn run(&self, _ctx: &Context, _req: &Request) -> Reply {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Reply::text(Exit::Success, "ran")
    }

    fn needs_the_tree_lease(&self) -> bool {
        !self.exempt
    }
}

#[test]
fn only_the_session_holding_a_tree_may_use_it() {
    let conn = ignite_store::local::open_in_memory().unwrap();
    trees::upsert(&conn, &tree("tree-01", "leased")).unwrap();
    trees::upsert(&conn, &tree("tree-02", "free")).unwrap();
    let db = Arc::new(Mutex::new(conn));
    let runs = Arc::new(AtomicUsize::new(0));
    let mut registry = Registry::default();
    for (name, exempt) in [("check", false), ("status", true)] {
        registry.register(Box::new(Counted {
            name,
            exempt,
            runs: runs.clone(),
        }));
    }
    let mut server = Server::new(context(), registry, Some(db)).unwrap();
    let hooks = Arc::new(AtomicUsize::new(0));
    server.add_hook(Box::new(Changed(hooks.clone())));

    let holder = server.handle(&from(
        request("check", &[]),
        "/repo/trees/tree-01/src",
        "local_a",
    ));
    assert_eq!(
        (holder.exit, runs.load(Ordering::SeqCst)),
        (Exit::Success, 1)
    );

    let other = server.handle(&from(
        request("check", &[]),
        "/repo/trees/tree-01",
        "local_b",
    ));
    assert_eq!(other.exit, Exit::Conflict);
    assert_eq!(
        other.text,
        "conflict tree-01 is not leased to local_b (it is leased to local_a); \
         only the session that holds a tree may use it"
    );
    let mut anonymous = request("check", &[]);
    anonymous.cwd = PathBuf::from("/repo/trees/tree-01");
    assert_eq!(server.handle(&anonymous).exit, Exit::Conflict);
    let free = server.handle(&from(
        request("check", &[]),
        "/repo/trees/tree-02",
        "local_a",
    ));
    assert_eq!(free.exit, Exit::Conflict);
    assert!(
        free.text
            .starts_with("conflict tree-02 is not leased to local_a (it is free); "),
        "{}",
        free.text
    );
    // Refused before the verb, the hooks (which renew the lease) or the record of the command.
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert_eq!(hooks.load(Ordering::SeqCst), 1);
    assert!(
        server
            .current_command(&SessionId::new("local_b").unwrap())
            .is_none()
    );

    // An exempt verb runs for anyone, and an unknown verb from a held tree is still refused.
    let status = server.handle(&from(
        request("status", &[]),
        "/repo/trees/tree-01",
        "local_b",
    ));
    assert_eq!(
        (status.exit, runs.load(Ordering::SeqCst)),
        (Exit::Success, 2)
    );
    let unknown = server.handle(&from(
        request("nope", &[]),
        "/repo/trees/tree-01",
        "local_b",
    ));
    assert_eq!(unknown.exit, Exit::Conflict);
    // Outside any tree nothing is checked.
    let outside = server.handle(&from(request("check", &[]), "/repo", "local_b"));
    assert_eq!(outside.exit, Exit::Success);
}

#[test]
fn start_wait_status_and_doctor_need_no_tree_lease() {
    use crate::doctor::DoctorVerb;
    let server = Server::new(context(), Registry::default(), None).unwrap();
    assert!(!server.registry.get("wait").unwrap().needs_the_tree_lease());
    assert!(!DoctorVerb::default().needs_the_tree_lease());
}

#[test]
fn stop_and_wait_are_answered_from_a_tree_being_reset() {
    let conn = ignite_store::local::open_in_memory().unwrap();
    trees::upsert(&conn, &tree("tree-01", "resetting")).unwrap();
    let db = Arc::new(Mutex::new(conn));
    let server = Server::new(context(), Registry::default(), Some(db)).unwrap();
    for verb in ["stop", "wait"] {
        let reply = server.handle(&from(
            request(verb, &["T-9"]),
            "/repo/trees/tree-01",
            "local_a",
        ));
        // The verb itself answers; the lease check refuses nothing.
        assert_eq!(reply.exit, Exit::Usage, "{verb}: {}", reply.text);
        assert_eq!(reply.text, "unknown handle: T-9", "{verb}");
    }
}

/// A verb that finishes only when the test sends it a unit, under any name, and that says
/// whether it blocks an update.
struct Held {
    name: &'static str,
    blocks: bool,
    go: Mutex<mpsc::Receiver<()>>,
}

impl Verb for Held {
    fn name(&self) -> &str {
        self.name
    }

    fn blocks_update(&self) -> bool {
        self.blocks
    }

    fn run(&self, _ctx: &Context, _req: &Request) -> Reply {
        self.go.lock().unwrap().recv().unwrap();
        // What the verb can learn about its caller while it runs.
        let gone = caller_probe().map(|probe| probe());
        Reply::text(Exit::Success, format!("{gone:?}"))
    }
}

fn held(name: &'static str, blocks: bool) -> (Box<Held>, mpsc::Sender<()>) {
    let (send, receive) = mpsc::channel();
    let verb = Held {
        name,
        blocks,
        go: Mutex::new(receive),
    };
    (Box::new(verb), send)
}

#[test]
fn a_verb_that_only_waits_does_not_hold_back_an_update() {
    let (land, finish_land) = held("land", true);
    let (dispatch, finish_dispatch) = held("dispatch", false);
    let mut registry = Registry::default();
    registry.register(land);
    registry.register(dispatch);
    let server = Server::new(context(), registry, None).unwrap();
    for verb in ["dispatch", "land"] {
        let mut req = request(verb, &[]);
        req.wait_s = 0;
        assert_eq!(server.handle(&req).exit, Exit::Running);
    }
    // Both run, and only the one that does work is waited for.
    assert_eq!(server.running_verbs(), 1);
    finish_dispatch.send(()).unwrap();
    finish_land.send(()).unwrap();
    while server.running_verbs() > 0 {
        std::thread::yield_now();
    }
}

#[test]
fn a_verb_can_tell_that_its_caller_left_until_it_was_given_a_handle() {
    let (verb, finish) = held("run", true);
    let mut registry = Registry::default();
    registry.register(verb);
    let server = Server::new(context(), registry, None).unwrap();
    let gone: CallerProbe = Arc::new(|| true);

    // Still waited for: the probe says the client has gone.
    finish.send(()).unwrap();
    let mut req = request("run", &[]);
    req.wait_s = 3600;
    assert_eq!(
        server.handle_from(&req, Some(gone.clone())).text,
        "Some(true)"
    );

    // Handed to a handle: the client leaving is no longer news.
    req.wait_s = 0;
    let first = server.handle_from(&req, Some(gone));
    let handle = first.handle.unwrap();
    finish.send(()).unwrap();
    let mut wait = request("wait", &[handle.as_str()]);
    wait.wait_s = 3600;
    assert_eq!(server.handle(&wait).text, "Some(false)");

    // No probe where the connection cannot be probed.
    finish.send(()).unwrap();
    req.wait_s = 3600;
    assert_eq!(server.handle(&req).text, "None");
}

/// Accepts fail twice, then one connection arrives.
#[cfg(unix)]
struct Flaky {
    failures: Mutex<u32>,
    stream: Mutex<Option<Box<dyn Stream>>>,
}

#[cfg(unix)]
impl Listener for Flaky {
    fn accept(&self) -> std::io::Result<Box<dyn Stream>> {
        let mut failures = self.failures.lock().unwrap();
        if *failures > 0 {
            *failures -= 1;
            return Err(std::io::Error::other("too many open files"));
        }
        match self.stream.lock().unwrap().take() {
            Some(stream) => Ok(stream),
            None => {
                // Nothing else will connect; wait here as a real listener would.
                loop {
                    std::thread::park();
                }
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn a_failed_accept_does_not_end_the_server() {
    let (accepted, mut client) = std::os::unix::net::UnixStream::pair().unwrap();
    let listener = Flaky {
        failures: Mutex::new(2),
        stream: Mutex::new(Some(Box::new(accepted))),
    };
    let mut registry = Registry::default();
    registry.register(Box::new(Echo));
    let server = Arc::new(Server::new(context(), registry, None).unwrap());
    std::thread::spawn(move || serve_listener(&server, &listener));

    write_line(&mut client, &request("echo", &["still", "here"])).unwrap();
    let mut line = String::new();
    BufReader::new(&mut client).read_line(&mut line).unwrap();
    let reply: Reply = serde_json::from_str(&line).unwrap();
    assert_eq!(reply.text, "still here");
}

/// A verb that records two steps and then answers.
struct Steps2;

impl Verb for Steps2 {
    fn name(&self) -> &'static str {
        "land"
    }

    fn run(&self, _ctx: &Context, _req: &Request) -> Reply {
        crate::steps::step("rebasing onto staging");
        let _lease = crate::steps::waiting("the staging lease");
        crate::steps::step("pushing to staging");
        Reply::text(Exit::Success, "landed")
    }
}

#[test]
fn a_client_that_asks_for_progress_hears_each_step_before_the_reply() {
    let mut registry = Registry::default();
    registry.register(Box::new(Steps2));
    let server = Server::new(context(), registry, None).unwrap();
    let mut heard = Vec::new();
    let mut pass = |line: &str| {
        heard.push(line.to_owned());
        true
    };
    let reply = server.handle_streaming(&request("land", &[]), None, Some(&mut pass));
    assert_eq!(reply.text, "landed");
    assert_eq!(
        heard,
        [
            "rebasing onto staging",
            "waiting for the staging lease",
            "pushing to staging"
        ]
    );
}

#[test]
fn the_client_prints_progress_lines_keeps_the_handle_and_returns_the_reply_line() {
    let handle = Handle::new("L-3").unwrap();
    // A reply with a handle of its own is still the reply, not a handle line.
    let reply = still_running("land", &handle);
    let text = format!(
        "{}\n{}\n{}\n{}\n",
        serde_json::json!({"handle": "L-3"}),
        serde_json::json!({"progress": "rebasing onto staging"}),
        serde_json::json!({"progress": "running the gate in \"landing-3\""}),
        serde_json::to_string(&reply).unwrap()
    );
    let mut heard = Vec::new();
    let mut show = |line: &str| heard.push(line.to_owned());
    let mut named = None;
    let read = read_reply(std::io::Cursor::new(text), Some(&mut show), &mut named).unwrap();
    assert_eq!(read, reply);
    assert_eq!(named, Some(handle));
    assert_eq!(
        heard,
        ["rebasing onto staging", "running the gate in \"landing-3\""]
    );
    // A connection that ends before the reply keeps the handle it named.
    let mut named = None;
    let cut = read_reply(
        std::io::Cursor::new(format!("{}\n", serde_json::json!({"handle": "T-9"}))),
        None,
        &mut named,
    );
    assert_eq!(cut.unwrap_err().kind(), std::io::ErrorKind::UnexpectedEof);
    assert_eq!(named.unwrap().as_str(), "T-9");
}

/// A `land` that holds its tree and finishes only when the test sends it a unit.
struct HeldLanding(Mutex<mpsc::Receiver<()>>);

impl Verb for HeldLanding {
    fn name(&self) -> &'static str {
        "land"
    }

    fn holds_the_tree(&self) -> bool {
        true
    }

    fn run(&self, _ctx: &Context, _req: &Request) -> Reply {
        self.0.lock().unwrap().recv().unwrap();
        Reply::text(Exit::Success, "landed")
    }
}

/// A second `ignite land` of one ticket started in its tree while the landing the daemon had
/// resumed after a restart was still running there, and both rebased and built the tree. A
/// verb that acts on the tree is now refused while a landing runs in it, and the refusal names
/// the running landing.
#[test]
fn nothing_changes_a_tree_while_a_landing_runs_in_it() {
    let conn = ignite_store::local::open_in_memory().unwrap();
    trees::upsert(&conn, &tree("tree-01", "leased")).unwrap();
    trees::upsert(&conn, &tree("tree-02", "leased")).unwrap();
    let db = Arc::new(Mutex::new(conn));
    let (finish, receive) = mpsc::channel();
    let runs = Arc::new(AtomicUsize::new(0));
    let mut registry = Registry::default();
    registry.register(Box::new(HeldLanding(Mutex::new(receive))));
    registry.register(Box::new(Counted {
        name: "check",
        exempt: false,
        runs: runs.clone(),
    }));
    let server = Server::new(context(), registry, Some(db)).unwrap();
    let in_tree = |verb: &str, tree: &str| {
        let mut req = from(
            request(verb, &[]),
            &format!("/repo/trees/{tree}"),
            "local_a",
        );
        // The landing hands its handle back at once; the others answer within the wait.
        req.wait_s = if verb == "land" { 0 } else { 60 };
        server.handle(&req)
    };

    let first = in_tree("land", "tree-01");
    assert_eq!(first.exit, Exit::Running);
    let handle = first.handle.clone().unwrap();

    let second = in_tree("land", "tree-01");
    assert_eq!(second.exit, Exit::Conflict);
    assert_eq!(
        second.text,
        format!(
            "conflict land {handle} is still running in tree-01; `ignite land` waits until it \
             ends\nnext     ignite wait {handle} (or ignite stop {handle} to end it)"
        )
    );
    assert_eq!(in_tree("check", "tree-01").exit, Exit::Conflict);
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    // Another tree, and waiting on the landing itself, are not held back.
    assert_eq!(in_tree("check", "tree-02").exit, Exit::Success);
    let mut wait = from(
        request("wait", &[handle.as_str()]),
        "/repo/trees/tree-01",
        "local_a",
    );
    wait.wait_s = 0;
    assert_eq!(server.handle(&wait).exit, Exit::Running);

    finish.send(()).unwrap();
    wait.wait_s = 3600;
    assert_eq!(server.handle(&wait).exit, Exit::Success);
    assert_eq!(in_tree("check", "tree-01").exit, Exit::Success);
}
