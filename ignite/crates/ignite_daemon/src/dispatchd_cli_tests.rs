//! CLI workers take reservations, receive messages, and resume when their commands finish.

use ignite_core::config::CliProgram;
use ignite_core::protocol::hub::Reservation;
use ignite_core::traits::Clock;

use super::tell_tests::claimed;
use super::tests::{Rig, detail, lock, rig, session, ticket_id};
use super::wake::WAKE;
use super::*;
use crate::clid::{FakeCliPool, SessionCheck};

/// Configures Claude in the fake pool and sets the machine-wide worker ceiling.
pub(super) fn cli(rig: &Rig, ceiling: u32) -> Arc<FakeCliPool> {
    cli_with(rig, ceiling, Arc::new(FakeCliPool::default()))
}

/// [`cli`] through `fake`, which a restarted rig shares with the one before it, as a restarted
/// daemon finds the slots its CLI pool wrote to the database.
pub(super) fn cli_with(rig: &Rig, ceiling: u32, fake: Arc<FakeCliPool>) -> Arc<FakeCliPool> {
    let mut config = rig.ctx.config.clone();
    config.pool.cli.insert(
        "claude".to_owned(),
        CliProgram {
            program: "claude".to_owned(),
            ..CliProgram::default()
        },
    );
    lock(&rig.host.worker_capacity).0 = ceiling;
    rig.dispatcher.set_cli(fake.clone(), &config);
    fake
}

/// The fake hub reserves a Claude worker for the ticket.
pub(super) fn reserve(rig: &Rig, ticket: &str, key: &str, labels: &[&str]) {
    rig.hub
        .insert_ticket(detail(ticket, "open", None, labels, ""));
    offer(rig, ticket, "claude", key);
}

fn offer(rig: &Rig, ticket: &str, provider: &str, key: &str) {
    queue_offer(rig, ticket, provider, key, false);
}

fn queue_offer(rig: &Rig, ticket: &str, provider: &str, key: &str, master_blocker: bool) {
    rig.hub.queue_reservation(Reservation {
        master_blocker,
        provider: Some(provider.to_owned()),
        ticket: ticket_id(ticket),
        machine: rig.ctx.machine.clone(),
        session: session(key),
        expires_at: rig.clock.now().plus_seconds(600.0),
        code: "7f3a".to_owned(),
    });
}

pub(super) fn tick(rig: &Rig) {
    rig.dispatcher.tick(&rig.ctx);
}

#[test]
fn an_app_in_cli_mode_hands_off_through_the_cli_pool_without_a_dispatcher() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    reserve(&rig, "T.1", "reservation-1", &["tier:opus"]);
    tick(&rig);

    let begun = fake.begun();
    assert_eq!(begun.len(), 1);
    let (reservation, model, effort) = &begun[0];
    assert_eq!(
        (reservation.ticket.as_str(), reservation.session.as_str()),
        ("T.1", "reservation-1")
    );
    assert_eq!(reservation.code, "7f3a");
    assert_eq!(
        (model.as_str(), effort.as_deref()),
        ("opus", Some("medium"))
    );

    // The new session joins the pool, holds the reservation, and fills a slot.
    let member = rig
        .dispatcher
        .pool()
        .member(&session("cli-claude-00000001"))
        .unwrap()
        .expect("the CLI session joins the pool");
    assert!(member.single_use);
    assert_eq!(rig.state_of("cli-claude-00000001"), "reserved");
    rig.hold("cli-claude-00000001", "T.1");
    assert_eq!(rig.state_of("cli-claude-00000001"), "working");
    assert_eq!(rig.dispatcher.view().unwrap().len(), 1);

    let asked = &rig.hub.asked()[0];
    assert!(asked.idle_workers.is_empty());
    let room = asked.room.as_ref().unwrap();
    assert_eq!(room.start, 2);
    assert_eq!(room.providers.len(), 1);
    assert_eq!(room.providers[0].name, "claude");
}

