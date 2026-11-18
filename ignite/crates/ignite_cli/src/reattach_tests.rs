use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;

use super::*;

/// What one exchange with the fake daemon does.
enum Answer {
    Reply(Reply),
    /// The connection is refused before the request is written.
    Refused,
    /// The socket does not exist.
    Missing,
    /// The connection drops after the request was written, having named this handle.
    Dropped(Option<&'static str>),
}

/// A daemon that answers each exchange from a script and keeps a clock that pauses advance.
struct Scripted {
    answers: VecDeque<Answer>,
    sent: Vec<Request>,
    now: Duration,
    /// What [`Daemon::starting`] answers: a daemon holds the lock or started a moment ago.
    starting: bool,
}

impl Scripted {
    fn new(answers: Vec<Answer>) -> Scripted {
        Scripted {
            answers: answers.into(),
            sent: Vec::new(),
            now: Duration::ZERO,
            starting: false,
        }
    }

    fn verbs(&self) -> Vec<String> {
        self.sent
            .iter()
            .map(|r| format!("{} {}", r.verb, r.args.join(" ")).trim().to_owned())
            .collect()
    }
}

impl Daemon for Scripted {
    fn exchange(
        &mut self,
        req: &Request,
        progress: Option<&mut dyn FnMut(&str)>,
    ) -> Result<Reply, Cut> {
        if let Some(progress) = progress {
            progress("rebasing");
        }
        self.sent.push(req.clone());
        // Each exchange takes a second.
        self.now += Duration::from_secs(1);
        let cut = |sent, handle: Option<&str>, kind| Cut {
            sent,
            handle: handle.map(|h| Handle::new(h).unwrap()),
            error: io::Error::new(kind, "the daemon went away"),
        };
        match self.answers.pop_front().unwrap_or(Answer::Refused) {
            Answer::Reply(reply) => Ok(reply),
            Answer::Refused => Err(cut(false, None, ErrorKind::ConnectionRefused)),
            Answer::Missing => Err(cut(false, None, ErrorKind::NotFound)),
            Answer::Dropped(handle) => Err(cut(true, handle, ErrorKind::UnexpectedEof)),
        }
    }

    fn pause(&mut self, pause: Duration) {
        self.now += pause;
    }

    fn elapsed(&self) -> Duration {
        self.now
    }

