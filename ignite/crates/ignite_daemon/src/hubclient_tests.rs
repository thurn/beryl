use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::*;
use ignite_core::ids::{MachineId, SessionId, TicketId, TreeId};
use ignite_core::protocol::Class;
use ignite_core::time::Timestamp;

/// One request the stand-in hub received.
#[derive(Clone, Debug)]
pub(crate) struct Seen {
    pub method: String,
    pub url: String,
    pub authorization: Option<String>,
    pub actor: Option<String>,
    pub body: String,
}

type Answer = Box<dyn Fn(&str, &str, &str) -> (u16, String) + Send + Sync>;

/// A hub on a local port that answers every request with `answer(method, url, body)` and
/// remembers what it was sent.
pub(crate) struct FakeHttpHub {
    server: Arc<tiny_http::Server>,
    thread: Option<JoinHandle<()>>,
    pub port: u16,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

impl FakeHttpHub {
    pub fn start(
        answer: impl Fn(&str, &str, &str) -> (u16, String) + Send + Sync + 'static,
    ) -> Self {
        let answer: Answer = Box::new(answer);
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let port = server.server_addr().to_ip().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let thread = {
            let (server, seen) = (server.clone(), seen.clone());
            std::thread::spawn(move || {
                for mut request in server.incoming_requests() {
                    let header = |name: &'static str| {
                        request
                            .headers()
                            .iter()
                            .find(|h| h.field.equiv(name))
                            .map(|h| h.value.to_string())
                    };
                    let mut body = String::new();
                    let (authorization, actor) = (header("Authorization"), header("X-Actor"));
                    request.as_reader().read_to_string(&mut body).unwrap();
                    let method = request.method().to_string();
                    let url = request.url().to_owned();
                    let (status, reply) = answer(&method, &url, &body);
                    seen.lock().unwrap().push(Seen {
                        method,
                        url,
                        authorization,
                        actor,
                        body,
                    });
                    let response = tiny_http::Response::from_string(reply).with_status_code(status);
                    let _ = request.respond(response);
                }
            })
        };
        FakeHttpHub {
            server,
            thread: Some(thread),
            port,
            seen,
        }
    }