#[test]
fn configured_providers_offer_their_tiers_and_use_their_effort_defaults() {
    let fake = Arc::new(FakeCliPool::default());
    let mut rig = rig(&[]);
    rig.ctx.config.pool.default_effort = [("fake".to_owned(), "high".to_owned())].into();
    rig.ctx.config.pool.models.insert(
        "fake".to_owned(),
        [("custom".to_owned(), "fake-model".to_owned())].into(),
    );
    rig.ctx.config.pool.cli.insert(
        "fake".to_owned(),
        CliProgram {
            program: "fake-cli".to_owned(),
            dialect: Some("codex".to_owned()),
            ..CliProgram::default()
        },
    );
    rig.dispatcher.set_cli(fake.clone(), &rig.ctx.config);
    rig.hub.insert_ticket(detail("T.1", "open", None, &[], ""));
    rig.hub.insert_ticket(detail(
        "T.2",
        "open",
        None,
        &["tier:custom", "effort:medium"],
        "",
    ));
    offer(&rig, "T.1", "fake", "reservation-1");
    offer(&rig, "T.2", "fake", "reservation-2");
    tick(&rig);

    let begun = fake.begun();
    assert_eq!(begun.len(), 2);
    assert_eq!(begun[1].1, "fake-model");
    assert_eq!(rig.dispatcher.cli_apps().collect::<Vec<_>>(), ["fake"]);
    let asks = rig.hub.asked();
    let providers = &asks[0].room.as_ref().unwrap().providers;
    assert_eq!(providers[0].name, "fake");
    assert_eq!(providers[0].tiers, ["custom"]);
    assert_eq!(providers[0].model_ids["custom"], "fake-model");
    let efforts: BTreeMap<_, _> = begun
        .iter()
        .map(|(reservation, _, effort)| (reservation.ticket.as_str(), effort.as_deref()))
        .collect();
    assert_eq!(
        efforts,
        [("T.1", Some("high")), ("T.2", Some("medium"))].into()
    );
}

#[test]
fn zero_room_allows_the_hub_to_offer_a_ticket_that_holds_master_back() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    reserve(&rig, "T.1", "reservation-1", &[]);
    reserve(&rig, "T.2", "reservation-2", &[]);
    tick(&rig);
    rig.hold("cli-claude-00000001", "T.1");
    rig.hold("cli-claude-00000002", "T.2");
    let before = rig.hub.asked().len();
    tick(&rig);
    let asked = rig.hub.asked();
    assert_eq!(asked.len(), before + 1);
    assert_eq!(asked[before].room.as_ref().unwrap().start, 0);
    assert!(!asked[before].master_only);
    assert!(
        rig.dispatcher
            .worker_room_line()
            .unwrap()
            .contains("ceiling 2")
    );

    rig.hub.insert_ticket(detail("R.1", "open", None, &[], ""));
    queue_offer(&rig, "R.1", "claude", "reservation-3", true);
    tick(&rig);
    assert_eq!(fake.begun()[2].0.session.as_str(), "reservation-3");
}

#[test]
fn a_tell_and_a_stall_check_for_a_cli_session_reach_the_cli_pool() {
    let rig = rig(&[]);
    let fake = cli(&rig, 1);
    reserve(&rig, "T.1", "reservation-1", &[]);
    tick(&rig);
    let worker = "cli-claude-00000001";
    rig.hold(worker, "T.1");
    lock(&rig.host.commands).insert(
        session(worker),
        SessionCommand {
            command: "start T.1".to_owned(),
            since: rig.clock.now(),
            running: false,
        },
    );
    tick(&rig);

    // A tell made after the claim is queued for the session's next turn.
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    rig.hub.insert_ticket(claimed(
        "T.1",
        worker,
        &[(1, "2027-01-02T00:00:00.000Z", "Use the new helper.")],
        "2027-01-01T00:00:00.000Z",
    ));
    rig.hub.push_change("ticket", Some(ticket_id("T.1")));
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    let sent = fake.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].session, session(worker));
    assert_eq!(sent[0].text, "Message about T.1: Use the new helper.");
    assert!(!sent[0].urgent);

    // Silent for 30 minutes: the CLI pool answers the check, and an idle session is reminded.
    fake.set_check(&session(worker), SessionCheck::Running);
    rig.clock.advance(SILENT_S + 1.0);
    tick(&rig);
    assert_eq!(fake.sent().len(), 1, "a running session is left alone");
    fake.set_check(&session(worker), SessionCheck::Idle);
    rig.clock.advance(SILENT_S + 1.0);
    tick(&rig);
    assert_eq!(fake.sent()[1].text, CONTINUE);

    // Still idle 15 minutes after the reminder: the ticket is released, as for any worker.
    rig.clock.advance(IDLE_AFTER_MESSAGE_S + 1.0);
    tick(&rig);
    assert_eq!(
        *lock(&rig.host.releases),
        [(session(worker), ticket_id("T.1"))]
    );
}