    fn starting(&self) -> bool {
        self.starting
    }
}

fn request(verb: &str) -> Request {
    Request {
        verb: verb.to_owned(),
        args: Vec::new(),
        cwd: PathBuf::from("/repo/trees/tree-01"),
        session: None,
        wait_s: 540,
        pid: None,
    }
}

fn passed() -> Reply {
    Reply::text(Exit::Success, "passed   rust:test")
}

#[test]
fn a_command_cut_off_by_a_restart_waits_on_its_handle_and_prints_the_same_reply() {
    let mut daemon = Scripted::new(vec![
        Answer::Dropped(Some("T-12")),
        // The daemon is starting again.
        Answer::Refused,
        Answer::Refused,
        Answer::Reply(passed()),
    ]);
    let reply = send(&mut daemon, &request("test"), None, true).unwrap();
    assert_eq!(reply, passed());
    assert_eq!(
        daemon.verbs(),
        ["test", "wait T-12", "wait T-12", "wait T-12"]
    );
    // The wait lasts what is left of the request's own.
    assert!(daemon.sent[1].wait_s < 540 && daemon.sent[1].wait_s > 500);
}

#[test]
fn a_reattached_wait_gives_up_after_five_minutes_and_names_the_handle() {
    let mut daemon = Scripted::new(vec![Answer::Dropped(Some("L-3"))]);
    let reply = send(&mut daemon, &request("land"), None, true).unwrap();
    assert_eq!(reply.exit, Exit::Usage);
    assert!(
        reply
            .text
            .contains("has not answered for 5 minutes since `ignite land` (L-3)")
            && reply.text.contains("ignite wait L-3"),
        "{}",
        reply.text
    );
    assert!(daemon.elapsed() >= RETRY_FOR);
}

#[test]
fn a_read_cut_off_before_a_handle_is_sent_again() {
    let mut daemon = Scripted::new(vec![
        Answer::Dropped(None),
        Answer::Refused,
        Answer::Reply(passed()),
    ]);
    assert_eq!(
        send(&mut daemon, &request("status"), None, true).unwrap(),
        passed()
    );
    assert_eq!(daemon.verbs(), ["status", "status", "status"]);
}

#[test]
fn work_cut_off_before_a_handle_is_not_sent_again_and_its_outcome_is_unknown() {
    let mut daemon = Scripted::new(vec![Answer::Dropped(None), Answer::Reply(passed())]);
    let reply = send(&mut daemon, &request("close"), None, true).unwrap();
    assert_eq!(reply.exit, Exit::Conflict);
    assert!(reply.text.starts_with("unknown "), "{}", reply.text);
    assert!(reply.text.contains("ignite status"), "{}", reply.text);
    assert_eq!(daemon.verbs(), ["close"]);
}

#[test]
fn a_daemon_that_is_starting_is_sent_the_request_again_and_a_missing_one_fails_at_once() {
    let mut daemon = Scripted::new(vec![
        Answer::Refused,
        Answer::Refused,
        Answer::Reply(passed()),
    ]);
    let mut steps = Vec::new();
    let mut show = |step: &str| steps.push(step.to_owned());
    assert_eq!(
        send(&mut daemon, &request("land"), Some(&mut show), true).unwrap(),
        passed()
    );
    assert_eq!(daemon.verbs(), ["land", "land", "land"]);
    assert_eq!(steps.len(), 3);

    let mut missing = Scripted::new(vec![Answer::Missing, Answer::Reply(passed())]);
    let cut = send(&mut missing, &request("land"), None, true).unwrap_err();
    assert_eq!(cut.error.kind(), ErrorKind::NotFound);
    assert_eq!(missing.verbs(), ["land"]);

    // Without retries for an unsent request, a refused connection goes back to the caller (the
    // dispatcher's fallback waits by itself).
    let mut refused = Scripted::new(vec![Answer::Refused, Answer::Reply(passed())]);
    assert!(send(&mut refused, &request("dispatch"), None, false).is_err());
    assert_eq!(refused.verbs(), ["dispatch"]);
}

#[test]
fn a_daemon_that_stays_down_is_given_up_on_after_five_minutes() {
    let mut daemon = Scripted::new(Vec::new());
    let cut = send(&mut daemon, &request("test"), None, true).unwrap_err();
    assert_eq!(cut.error.kind(), ErrorKind::ConnectionRefused);
    assert!(daemon.elapsed() >= RETRY_FOR);
}

/// On Windows the old daemon's pipe is gone before the new daemon creates its own, so the
/// socket is missing while the new daemon starts. A client that finds it missing waits while a
/// daemon is starting, and only then.
#[test]
fn a_missing_socket_is_retried_while_a_daemon_is_starting() {
    let mut daemon = Scripted::new(vec![
        Answer::Missing,
        Answer::Missing,
        Answer::Refused,
        Answer::Reply(passed()),
    ]);
    daemon.starting = true;
    assert_eq!(
        send(&mut daemon, &request("land"), None, true).unwrap(),
        passed()
    );
    assert_eq!(daemon.verbs(), ["land", "land", "land", "land"]);

    let mut stopped = Scripted::new(vec![Answer::Missing, Answer::Reply(passed())]);
    stopped.starting = false;
    let cut = send(&mut stopped, &request("land"), None, true).unwrap_err();
    assert_eq!(cut.error.kind(), ErrorKind::NotFound);
    assert_eq!(stopped.verbs(), ["land"]);
}