    /// A client for this hub that retries once, without pausing.
    pub fn client(&self) -> HubClient {
        quick(HubClient::with_base(
            format!("http://127.0.0.1:{}/test/api", self.port),
            "secret",
            "m-1",
        ))
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for FakeHttpHub {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn quick(mut client: HubClient) -> HubClient {
    client.retries = 1;
    client.retry_pause = Duration::ZERO;
    client
}

/// A client for a port where nothing listens: the hub is stopped.
pub(crate) fn stopped_hub() -> HubClient {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    quick(HubClient::with_base(
        format!("http://127.0.0.1:{port}/test/api"),
        "secret",
        "m-1",
    ))
}

pub(crate) fn record(verb: &str) -> Record {
    Record {
        machine: MachineId::new("m-1").unwrap(),
        ticket: None,
        session: None,
        tree: None,
        verb: verb.to_owned(),
        class: Class::Agent,
        family: None,
        queued_s: 0.0,
        wall_s: 1.0,
        cpu_s: 1.0,
        instructions: None,
        peak_mb: 10,
        outcome: None,
        commit: None,
        ignite: "abc".to_owned(),
        at: Timestamp::EPOCH,
        compile_units: Vec::new(),
    }
}

fn claim() -> Claim {
    Claim {
        ticket: TicketId::new("a.1").unwrap(),
        machine: MachineId::new("m-1").unwrap(),
        session: SessionId::new("local_a").unwrap(),
        tree: TreeId::new("tree-01").unwrap(),
    }
}

#[test]
fn a_request_carries_the_token_and_the_actor_and_reads_the_reply() {
    let hub = FakeHttpHub::start(|_, _, _| (200, r#"{"results": [null]}"#.to_owned()));
    let found = hub.client().lookup(&[]).unwrap();
    assert_eq!(found, [None]);
    let seen = hub.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        (seen[0].method.as_str(), seen[0].url.as_str()),
        ("POST", "/test/api/results/lookup")
    );
    assert_eq!(seen[0].authorization.as_deref(), Some("Bearer secret"));
    assert_eq!(seen[0].actor.as_deref(), Some("m-1"));
    assert_eq!(seen[0].body, r#"{"keys":[]}"#);
}

#[test]
fn a_stopped_hub_is_unreachable() {
    let error = stopped_hub().claim(&claim()).unwrap_err();
    assert!(matches!(error, Error::Unreachable(_)), "{error:?}");
}

#[test]
fn a_server_error_is_tried_again() {
    let calls = Arc::new(Mutex::new(0));
    let counter = calls.clone();
    let hub = FakeHttpHub::start(move |_, _, _| {
        let mut calls = counter.lock().unwrap();
        *calls += 1;
        if *calls == 1 {
            (503, "busy".to_owned())
        } else {
            (200, "{}".to_owned())
        }
    });
    hub.client().records(&[record("test")]).unwrap();
    assert_eq!(*calls.lock().unwrap(), 2);
}

#[test]
fn statuses_become_the_shared_errors() {
    let hub = FakeHttpHub::start(|_, url, _| match url {
        "/test/api/claims" => (409, r#"{"error": "claimed by local_b"}"#.to_owned()),
        "/test/api/landed" => (404, "{}".to_owned()),
        _ => (401, "{}".to_owned()),
    });
    let client = hub.client();
    assert!(matches!(client.claim(&claim()), Err(Error::Conflict(m)) if m.contains("local_b")));
    let landed = Landed {
        ticket: TicketId::new("a.1").unwrap(),
        machine: MachineId::new("m-1").unwrap(),
        session: SessionId::new("local_a").unwrap(),
        work: ignite_core::protocol::tickets::LandedWork::default(),
        fresh: false,
    };
    assert!(matches!(client.landed(&landed), Err(Error::NotFound(_))));
    assert!(matches!(client.records(&[]), Err(Error::Invalid(m)) if m.contains("token")));
    // A client error is not tried again.
    assert_eq!(hub.seen().len(), 3);
}

#[test]
fn a_busy_lease_is_an_answer_not_an_error() {
    let hub = FakeHttpHub::start(|_, _, _| {
        (
            409,
            r#"{"holder": "local_b", "expires_at": null}"#.to_owned(),
        )
    });
    let op = LeaseOp::Release {
        holder: "local_a".to_owned(),
        if_revision: None,
    };
    let reply = hub.client().lease("land", op).unwrap();
    let LeaseReply::Busy(busy) = reply else {
        panic!("expected busy, got {reply:?}");
    };
    assert_eq!(busy.holder.as_deref(), Some("local_b"));
    assert_eq!(hub.seen()[0].url, "/test/api/leases/land");
}

#[test]
fn the_feed_is_a_long_poll_from_a_position() {
    let hub = FakeHttpHub::start(|_, _, _| (200, r#"{"seq": 9, "changes": []}"#.to_owned()));
    let batch = hub.client().feed(Seq(4), 0).unwrap();
    assert_eq!(batch.seq, Seq(9));
    let seen = hub.seen();
    assert_eq!(
        (seen[0].method.as_str(), seen[0].url.as_str()),
        ("GET", "/test/api/feed?since=4&wait=0")
    );
}

#[test]
fn next_reads_nothing_as_no_reservation() {
    let hub = FakeHttpHub::start(|_, _, _| (200, "null".to_owned()));
    let free = MachineFree {
        machine: MachineId::new("m-1").unwrap(),
        os: ignite_core::config::Os::Macos,
        idle_workers: Vec::new(),
        free_ticket_trees: 1,
        free_cores: 4.0,
        families: Vec::new(),
    };
    assert_eq!(hub.client().next(&free).unwrap(), None);
}