#[test]
fn a_cli_session_is_woken_as_soon_as_its_verb_ends() {
    let rig = rig(&[]);
    let fake = cli(&rig, 1);
    reserve(&rig, "T.1", "reservation-1", &[]);
    tick(&rig);
    let worker = session("cli-claude-00000001");
    rig.hold(worker.as_str(), "T.1");
    tick(&rig);
    lock(&rig.host.commands).insert(
        worker.clone(),
        SessionCommand {
            command: "test ignite".to_owned(),
            since: rig.clock.now(),
            running: false,
        },
    );
    rig.dispatcher.note_turn_end(&worker, true, rig.clock.now());
    tick(&rig);
    let sent = fake.sent();
    assert_eq!(sent.len(), 1, "no two-minute wait for a CLI session");
    assert_eq!(sent[0].text, WAKE);
}

#[test]
fn a_failed_start_returns_the_reservation_and_backs_off_the_provider() {
    let rig = rig(&[]);
    let fake = cli(&rig, 1);
    fake.refuse(Some("the program is missing"));
    reserve(&rig, "T.1", "reservation-1", &[]);
    tick(&rig);
    assert!(rig.dispatcher.view().unwrap().is_empty());
    assert_eq!(rig.hub.refused_reservations()[0].ticket, ticket_id("T.1"));
    let before = rig.hub.asked().len();
    tick(&rig);
    assert_eq!(rig.hub.asked().len(), before);
    fake.refuse(None);
    rig.clock.advance(REFUSED_S + 1.0);
    reserve(&rig, "T.1", "reservation-1", &[]);
    tick(&rig);
    assert_eq!(fake.begun().len(), 1);
}

#[test]
fn a_reservation_for_an_unknown_provider_is_returned_without_starting() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    offer(&rig, "T.1", "missing", "reservation-1");
    tick(&rig);
    assert_eq!(rig.hub.refused_reservations().len(), 1);
    assert!(fake.begun().is_empty());
    assert!(rig.dispatcher.view().unwrap().is_empty());
}

#[test]
fn memory_pressure_reports_zero_room() {
    let rig = rig(&[]);
    let _fake = cli(&rig, 2);
    *lock(&rig.host.worker_capacity) = (2, true, Some(8_000));
    tick(&rig);
    let asked = rig.hub.asked();
    let room = asked[0].room.as_ref().unwrap();
    assert_eq!(room.start, 0);
    assert!(room.memory_pressed);
    assert_eq!(
        rig.dispatcher.worker_room_line().as_deref(),
        Some("worker room limited by memory")
    );
}

#[test]
fn retrying_an_unread_reservation_rechecks_worker_memory() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    *lock(&rig.host.worker_capacity) = (2, false, Some(8_000));
    offer(&rig, "T.1", "claude", "reservation-1");
    tick(&rig);
    assert!(fake.begun().is_empty());
    assert!(rig.hub.refused_reservations().is_empty());

    rig.hub.insert_ticket(detail("T.1", "open", None, &[], ""));
    *lock(&rig.host.worker_capacity) = (2, false, Some(3_023));
    tick(&rig);
    assert!(fake.begun().is_empty());
    assert_eq!(rig.hub.refused_reservations()[0].ticket, ticket_id("T.1"));
}

#[test]
fn a_started_worker_holds_its_footprint_while_its_claim_is_pending() {
    let rig = rig(&[]);
    let fake = cli(&rig, 3);
    *lock(&rig.host.worker_capacity) = (3, false, Some(3_024));
    reserve(&rig, "T.1", "reservation-1", &[]);
    tick(&rig);
    assert_eq!(fake.begun().len(), 1);

    reserve(&rig, "T.2", "reservation-2", &[]);
    tick(&rig);
    assert_eq!(fake.begun().len(), 1);
    assert_eq!(rig.hub.refused_reservations()[0].ticket, ticket_id("T.2"));

    // The claim makes the worker visible to the running count. No startup debit remains.
    rig.hold("cli-claude-00000001", "T.1");
    reserve(&rig, "T.2", "reservation-3", &[]);
    tick(&rig);
    assert_eq!(fake.begun()[1].0.ticket, ticket_id("T.2"));
}

#[test]
fn the_hub_down_justiciar_runs_as_a_cli_session() {
    let rig = rig(&["local_w1"]);
    let fake = cli(&rig, 1);
    let given = rig
        .dispatcher
        .hand_off_justiciar(&rig.ctx, "Repair the hub.")
        .expect("a CLI justiciar starts");
    let started = fake.justiciars();
    assert_eq!(started.len(), 1);
    assert_eq!(
        (
            started[0].app.as_str(),
            started[0].model.as_str(),
            started[0].effort.as_str()
        ),
        ("claude", "opus", "high")
    );
    assert_eq!(started[0].prompt, "Repair the hub.");
    assert!(cli::is_cli_session(&given));
    assert!(
        rig.dispatcher
            .hand_off_justiciar(&rig.ctx, "Repair the hub.")
            .is_none(),
        "one justiciar at a time"
    );
}

