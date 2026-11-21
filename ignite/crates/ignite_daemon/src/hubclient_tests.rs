use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::*;
use ignite_core::ids::{MachineId, RecordId, SessionId, TicketId, TreeId};
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::NextReply;
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
    client.restart_window = Duration::ZERO;
    client
}

/// A port where nothing listens.
fn stopped_hub_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A client for a port where nothing listens: the hub is stopped.
pub(crate) fn stopped_hub() -> HubClient {
    let port = stopped_hub_port();
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
        code: None,
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

/// A client that tries once more, then goes on while the hub looks restarting for up to
/// `window`, pausing 10 ms between tries.
fn restart_tolerant(port: u16, window: Duration) -> HubClient {
    let mut client =
        HubClient::with_base(format!("http://127.0.0.1:{port}/test/api"), "secret", "m-1");
    client.retries = 1;
    client.retry_pause = Duration::from_millis(10);
    client.restart_window = window;
    client
}

#[test]
fn a_hub_behind_a_proxy_that_answers_502_while_it_restarts_is_waited_for() {
    // `tailscale serve` answers 502 while nothing listens behind it, here for six requests,
    // far more than `retries` covers.
    let calls = Arc::new(Mutex::new(0));
    let counter = calls.clone();
    let hub = FakeHttpHub::start(move |_, _, _| {
        let mut calls = counter.lock().unwrap();
        *calls += 1;
        if *calls <= 6 {
            (502, "Bad Gateway".to_owned())
        } else {
            (200, "{}".to_owned())
        }
    });
    restart_tolerant(hub.port, Duration::from_secs(10))
        .records(&[record("test")])
        .unwrap();
    assert_eq!(*calls.lock().unwrap(), 7);
}

#[test]
fn a_server_error_that_is_not_a_restart_is_tried_only_retries_times() {
    let calls = Arc::new(Mutex::new(0));
    let counter = calls.clone();
    let hub = FakeHttpHub::start(move |_, _, _| {
        *counter.lock().unwrap() += 1;
        (500, "broken".to_owned())
    });
    let error = restart_tolerant(hub.port, Duration::from_secs(10))
        .records(&[record("test")])
        .unwrap_err();
    assert!(matches!(error, Error::Unreachable(_)), "{error:?}");
    assert_eq!(*calls.lock().unwrap(), 2);
}

#[test]
fn a_hub_whose_port_refuses_connections_while_it_restarts_is_waited_for() {
    // The hub's port is free for 400 ms, as between the old hub's exit and the new one's
    // bind, then a hub listens there again.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let back = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        let server = tiny_http::Server::http(("127.0.0.1", port)).unwrap();
        let request = server.recv().unwrap();
        let _ = request.respond(tiny_http::Response::from_string("{}"));
    });
    restart_tolerant(port, Duration::from_secs(10))
        .records(&[record("test")])
        .unwrap();
    back.join().unwrap();
}

#[test]
fn a_hub_that_stays_down_is_given_up_after_the_restart_window() {
    let client = restart_tolerant(stopped_hub_port(), Duration::from_millis(300));
    let began = std::time::Instant::now();
    let error = client.records(&[record("test")]).unwrap_err();
    assert!(matches!(error, Error::Unreachable(_)), "{error:?}");
    assert!(began.elapsed() >= Duration::from_millis(300));
    assert!(began.elapsed() < Duration::from_secs(5));
}

#[test]
fn the_restart_window_covers_a_hub_restart() {
    assert!(RESTART_WINDOW >= Duration::from_secs(15));
    let client = HubClient::with_base("http://127.0.0.1:1/test/api", "", "m-1");
    assert_eq!(client.restart_window, RESTART_WINDOW);
}

