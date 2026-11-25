use std::path::PathBuf;

use ignite_core::config::Config;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::MachineId;
use ignite_core::protocol::client::Request;
use ignite_core::protocol::hub::ReleaseCause;
use ignite_core::protocol::tickets::Operation;
use ignite_core::traits::Clock;

use super::*;
use crate::pool::PoolVerb;
use crate::server::Verb;
use ignite_core::protocol::hub::WorkerState;

#[derive(Default)]
pub(super) struct FakeHost {
    pub(super) trees: Mutex<u32>,
    cores: Mutex<f64>,
    admit: Mutex<f64>,
    pub(super) waited: Mutex<f64>,
    pub(super) cpu_percent: Mutex<Option<u32>>,
    holders: Mutex<BTreeMap<SessionId, TicketId>>,
    pub(super) commands: Mutex<BTreeMap<SessionId, SessionCommand>>,
    pub(super) quiet: Mutex<BTreeMap<SessionId, Timestamp>>,
    pub(super) verbs: Mutex<BTreeMap<SessionId, Vec<RunningVerb>>>,
    pub(super) worker_capacity: Mutex<(u32, bool, Option<u64>)>,
    pub(super) interrupts: Mutex<Vec<(SessionId, String)>>,
    pub(super) releases: Mutex<Vec<(SessionId, TicketId)>>,
    /// Every release fails, as one at the hub does when the hub is unreachable.
    pub(super) fail_release: Mutex<bool>,
    /// Each session's title in the desktop app; a session missing here has none that can be read.
    pub(super) titles: Mutex<BTreeMap<SessionId, String>>,
    /// The live processes under each process, with the asker's branch already left out; a
    /// process missing here cannot be read.
    pub(super) processes: Mutex<BTreeMap<u32, Vec<u32>>>,
    /// Each session's latest outcome of each check, as the result cache records them.
    pub(super) sightings: Mutex<Vec<CheckSighting>>,
    /// Where `green` points; `None` when there is no `green` branch.
    pub(super) green: Mutex<Option<String>>,
}

pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Host for FakeHost {
    fn free_ticket_trees(&self) -> u32 {
        *lock(&self.trees)
    }

    fn free_cores(&self) -> f64 {
        *lock(&self.cores)
    }

    fn admit_cores(&self) -> f64 {
        *lock(&self.admit)
    }

    fn ticket_work_wait(&self) -> f64 {
        *lock(&self.waited)
    }

    fn pressure(
        &self,
        _now: Timestamp,
    ) -> Option<ignite_core::dispatch_pressure::DispatchPressure> {
        Some(ignite_core::dispatch_pressure::DispatchPressure {
            build_slots: 6,
            build_slots_free: 6,
            queue_seconds: self.ticket_work_wait().floor() as u32,
            cpu_percent: *lock(&self.cpu_percent),
            ..Default::default()
        })
    }

    fn worker_capacity(&self) -> (u32, bool, Option<u64>) {
        *lock(&self.worker_capacity)
    }

    fn holders(&self) -> BTreeMap<SessionId, TicketId> {
        lock(&self.holders).clone()
    }

    fn command(&self, session: &SessionId) -> Option<SessionCommand> {
        lock(&self.commands).get(session).cloned()
    }

    fn quiet_since(&self, session: &SessionId) -> Option<Timestamp> {
        lock(&self.quiet).get(session).copied()
    }

    fn running_verbs(&self, session: &SessionId) -> Vec<RunningVerb> {
        lock(&self.verbs).get(session).cloned().unwrap_or_default()
    }

    fn interrupt(&self, session: &SessionId, reply: Reply) -> bool {
        lock(&self.interrupts).push((session.clone(), reply.text));
        true
    }

    fn title(&self, session: &SessionId) -> Option<String> {
        lock(&self.titles).get(session).cloned()
    }

    fn running_under(&self, root: u32, _asker: Option<u32>) -> Option<Vec<u32>> {
        lock(&self.processes).get(&root).cloned()
    }

    fn check_sightings(&self, since: Timestamp) -> Vec<CheckSighting> {
        let since = since.to_string();
        lock(&self.sightings)
            .iter()
            .filter(|s| s.at >= since)
            .cloned()
            .collect()
    }

    fn green(&self) -> Option<String> {
        lock(&self.green).clone()
    }

    /// Saving frees the session's tree, as the real one does.
    fn free_lost(&self, session: &SessionId, ticket: &TicketId) -> Result<Option<String>> {
        let held = lock(&self.holders).remove(session);
        Ok((held.as_ref() == Some(ticket)).then(|| format!("saved/{ticket}")))
    }

    /// Releasing frees the session's tree, as the real release does.
    fn release(
        &self,
        session: &SessionId,
        ticket: &TicketId,
        _reason: &str,
        _cause: ReleaseCause,
    ) -> Result<()> {
        if *lock(&self.fail_release) {
            return Err(Error::Unreachable("the hub is away".to_owned()));
        }
        lock(&self.holders).remove(session);
        lock(&self.releases).push((session.clone(), ticket.clone()));
        Ok(())
    }
}