#[test]
fn an_unprobed_or_held_provider_is_not_offered() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    fake.set_probed("claude", false);
    tick(&rig);
    assert!(rig.hub.asked().is_empty());

    fake.set_probed("claude", true);
    fake.set_hold(
        "claude",
        Some(ignite_core::cli::Hold {
            app: "claude".to_owned(),
            reason: ignite_core::cli::HoldReason::NotLoggedIn,
            since: rig.clock.now(),
            cli_version: Some("2.1.323".to_owned()),
            message: "Invalid API key".to_owned(),
        }),
    );
    tick(&rig);
    assert!(rig.hub.asked().is_empty());

    fake.set_hold("claude", None);
    tick(&rig);
    assert_eq!(
        rig.hub.asked()[0].room.as_ref().unwrap().providers[0].name,
        "claude"
    );
}

/// A reserved regression whose culprit's CLI session ended when it landed asks
/// the CLI pool to resume the culprit's transcript with the regression's hand-off prompt.
#[test]
fn a_regression_for_a_cli_slot_names_its_culprit_to_the_cli_pool() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    let body = "caused-by T.1 (commit 5e1b0aa)\nfirst bad 5e1b0aa, last good 77c2d14";
    rig.hub.insert_ticket(detail(
        "R",
        "open",
        None,
        &["regression", "tier:opus"],
        body,
    ));
    offer(&rig, "R", "claude", "reservation-1");
    tick(&rig);

    assert_eq!(fake.begun().len(), 1);
    let resume = fake.resumes()[0].clone().expect("the culprit is named");
    assert_eq!(resume.culprit.unwrap().as_str(), "T.1");
    assert!(
        resume
            .prompt
            .starts_with("R is a regression caused by your landing of T.1")
            && resume.prompt.contains("ignite start R --reservation 7f3a"),
        "{}",
        resume.prompt
    );
    // An ordinary ticket names no culprit.
    reserve(&rig, "T.2", "reservation-2", &["tier:opus"]);
    tick(&rig);
    assert_eq!(fake.resumes().get(1), Some(&None));
}

/// Commits the landing in the real hub and drops both HTTP replies from the first delivery.
fn hub_with_lost_landing_reply(rig: &Rig) -> crate::hubclient::tests::FakeHttpHub {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ignite_hub::auth::Writer;
    use ignite_hub::routes::{Request, dispatch, tickets};

    use crate::hubclient::tests::FakeHttpHub;

    let machine = rig.ctx.machine.clone();
    let clock = rig.clock.clone();
    let request = move |method: &str, path: &str, body: &str| Request {
        method: method.to_owned(),
        path: path.to_owned(),
        query: Vec::new(),
        writer: Some(Writer {
            machine: machine.clone(),
            actor: "test".to_owned(),
        }),
        body: body.as_bytes().to_vec(),
        headers: Vec::new(),
    };
    let db = Mutex::new(ignite_store::hub::open_in_memory().unwrap());
    let landing_requests = AtomicUsize::new(0);
    FakeHttpHub::start(move |method, url, body| {
        let path = url.strip_prefix("/test/api").unwrap();
        let path = path.split_once('?').map_or(path, |(path, _)| path);
        let req = request(method, path, body);
        let mut db = lock(&db);
        let now = clock.now();
        let response = dispatch::handle(&mut db, &req, now, &[])
            .or_else(|| tickets::handle(&mut db, &req, now))
            .unwrap();
        if path == "/landed" && landing_requests.fetch_add(1, Ordering::SeqCst) < 2 {
            assert_eq!(response.status, 200);
            return (502, "the proxy lost the hub's reply".to_owned());
        }
        (response.status, String::from_utf8(response.body).unwrap())
    })
}

fn file_test_lane(hub: &dyn ignite_core::traits::Hub) -> (TicketId, TicketId) {
    use ignite_core::protocol::tickets::BatchRequest;
    use serde_json::json;

    let batch: BatchRequest = serde_json::from_value(json!({
        "ops": [
            {"op": "create", "fields": {"title": "first", "labels": ["lane:test"]}},
            {"op": "create", "fields": {"title": "second", "labels": ["lane:test", "tier:opus"]},
             "deps": [{"on": "$0"}]}
        ]
    }))
    .unwrap();
    let filed = hub.batch(&batch).unwrap();
    (filed.results[0].id.clone(), filed.results[1].id.clone())
}

