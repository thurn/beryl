use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use super::*;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::TicketId;
use ignite_core::protocol::hub::Claim;

use crate::hubclient::tests::stopped_hub;

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