#[test]
fn records_come_back_with_the_ids_the_hub_gave_them() {
    let hub = FakeHttpHub::start(|_, _, _| (200, r#"{"ids": [41, 42]}"#.to_owned()));
    let ids = hub
        .client()
        .numbered_records(&[record("test"), record("land")])
        .unwrap();
    assert_eq!(ids, [RecordId(41), RecordId(42)]);
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
        pressure: None,
        defer_successor: false,
    };
    assert!(matches!(client.landed(&landed), Err(Error::NotFound(_))));
    assert!(matches!(client.records(&[]), Err(Error::Unreachable(m)) if m.contains("token")));
    // A client error is not tried again.
    assert_eq!(hub.seen().len(), 3);
}

#[test]
fn a_body_too_large_is_a_verdict_that_is_not_tried_again() {
    let hub = FakeHttpHub::start(|_, _, _| (413, r#"{"error": "too large"}"#.to_owned()));
    let error = hub.client().records(&[record("test")]).unwrap_err();
    assert!(too_large(&error), "{error}");
    assert_eq!(hub.seen().len(), 1);
    // A malformed body is not too large.
    assert!(!too_large(&Error::Invalid(
        "400 bad request body".to_owned()
    )));
}

#[test]
fn a_read_only_listener_is_named_once_and_never_retried() {
    let hub =
        FakeHttpHub::start(|_, _, _| (405, r#"{"error":"this listener is read-only"}"#.to_owned()));
    let client = hub.client();
    let log = FailureLog::default();
    let mut said = 0;
    for _ in 0..4 {
        let error = client.records(&[record("test")]).unwrap_err();
        assert!(read_only(&error), "{error}");
        assert!(error.to_string().contains("point [hub] url"), "{error}");
        assert!(error.to_string().contains(&client.base), "{error}");
        said += usize::from(log.worth_saying(&error));
    }
    assert_eq!(said, 1);
    // One request per round: a 405 is not tried again.
    assert_eq!(hub.seen().len(), 4);
    // After a write succeeds, the next read-only answer is said again.
    log.succeeded();
    assert!(log.worth_saying(&Error::Unreachable(format!("{READ_ONLY}again"))));
    // Other failures are always said, and a 405 from elsewhere is not a read-only listener.
    let other = Error::Unreachable("the hub answered 502".to_owned());
    assert!(log.worth_saying(&other) && log.worth_saying(&other));
    let proxy = FakeHttpHub::start(|_, _, _| (405, "method not allowed".to_owned()));
    let error = proxy.client().records(&[record("test")]).unwrap_err();
    assert!(!read_only(&error), "{error}");
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
        admit_cores: None,
        pressure: None,
        families: Vec::new(),
        name: None,
        justiciar_only: false,
        master_only: false,
        explain: false,
    };
    assert_eq!(hub.client().next(&free).unwrap(), None);
    // An older hub answers `null` to a request for the reason, too.
    assert_eq!(
        hub.client().next_explained(&free).unwrap(),
        NextReply::Nothing(None)
    );
    let why =
        FakeHttpHub::start(|_, _, _| (200, r#"{"nothing": "no ticket tree is free"}"#.to_owned()));
    assert_eq!(
        why.client().next_explained(&free).unwrap(),
        NextReply::Nothing(Some("no ticket tree is free".to_owned()))
    );
}

#[test]
fn listings_are_looked_up_and_stored_by_family_and_hash() {
    let hub = FakeHttpHub::start(|_, url, _| {
        if url.ends_with("/listings/lookup") {
            (200, r#"{"listings": ["[1]", null]}"#.to_owned())
        } else {
            (200, r#"{"ok": true}"#.to_owned())
        }
    });
    let key = |family: &str| ListingKey {
        family: family.to_owned(),
        hash: ignite_core::ids::InputHash::new("b".repeat(64)).unwrap(),
    };
    let found = hub.client().listings(&[key("unit"), key("rs")]).unwrap();
    assert_eq!(found, [Some("[1]".to_owned()), None]);
    let stored = StoredListing {
        key: key("unit"),
        listing: "[1]".to_owned(),
    };
    hub.client().store_listing(&stored).unwrap();
    let seen = hub.seen();
    assert_eq!(seen[0].url, "/test/api/listings/lookup");
    assert_eq!(seen[1].url, "/test/api/listings/store");
    let sent: StoredListing = serde_json::from_str(&seen[1].body).unwrap();
    assert_eq!(sent, stored);
    assert!(matches!(
        stopped_hub().listings(&[key("unit")]),
        Err(Error::Unreachable(_))
    ));
}

#[test]
fn verdicts_are_looked_up_and_stored_by_their_routes() {
    use ignite_core::protocol::hub::{Verdict, VerdictKey};
    let key = VerdictKey {
        tree: "tree".to_owned(),
        base: "3a9e4c1".parse().unwrap(),
        settings: "settings".to_owned(),
    };
    let verdict = Verdict {
        key: key.clone(),
        head: "4b0f5d2".parse().unwrap(),
        machine: "host".to_owned(),
        passed_at: Timestamp::EPOCH,
        families: vec!["unit".to_owned()],
        left_to_tip: 1,
        always_not_run: Vec::new(),
    };
    let reply = serde_json::to_string(&verdict).unwrap();
    let hub = FakeHttpHub::start(move |_, url, _| {
        if url.ends_with("/verdicts/lookup") {
            (200, reply.clone())
        } else {
            (200, r#"{"ok": true}"#.to_owned())
        }
    });
    assert_eq!(hub.client().verdict(&key).unwrap(), Some(verdict.clone()));
    hub.client().store_verdict(&verdict).unwrap();
    let seen = hub.seen();
    assert_eq!(seen[0].url, "/test/api/verdicts/lookup");
    assert_eq!(
        serde_json::from_str::<VerdictKey>(&seen[0].body).unwrap(),
        key
    );
    assert_eq!(seen[1].url, "/test/api/verdicts/store");
    assert_eq!(
        serde_json::from_str::<Verdict>(&seen[1].body).unwrap(),
        verdict
    );
    let none = FakeHttpHub::start(|_, _, _| (200, "null".to_owned()));
    assert_eq!(none.client().verdict(&key).unwrap(), None);
}

#[test]
fn tip_costs_rounds_the_last_round_and_rates_use_their_routes() {
    use ignite_core::tipround::{TipCostsRequest, TipRound};
    let hub = FakeHttpHub::start(|_, url, _| {
        if url.ends_with("/tips/costs") {
            (
                200,
                r#"{"claim_seconds": 60.0, "costs": {"unit:a": 0.5}}"#.to_owned(),
            )
        } else if url.ends_with("/tips/rounds") {
            (200, r#"{"ticket": "t.1", "created": true}"#.to_owned())
        } else if url.ends_with("/rates") {
            (
                200,
                r#"{"day": "2026-11-19", "land": 18.0, "tip": null}"#.to_owned(),
            )
        } else {
            (200, r#"{"round": null}"#.to_owned())
        }
    });
    let request = TipCostsRequest {
        commit: ignite_core::ids::Commit::new("abcd").unwrap(),
        family: "unit".to_owned(),
        claim_seconds: 60.0,
    };
    let costs = hub.client().tip_costs(&request).unwrap();
    assert!((costs.costs["unit:a"] - 0.5).abs() < f64::EPSILON);
    let round = TipRound {
        commit: request.commit.clone(),
        machine: ignite_core::ids::MachineId::new("m-1").unwrap(),
        at: ignite_core::time::Timestamp::EPOCH,
        wall_s: 1.0,
        ran: 1,
        cached: 0,
        checks_s: 1.0,
        listing_s: 0.0,
        listing_alone_s: None,
        building_s: 0.0,
        waiting_s: 0.0,
        budget: None,
    };
    assert_eq!(
        hub.client().tip_round(&round).unwrap().ticket.as_deref(),
        Some("t.1")
    );
    assert_eq!(hub.client().last_tip_round().unwrap(), None);
    let rates = hub.client().rates().unwrap();
    assert_eq!((rates.land, rates.tip), (Some(18.0), None));
    let urls: Vec<String> = hub
        .seen()
        .into_iter()
        .map(|s| s.method + " " + &s.url)
        .collect();
    assert_eq!(
        urls,
        [
            "POST /test/api/tips/costs",
            "POST /test/api/tips/rounds",
            "GET /test/api/tips/rounds/last",
            "GET /test/api/rates"
        ]
    );
}

#[test]
fn an_export_names_its_tickets_and_reads_json_lines() {
    let line = |id: &str| {
        format!(
            r#"{{"id":"{id}","parent":null,"next_child":1,"title":"t","type":"task","status":"open","priority":2,"assignee":null,"defer_until":null,"close_reason":null,"created_at":"2026-01-01T00:00:00.000Z","created_by":"x","updated_at":"2026-01-01T00:00:00.000Z","closed_at":null,"version":3}}"#
        )
    };
    let body = format!("{}\n{}\n", line("T.1"), line("T.2"));
    let hub = FakeHttpHub::start(move |_, _, _| (200, body.clone()));
    let ids = [TicketId::new("T.1").unwrap(), TicketId::new("T.2").unwrap()];
    let tickets = hub.client().export(None, &ids).unwrap();
    let got: Vec<&str> = tickets.iter().map(|w| w.ticket.id.as_str()).collect();
    assert_eq!(got, ["T.1", "T.2"]);
    assert_eq!(hub.seen()[0].url, "/test/api/export?id=T.1&id=T.2");
}

#[test]
fn a_refused_token_is_read_again_from_the_identity_file_and_sent_once_more() {
    // `ignite install` saved a new token while the client held the old one: the hub refuses the
    // old one, and the client sends the request again with the token in the file.
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-hubclient-reread-{}", std::process::id()));
    let file = crate::update::IdentityFile {
        path: dir.join("machine.json"),
    };
    file.save(&crate::update::Identity {
        machine: MachineId::new("m-1").unwrap(),
        token: Some("fresh".to_owned()),
    })
    .unwrap();
    let answered = Arc::new(Mutex::new(0));
    let hub = {
        let answered = answered.clone();
        FakeHttpHub::start(move |_, _, _| {
            let mut count = answered.lock().unwrap();
            *count += 1;
            if *count == 1 {
                (401, r#"{"error":"Missing or unknown token"}"#.to_owned())
            } else {
                (200, r#"{"ids": []}"#.to_owned())
            }
        })
    };
    let client = hub.client().reading_token_from(file);
    client.records(&[]).unwrap();
    let sent: Vec<Option<String>> = hub.seen().into_iter().map(|s| s.authorization).collect();
    assert_eq!(
        sent,
        [
            Some("Bearer secret".to_owned()),
            Some("Bearer fresh".to_owned())
        ]
    );
    assert_eq!(client.token(), "fresh", "later requests send the new token");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_token_still_refused_after_reading_it_again_fails_once_and_names_install() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-hubclient-refused-{}", std::process::id()));
    let file = crate::update::IdentityFile {
        path: dir.join("machine.json"),
    };
    file.save(&crate::update::Identity {
        machine: MachineId::new("m-1").unwrap(),
        token: Some("secret".to_owned()),
    })
    .unwrap();
    let hub = FakeHttpHub::start(|_, _, _| (401, "{}".to_owned()));
    let client = hub.client().reading_token_from(file);
    let error = client.records(&[]).unwrap_err();
    assert!(
        matches!(&error, Error::Unreachable(m) if m.contains("even after it was read again") && m.contains("ignite install")),
        "{error}"
    );
    assert_eq!(
        hub.seen().len(),
        2,
        "one request and one more after reading the token"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