#[test]
fn a_lost_landing_reply_delivers_the_lane_successor_once_the_cli_turn_ends() {
    use ignite_core::ids::Commit;
    use ignite_core::protocol::hub::{Claim, Landed};
    use ignite_core::protocol::tickets::{LandedWork, Status};

    use crate::land::records::{LandingRecord, RecordState, Records};

    let mut rig = rig(&[]);
    rig.ctx.config.pool.models.insert(
        "codex".to_owned(),
        [("opus".to_owned(), "codex-model".to_owned())].into(),
    );
    let fake = Arc::new(FakeCliPool::default());
    rig.dispatcher.set_cli(fake.clone(), &rig.ctx.config);
    let worker = session("cli-codex-00000001");
    let now = rig.clock.now();
    let http = hub_with_lost_landing_reply(&rig);
    rig.ctx.hub = Arc::new(http.client());

    let (first, second) = file_test_lane(rig.ctx.hub.as_ref());
    let claim = Claim {
        ticket: first.clone(),
        machine: rig.ctx.machine.clone(),
        session: worker.clone(),
        tree: "tree-01".parse().unwrap(),
        code: None,
    };
    rig.ctx.hub.claim(&claim).unwrap();
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-lane-landing-{}", std::process::id()));
    let records = Records::new(&dir);
    let commit = "3a9e4c1".parse::<Commit>().unwrap();
    let record = LandingRecord {
        landed: Landed {
            ticket: first.clone(),
            machine: rig.ctx.machine.clone(),
            session: worker.clone(),
            work: LandedWork {
                commits: vec![commit.clone()],
                files: Vec::new(),
                note: "Landed the first ticket.".to_owned(),
            },
            fresh: false,
            pressure: None,
            defer_successor: false,
        },
        commit,
        state: RecordState::Pushed,
        written_at: Some(now),
        tree: None,
    };
    records.write(&record).unwrap();
    assert!(matches!(
        records.deliver(rig.ctx.hub.as_ref(), &record, Ok(None)),
        Err(Error::Unreachable(_))
    ));
    assert_eq!(
        rig.ctx.hub.ticket(&first).unwrap().ticket.status,
        Status::Closed
    );

    let send_pending = || {
        records
            .deliver_all(
                rig.ctx.hub.as_ref(),
                |landed, next| {
                    rig.dispatcher
                        .chain(&rig.ctx, &landed.ticket, next, landed.fresh);
                },
                || Ok(None),
            )
            .unwrap();
    };
    fake.set_check(&worker, SessionCheck::Running);
    send_pending();
    assert!(fake.begun().is_empty());
    assert_eq!(records.get(&first).unwrap(), Some(record));

    fake.set_check(&worker, SessionCheck::Gone);
    send_pending();
    send_pending();
    let begun = fake.begun();
    assert_eq!(begun.len(), 1);
    let (next, model, _) = &begun[0];
    assert_eq!(next.session, worker);
    assert_eq!(next.ticket, second);
    assert_eq!(model, "codex-model");
    rig.ctx
        .hub
        .claim(&Claim {
            ticket: second,
            code: Some(next.code.clone()),
            ..claim
        })
        .unwrap();
    send_pending();
    assert_eq!(fake.begun().len(), 1);
    assert!(records.all().unwrap().is_empty());
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn capacity_reports_probe_and_hold_independently() {
    use ignite_core::capacity::ProviderCapacity;
    use ignite_core::cli::{Hold, HoldReason};

    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    fake.set_hold(
        "claude",
        Some(Hold {
            app: "claude".into(),
            reason: HoldReason::PlanLimit,
            since: rig.clock.now(),
            cli_version: None,
            message: "The plan is at its limit".into(),
        }),
    );
    let mut expected = ProviderCapacity {
        name: "claude".into(),
        tiers: ["light", "opus", "sonnet"].map(str::to_owned).into(),
        probed: true,
        hold_reason: Some("plan_limit".into()),
        model_ids: ["light", "opus", "sonnet"]
            .into_iter()
            .map(|tier| (tier.to_owned(), tier.to_owned()))
            .collect(),
        default_model: Some("sonnet".into()),
    };
    assert_eq!(rig.dispatcher.provider_capacity(), [expected.clone()]);
    fake.set_probed("claude", false);
    fake.set_hold("claude", None);
    expected.probed = false;
    expected.hold_reason = None;
    assert_eq!(rig.dispatcher.provider_capacity(), [expected]);
}