pub(super) struct Rig {
    pub(super) ctx: Context,
    pub(super) clock: Arc<FakeClock>,
    pub(super) git: Arc<FakeGit>,
    pub(super) hub: Arc<FakeHub>,
    pub(super) host: Arc<FakeHost>,
    pub(super) headless_dispatch: Arc<HeadlessDispatch>,
}

pub(super) const START: i64 = 1_800_000_000_000;

pub(super) fn rig(workers: &[&str]) -> Rig {
    rig_on(
        workers,
        ignite_store::local::open_in_memory().unwrap(),
        Timestamp::from_millis(START),
    )
}

/// Creates headless dispatch state backed by `db`, with the clock set to `now`. Opening the same
/// database again simulates a daemon restart.
pub(super) fn rig_on(workers: &[&str], db: rusqlite::Connection, now: Timestamp) -> Rig {
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
        ticket_trees = 4
        landing_trees = 1
        [pool.models.claude]
        high = "opus"
        opus = "opus"
        sonnet = "sonnet"
        light = "light"
    "#;
    let clock = Arc::new(FakeClock::new(now));
    let git = Arc::new(FakeGit::default());
    let hub = Arc::new(FakeHub::default());
    let ctx = Context {
        config: Config::parse("ignite.toml", text).unwrap(),
        repo: PathBuf::from("/repo"),
        machine: MachineId::new("m-1").unwrap(),
        clock: clock.clone(),
        hub: hub.clone(),
        git: git.clone(),
        launcher: Arc::new(FakeLauncher::default()),
        builds: Arc::new(FakeBuilds::default()),
        session: None,
        tree: None,
    };
    let host = Arc::new(FakeHost::default());
    *lock(&host.trees) = 8;
    lock(&host.worker_capacity).0 = 16;
    *lock(&host.cores) = 10.0;
    *lock(&host.admit) = 15.0;
    let pool = Pool::new(db);
    let setup = Setup {
        os: Os::Macos,
        families: Vec::new(),
    };
    let headless_dispatch = Arc::new(HeadlessDispatch::new(pool, host.clone(), setup));
    let rig = Rig {
        ctx,
        clock,
        git,
        hub,
        host,
        headless_dispatch,
    };
    for name in workers {
        pool_member(&rig, name);
    }
    rig
}

fn pool_member(rig: &Rig, name: &str) {
    rig.headless_dispatch
        .pool()
        .put(&crate::pool::Member {
            session: session(name),
            app: "claude".to_owned(),
            down: false,
            models: vec!["sonnet".to_owned(), "opus".to_owned()],
            files: Vec::new(),
            added_at: rig.clock.now(),
            single_use: name.starts_with("cli-"),
        })
        .unwrap();
}

pub(super) fn session(name: &str) -> SessionId {
    SessionId::new(name).unwrap()
}

pub(super) fn ticket_id(name: &str) -> TicketId {
    TicketId::new(name).unwrap()
}

pub(super) fn request(verb: &str, args: &[&str], session: Option<&str>) -> Request {
    Request {
        verb: verb.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        cwd: PathBuf::from("/repo"),
        session: session.map(|s| SessionId::new(s).unwrap()),
        wait_s: 0,
        pid: None,
    }
}

