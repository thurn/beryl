use std::path::PathBuf;

use super::*;
use ignite_core::config::Config;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::MachineId;
use ignite_core::protocol::emergency::EMERGENCY_LASTS_S;

use crate::server::{Registry, Server};

fn emergency(at: Timestamp) -> Emergency {
    Emergency {
        ticket: TicketId::new("TJUS1").unwrap(),
        session: SessionId::new("local_j").unwrap(),
        machine: MachineId::new("m-2").unwrap(),
        declared_at: at,
        renewed_at: at,
        allowed: vec![Allowance {
            ticket: TicketId::new("TWORK").unwrap(),
            verb: Some("land".to_owned()),
        }],
    }
}

fn session(name: &str) -> SessionId {
    SessionId::new(name).unwrap()
}

fn ticket(name: &str) -> TicketId {
    TicketId::new(name).unwrap()
}

fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(Timestamp::EPOCH))
}

#[test]
fn heavy_verbs_refuse_with_their_own_exit_code_and_reading_verbs_do_not() {
    let emergencies = Emergencies::new(clock(), None);
    let worker = session("local_w");
    assert!(emergencies.refusal("test", Some(&worker), None).is_none());

    emergencies.heard(Some(emergency(Timestamp::EPOCH)));
    let refused = emergencies.refusal("test", Some(&worker), None).unwrap();
    assert_eq!(refused.exit, Exit::Paused);
    assert!(refused.text.contains("TJUS1"), "{}", refused.text);
    assert!(
        refused.text.contains("ignite wait emergency"),
        "{}",
        refused.text
    );
    for verb in ["status", "ticket", "wait", "guide", "sync", "why"] {
        assert!(
            emergencies.refusal(verb, Some(&worker), None).is_none(),
            "{verb}"
        );
    }

    emergencies.heard(None);
    assert!(emergencies.refusal("test", Some(&worker), None).is_none());
}

#[test]
fn the_justiciar_its_ticket_and_the_tickets_it_allows_are_not_refused() {
    let emergencies = Emergencies::new(clock(), None);
    emergencies.heard(Some(emergency(Timestamp::EPOCH)));
    let worker = session("local_w");
    assert!(
        emergencies
            .refusal("land", Some(&session("local_j")), None)
            .is_none()
    );
    // A justiciar with no session, such as a Codex chat, works in the justiciar ticket's tree.
    assert!(
        emergencies
            .refusal("land", None, Some(&ticket("TJUS1")))
            .is_none()
    );
    assert!(
        emergencies
            .refusal("land", Some(&worker), Some(&ticket("TWORK")))
            .is_none()
    );
    assert!(
        emergencies
            .refusal("test", Some(&worker), Some(&ticket("TWORK")))
            .is_some()
    );
    assert!(
        emergencies
            .refusal("start", Some(&worker), Some(&ticket("TOTHR")))
            .is_some()
    );
}

#[test]
fn an_emergency_nobody_renews_lapses_on_the_daemon_too() {
    let clock = clock();
    let emergencies = Emergencies::new(clock.clone(), None);
    emergencies.heard(Some(emergency(Timestamp::EPOCH)));
    clock.set(Timestamp::EPOCH.plus_seconds(EMERGENCY_LASTS_S - 1.0));
    assert!(emergencies.refusal("check", None, None).is_some());
    clock.set(Timestamp::EPOCH.plus_seconds(EMERGENCY_LASTS_S));
    assert!(emergencies.refusal("check", None, None).is_none());
    assert!(emergencies.lines().is_empty());
}