/// A ticket for the fake hub, from the fields that matter here.
pub(super) fn detail(
    id: &str,
    status: &str,
    holder: Option<&str>,
    labels: &[&str],
    body: &str,
) -> TicketDetail {
    serde_json::from_value(serde_json::json!({
        "id": id, "parent": null, "next_child": 1, "title": format!("Ticket {id}"),
        "body": body, "type": "task", "status": status, "priority": 2, "assignee": holder,
        "labels": labels, "defer_until": null, "close_reason": null,
        "created_at": "2027-01-01T00:00:00.000Z", "created_by": "test",
        "updated_at": "2027-01-01T00:00:00.000Z", "closed_at": null, "version": 1,
        "session": holder, "edges": [], "children": [], "ready": false, "blocked_by": [],
    }))
    .unwrap()
}

impl Rig {
    pub(super) fn pool(&self, args: &[&str], session: Option<&str>) -> Reply {
        PoolVerb::new(self.headless_dispatch.clone())
            .run(&self.ctx, &request("pool", args, session))
    }

    pub(super) fn tell(&self, args: &[&str]) -> Reply {
        TellVerb.run(&self.ctx, &request("tell", args, None))
    }

    pub(super) fn tell_as(&self, session: &str, args: &[&str]) -> Reply {
        TellVerb.run(&self.ctx, &request("tell", args, Some(session)))
    }

    pub(super) fn hold(&self, worker: &str, ticket: &str) {
        lock(&self.host.holders).insert(session(worker), ticket_id(ticket));
    }

    pub(super) fn finish(&self, worker: &str) {
        lock(&self.host.holders).remove(&session(worker));
    }

    pub(super) fn state_of(&self, worker: &str) -> String {
        let view = self.headless_dispatch.view().unwrap();
        view.into_iter()
            .find(|w| w.session == session(worker))
            .unwrap()
            .state
    }
}

#[test]
fn resuming_every_machine_ends_the_emergency_here_at_once() {
    use crate::emergency::{Emergencies, EmergencyVerb};
    let rig = rig(&[]);
    // The fake hub declares emergencies at the epoch.
    rig.clock.set(Timestamp::EPOCH);
    let emergencies = Arc::new(Emergencies::new(rig.clock.clone(), None));
    let declare = request("emergency", &["declare", "TJUS1"], Some("local_j"));
    let declared = EmergencyVerb(emergencies.clone()).run(&rig.ctx, &declare);
    assert_eq!(declared.exit, Exit::Success, "{}", declared.text);
    assert!(emergencies.in_force().is_some());
    let pool = PoolVerb::new(rig.headless_dispatch.clone()).ending(emergencies.clone());
    let resumed = pool.run(
        &rig.ctx,
        &request("pool", &["resume", "--all"], Some("local_j")),
    );
    assert!(
        resumed.text.contains("ended    emergency TJUS1"),
        "{}",
        resumed.text
    );
    assert!(emergencies.in_force().is_none());
}

#[test]
fn resuming_every_machine_refuses_an_emergency_another_session_declared() {
    use crate::emergency::{Emergencies, EmergencyVerb};
    let rig = rig(&[]);
    rig.clock.set(Timestamp::EPOCH);
    let emergencies = Arc::new(Emergencies::new(rig.clock.clone(), None));
    let declare = request("emergency", &["declare", "TJUS1"], Some("local_j"));
    assert_eq!(
        EmergencyVerb(emergencies.clone())
            .run(&rig.ctx, &declare)
            .exit,
        Exit::Success
    );
    let pool = PoolVerb::new(rig.headless_dispatch.clone()).ending(emergencies.clone());
    let resume =
        |session: &str, args: &[&str]| pool.run(&rig.ctx, &request("pool", args, Some(session)));

    let refused = resume("local_w", &["resume", "--all"]);
    assert_ne!(refused.exit, Exit::Success, "{}", refused.text);
    assert!(
        refused.text.contains("TJUS1") && refused.text.contains("local_j"),
        "{}",
        refused.text
    );
    assert!(emergencies.in_force().is_some());
    let wrong = resume("local_w", &["resume", "--all", "--emergency", "TOTHR"]);
    assert_ne!(wrong.exit, Exit::Success, "{}", wrong.text);
    let missing = resume("local_w", &["resume", "--all", "--emergency"]);
    assert_eq!(missing.exit, Exit::Usage, "{}", missing.text);
    assert!(emergencies.in_force().is_some());

    let ended = resume("local_w", &["resume", "--all", "--emergency=TJUS1"]);
    assert_eq!(ended.exit, Exit::Success, "{}", ended.text);
    assert!(
        ended.text.contains("ended    emergency TJUS1"),
        "{}",
        ended.text
    );
    assert!(emergencies.in_force().is_none());
}

#[test]
fn tell_names_the_sending_session_in_the_comment() {
    let rig = rig(&[]);
    rig.hub.insert_ticket(detail("T.1", "open", None, &[], ""));
    let reply = rig.tell_as("local_j1", &["T.1", "Stop building."]);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    let ops = &rig.hub.batches()[0].ops;
    let [Operation::Comment { text, .. }] = ops.as_slice() else {
        panic!("{ops:?}");
    };
    assert_eq!(text, "from local_j1: Stop building.");
}

#[test]
fn tell_all_comments_on_every_ticket_in_progress_in_one_batch() {
    let rig = rig(&[]);
    rig.hub
        .insert_ticket(detail("T.1", "in_progress", Some("local_w1"), &[], ""));
    rig.hub
        .insert_ticket(detail("T.2", "in_progress", Some("local_w2"), &[], ""));
    rig.hub.insert_ticket(detail("T.3", "open", None, &[], ""));
    assert_eq!(
        rig.tell(&["--all", "Skip oracle checks."]).exit,
        Exit::Success
    );
    let batches = rig.hub.batches();
    assert_eq!(batches.len(), 1);
    let commented: Vec<&str> = batches[0]
        .ops
        .iter()
        .filter_map(|op| match op {
            Operation::Comment { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(commented, ["T.1", "T.2"]);
}

#[test]
fn tell_on_a_ticket_nobody_holds_is_only_a_comment() {
    let rig = rig(&["local_w1"]);
    rig.hub.insert_ticket(detail("T.1", "open", None, &[], ""));
    rig.headless_dispatch.poll_feed(&rig.ctx, 0).unwrap();
    let reply = rig.tell(&["T.1", "Use the new helper."]);
    assert!(reply.text.contains("nobody holds it"), "{}", reply.text);
    let ops = &rig.hub.batches()[0].ops;
    assert!(matches!(ops.as_slice(), [Operation::Comment { .. }]));
    rig.headless_dispatch.poll_feed(&rig.ctx, 0).unwrap();
    assert!(lock(&rig.host.interrupts).is_empty());
}

#[test]
fn the_pool_shows_the_command_each_worker_runs() {
    let rig = rig(&["cli-claude-00000001"]);
    rig.hold("cli-claude-00000001", "T.1");
    let since = rig.clock.now();
    lock(&rig.host.commands).insert(
        session("cli-claude-00000001"),
        SessionCommand {
            command: "wait L-5521".to_owned(),
            since,
            running: true,
        },
    );
    let reply = rig.pool(&["list"], None);
    assert!(reply.text.contains("`wait L-5521`"), "{}", reply.text);
    let view: Vec<WorkerState> = serde_json::from_value(reply.json.unwrap()).unwrap();
    assert_eq!(view[0].command.as_deref(), Some("wait L-5521"));
    assert_eq!(view[0].command_since, Some(since));
}

#[test]
fn a_draining_daemon_asks_the_hub_for_nothing_until_it_stops() {
    use super::cli_tests::{cli, reserve, tick};
    let rig = rig(&[]);
    let fake = cli(&rig, 1);
    reserve(&rig, "T.1", "reservation-1", &[]);
    rig.headless_dispatch.set_draining(true);
    tick(&rig);
    assert!(rig.hub.asked().is_empty());
    rig.headless_dispatch.set_draining(false);
    tick(&rig);
    assert_eq!(fake.begun()[0].0.ticket.as_str(), "T.1");
}