#[test]
fn a_local_clear_lifts_only_that_emergency_and_outlives_a_restart() {
    let dir = std::env::temp_dir().join(format!("ignite-emergency-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("daemon.db");
    let _ = std::fs::remove_file(&path);
    let open = || ignite_store::local::open(&path).unwrap();
    let emergencies = Emergencies::new(clock(), Some(open()));
    emergencies.heard(Some(emergency(Timestamp::EPOCH)));
    drop(emergencies);

    // A restarted daemon enforces what it heard before, even before the hub answers.
    let restarted = Emergencies::new(clock(), Some(open()));
    assert!(restarted.refusal("land", None, None).is_some());
    assert_eq!(restarted.clear_locally().unwrap().ticket, ticket("TJUS1"));
    assert!(restarted.refusal("land", None, None).is_none());
    assert!(
        restarted
            .lines()
            .iter()
            .any(|l| l.contains("clear --local"))
    );
    drop(restarted);

    let again = Emergencies::new(clock(), Some(open()));
    assert!(again.refusal("land", None, None).is_none());
    // Renewing the same emergency keeps it lifted; a new declaration is enforced again.
    let mut renewed = emergency(Timestamp::EPOCH);
    renewed.renewed_at = Timestamp::EPOCH.plus_seconds(60.0);
    again.heard(Some(renewed));
    assert!(again.refusal("land", None, None).is_none());
    again.heard(Some(emergency(Timestamp::EPOCH.plus_seconds(120.0))));
    assert!(again.refusal("land", None, None).is_some());
    drop(again);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn status_and_doctor_show_the_emergency_with_its_age_and_owner() {
    let clock = clock();
    let emergencies = Arc::new(Emergencies::new(clock.clone(), None));
    assert!(EmergencySection(emergencies.clone()).lines().is_empty());
    assert!(emergencies.finding().is_none());

    emergencies.heard(Some(emergency(Timestamp::EPOCH)));
    clock.set(Timestamp::EPOCH.plus_seconds(25.0 * 60.0));
    let lines = EmergencySection(emergencies.clone()).lines();
    assert!(
        lines[0].starts_with("TJUS1, justiciar local_j on m-2"),
        "{lines:?}"
    );
    assert!(lines[0].contains("declared 25 min ago"), "{lines:?}");
    assert!(lines[0].contains("lapses in 1 h 35 min"), "{lines:?}");
    assert_eq!(lines[1], "allowed TWORK (land)");
    let finding = emergencies.finding().unwrap();
    assert!(finding.warning);
    assert!(
        finding.line().contains("ignite emergency clear --local"),
        "{}",
        finding.line()
    );
}

#[test]
fn waiting_returns_the_all_clear_once_the_emergency_ends() {
    let emergencies = Emergencies::new(clock(), None);
    let never = || false;
    assert_eq!(emergencies.wait(60, &never).exit, Exit::Success);
    emergencies.heard(Some(emergency(Timestamp::EPOCH)));
    // The fake clock never moves, so a wait of no seconds runs out at once.
    let still = emergencies.wait(0, &never);
    assert_eq!(still.exit, Exit::Running);
    assert!(
        still.text.contains("ignite wait emergency"),
        "{}",
        still.text
    );
    let left = || true;
    assert_eq!(emergencies.wait(60, &left).exit, Exit::Running);
}

/// A heavy verb that always succeeds.
struct Heavy;

impl Verb for Heavy {
    fn name(&self) -> &'static str {
        "test"
    }

    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    fn run(&self, _ctx: &Context, _req: &Request) -> Reply {
        Reply::text(Exit::Success, "ran")
    }
}

fn context(hub: Arc<FakeHub>) -> Context {
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
        clock: clock(),
        hub,
        git: Arc::new(FakeGit::default()),
        launcher: Arc::new(FakeLauncher::default()),
        builds: Arc::new(FakeBuilds::default()),
        session: None,
        tree: None,
    }
}

fn request(verb: &str, args: &[&str], session: &str) -> Request {
    Request {
        verb: verb.to_owned(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: PathBuf::from("/repo"),
        session: Some(SessionId::new(session).unwrap()),
        wait_s: 1,
        pid: None,
    }
}

/// `ignite wait emergency` with no wait, since the fake clock never moves.
fn waiting() -> Request {
    let mut req = request("wait", &["emergency"], "local_w");
    req.wait_s = 0;
    req
}

#[test]
fn the_server_refuses_a_heavy_verb_until_every_machine_resumes() {
    let hub = Arc::new(FakeHub::default());
    let emergencies = Arc::new(Emergencies::new(clock(), None));
    let mut registry = Registry::default();
    registry.register(Box::new(Heavy));
    registry.register(Box::new(EmergencyVerb(emergencies.clone())));
    let mut server = Server::new(context(hub.clone()), registry, None).unwrap();
    server.enforce_emergency(emergencies.clone());

    let declared = server.handle(&request("emergency", &["declare", "TJUS1"], "local_j"));
    assert_eq!(declared.exit, Exit::Success, "{}", declared.text);
    assert_eq!(
        hub.pauses().len(),
        1,
        "declaring pauses hand-offs everywhere"
    );
    assert_eq!(
        server.handle(&request("test", &["unit"], "local_w")).exit,
        Exit::Paused
    );
    assert_eq!(
        server.handle(&request("test", &["unit"], "local_j")).exit,
        Exit::Success
    );
    let waited = server.handle(&waiting());
    assert_eq!(waited.exit, Exit::Running, "{}", waited.text);

    // What `ignite pool resume --all` does here (see the dispatcher's tests), and what the next
    // heartbeat does on every other machine.
    emergencies.heard(None);
    assert_eq!(
        server.handle(&request("test", &["unit"], "local_w")).exit,
        Exit::Success
    );
    let waited = server.handle(&waiting());
    assert_eq!(waited.exit, Exit::Success, "{}", waited.text);
}
