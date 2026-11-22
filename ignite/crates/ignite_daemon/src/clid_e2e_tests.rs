//! CLI mode end to end: a real [`CliDaemon`] over a database file, its turns started by a real
//! `SupervisedLauncher` through [`SupervisedTurns`], read by the registered CLI adapters and decided
//! by the real `decide`, with the clock faked.
//!
//! The supervisor and `ignite internal cli-turn` are this test binary run again with one of the
//! ignored role tests below. The CLI program is a shell script written into each test's home
//! (see [`SCRIPT`]): each run takes the next invocation number `n`, records its arguments and
//! environment as `args.<n>` and `env.<n>`, and then runs `turn.<n>` if the test wrote one, or
//! prints a clean turn. The helpers it offers a turn are `init` (the first line, naming the
//! transcript and the model it was asked for), `say <text>`, `result`, `started` (writes to the
//! FIFO `fifo.<n>`, which the test reads to know the turn runs) and `hang` (waits until the
//! test writes `gate.<n>`).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ignite_core::cli::claude::Claude;
use ignite_core::cli::decide::{INTERRUPTED_TEXT, block_text};
use ignite_core::cli::{CliApp, HoldReason, SlotState, TurnEnd};
use ignite_core::config::Config;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub};
use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::protocol::client::Reply;
use ignite_core::protocol::hub::ReleaseCause;
use ignite_core::protocol::hub::Reservation;
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Result};
use ignite_store::local::cli_slots as store;
use ignite_store::local::rows::CheckSighting;

use super::health::{Health, HealthEffects};
use super::start::{TurnLauncher, find_run};
use super::*;
use crate::dispatchd::{RunningVerb, SessionCommand};
use crate::server::Context;
use crate::supervise::{Started, SupervisedLauncher};

const ROLES: &str = "clid::e2e::";
const START: i64 = 1_800_000_000_000;
const MODEL: &str = "claude-test-1";
const TICKET: &str = "T.1";

/// Run by the tests as a turn's supervisor: the launcher appends the spec file's path.
#[test]
#[ignore = "run by the tests in this module as a supervisor"]
fn role_supervisor() {
    let Some(spec) = std::env::args()
        .next_back()
        .filter(|arg| arg.ends_with("spec.json"))
    else {
        return;
    };
    // libtest has printed `test <name> ... ` without a newline, and the launcher looks for the
    // supervisor's ready line on a line of its own.
    println!();
    let _ = std::io::Write::flush(&mut std::io::stdout());
    std::process::exit(crate::supervise::supervise(Path::new(&spec)));
}

/// Run by the tests as `ignite internal cli-turn`: its arguments after `--` are the Ignite home
/// and then the program to run. The `--` keeps libtest from reading the program's own options,
/// such as `-p`, as its.
#[test]
#[ignore = "run by the tests in this module as ignite internal cli-turn"]
fn role_cli_turn() {
    if std::env::var_os(start::TURN_VARIABLE).is_none() {
        return;
    }
    let args: Vec<String> = std::env::args().collect();
    let Some(at) = args.iter().position(|arg| arg == "--") else {
        return;
    };
    let home = PathBuf::from(&args[at + 1]);
    std::process::exit(start::cli_turn(&home, &args[at + 2..]));
}

/// The scripted CLI program; `@DIR@` is replaced by the folder it keeps its files in.
const SCRIPT: &str = r#"#!/bin/sh
dir='@DIR@'
while ! mkdir "$dir/count.lock" 2>/dev/null; do sleep 0.01; done
n=$(( $(cat "$dir/count" 2>/dev/null || echo 0) + 1 ))
echo $n > "$dir/count"
rmdir "$dir/count.lock"
printf '%s\0' "$@" > "$dir/args.$n"
env > "$dir/env.$n"
model=''; sid='probe'; prev=''
for a in "$@"; do
  case "$prev" in --model) model=$a ;; --resume|--session-id) sid=$a ;; esac
  prev=$a
done
init() { echo "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$sid\",\"model\":\"$model\",\"claude_code_version\":\"9.9.9\"}"; }
say() { echo "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"$1\"}]}}"; }
result() { echo '{"type":"result","subtype":"success","is_error":false,"result":"done","total_cost_usd":0.01,"usage":{"input_tokens":1,"output_tokens":1}}'; }
started() { echo started > "$dir/fifo.$n"; }
hang() { while [ ! -e "$dir/gate.$n" ]; do sleep 0.05; done; }
if [ -f "$dir/turn.$n" ]; then . "$dir/turn.$n"; else init; say done; result; fi
"#;

/// The ticket holders and releases that the CLI pool sees through its host.
#[derive(Default)]
struct E2eHost {
    holders: Mutex<BTreeMap<SessionId, TicketId>>,
    releases: Mutex<Vec<(ReleaseCause, String)>>,
}

impl Host for E2eHost {
    fn free_ticket_trees(&self) -> u32 {
        8
    }
    fn free_cores(&self) -> f64 {
        8.0
    }
    fn admit_cores(&self) -> f64 {
        8.0
    }
    fn ticket_work_wait(&self) -> f64 {
        0.0
    }
    fn holders(&self) -> BTreeMap<SessionId, TicketId> {
        lock(&self.holders).clone()
    }
    fn command(&self, _session: &SessionId) -> Option<SessionCommand> {
        None
    }
    fn quiet_since(&self, _session: &SessionId) -> Option<Timestamp> {
        None
    }
    fn running_verbs(&self, _session: &SessionId) -> Vec<RunningVerb> {
        Vec::new()
    }
    fn interrupt(&self, _session: &SessionId, _reply: Reply) -> bool {
        false
    }
    fn release(
        &self,
        session: &SessionId,
        _ticket: &TicketId,
        reason: &str,
        cause: ReleaseCause,
    ) -> Result<()> {
        lock(&self.holders).remove(session);
        lock(&self.releases).push((cause, reason.to_owned()));
        Ok(())
    }
    fn title(&self, _session: &SessionId) -> Option<String> {
        None
    }
    fn check_sightings(&self, _since: Timestamp) -> Vec<CheckSighting> {
        Vec::new()
    }
    fn green(&self) -> Option<String> {
        None
    }
    fn running_under(&self, _root: u32, _asker: Option<u32>) -> Option<Vec<u32>> {
        None
    }
}

/// The daemon's real effects, with every turn end recorded.
struct Recording {
    inner: HealthEffects,
    ends: Mutex<Vec<TurnEnd>>,
}

impl Effects for Recording {
    fn usage(&self, slot: &Slot, samples: &[UsageSample]) {
        self.inner.usage(slot, samples);
    }
    fn ended(&self, slot: &Slot, end: &TurnEnd, events: &TurnEvents) {
        lock(&self.ends).push(end.clone());
        self.inner.ended(slot, end, events);
    }
    fn hold(&self, app: &str, reason: HoldReason, message: &str, cli_version: Option<&str>) {
        self.inner.hold(app, reason, message, cli_version);
    }
    fn probed(&self, app: &str) -> bool {
        self.inner.probed(app)
    }
    fn claim_is_current(&self, session: &SessionId, ticket: &TicketId) -> Result<bool> {
        self.inner.claim_is_current(session, ticket)
    }
}

/// Turns whose first start fails after the turn was recorded, as a daemon that dies between the
/// two phases of a start leaves it.
struct FailFirst {
    inner: SupervisedTurns,
    failed: AtomicBool,
}

impl Turns for FailFirst {
    fn start(&self, session: &SessionId, turn: u32, command: &CommandSpec) -> Result<()> {
        if !self.failed.swap(true, Ordering::SeqCst) {
            return Err(Error::Io(
                "the daemon died before the supervisor started".into(),
            ));
        }
        self.inner.start(session, turn, command)
    }
    fn recover(&self, session: &SessionId, turn: u32) -> Recovery {
        self.inner.recover(session, turn)
    }
    fn output(&self, session: &SessionId, turn: u32) -> Option<String> {
        self.inner.output(session, turn)
    }
    fn grew_at(&self, session: &SessionId, turn: u32) -> Option<Timestamp> {
        self.inner.grew_at(session, turn)
    }
    fn process(&self, session: &SessionId, turn: u32) -> Option<u32> {
        self.inner.process(session, turn)
    }
    fn end(&self, session: &SessionId, turn: u32) -> Result<()> {
        self.inner.end(session, turn)
    }
}

struct Rig {
    home: PathBuf,
    /// The scripted program's files.
    cli: PathBuf,
    clock: Arc<FakeClock>,
    host: Arc<E2eHost>,
    hub: Arc<FakeHub>,
    ctx: Context,
    health: Arc<Health>,
    effects: Arc<Recording>,
    daemon: Arc<CliDaemon>,
    supervisors: Arc<Mutex<Vec<Started>>>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        // A turn can still be waiting on a gate or FIFO. End it and reap its supervisor
        // before removing the run files, even if the test failed before opening the gate.
        let supervisors = std::mem::take(&mut *lock(&self.supervisors));
        let errors: Vec<_> = supervisors
            .into_iter()
            .filter_map(|started| started.stop_for_test().err())
            .collect();
        let _ = fs::remove_dir_all(&self.home);
        assert!(
            errors.is_empty() || std::thread::panicking(),
            "failed to reap test supervisors: {errors:?}"
        );
    }
}

fn config(program: &Path) -> Config {
    let text = format!(
        r#"
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
        [pool.cli.claude]
        program = "{program}"
        [pool.models.claude]
        light = "{MODEL}"
        [pool.cli.fake]
        dialect = "codex"
        program = "{program}"
        [pool.models.fake]
        custom = "fake-model"
        "#,
        program = program.display()
    );
    Config::parse("ignite.toml", &text).unwrap()
}

impl Rig {
    fn new(name: &str) -> Rig {
        let home = ignite_core::test_files::temp_dir()
            .join(format!("ignite-cli-e2e-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let cli = home.join("cli");
        fs::create_dir_all(&cli).unwrap();
        let program = home.join("fake-claude");
        fs::write(
            &program,
            SCRIPT.replace("@DIR@", &cli.display().to_string()),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let clock = Arc::new(FakeClock::new(Timestamp::from_millis(START)));
        let pool = Arc::new(Pool::new(Self::open(&home)));
        let health = Arc::new(
            Health::new(pool, home.clone(), "local".to_owned()).with_notify(Box::new(|_, _| {})),
        );
        let hub = Arc::new(FakeHub::default());
        let ctx = Context {
            config: config(&program),
            repo: home.clone(),
            machine: MachineId::new("m-7f3a").unwrap(),
            clock: clock.clone(),
            hub: hub.clone(),
            git: Arc::new(FakeGit::default()),
            launcher: Arc::new(Self::supervised(&home)),
            builds: Arc::new(FakeBuilds::default()),
            session: None,
            tree: None,
        };
        let effects = Arc::new(Recording {
            inner: HealthEffects {
                health: health.clone(),
                ctx: ctx.clone(),
            },
            ends: Mutex::new(Vec::new()),
        });
        let host = Arc::new(E2eHost::default());
        let mut rig = Rig {
            daemon: Arc::new(CliDaemon::new(
                CliSetup {
                    checkout: home.clone(),
                    apps: BTreeMap::new(),
                },
                Arc::new(Pool::new(Self::open(&home))),
                host.clone(),
                clock.clone(),
                Arc::new(NoTurns),
            )),
            home,
            cli,
            clock,
            host,
            hub,
            ctx,
            health,
            effects,
            supervisors: Arc::default(),
        };
        rig.restart();
        rig
    }

    fn open(home: &Path) -> rusqlite::Connection {
        ignite_store::local::open(&home.join("daemon.db")).unwrap()
    }

    fn supervised(home: &Path) -> SupervisedLauncher {
        let exe = std::env::current_exe().unwrap().display().to_string();
        SupervisedLauncher::with_command(
            vec![
                exe,
                "--exact".to_owned(),
                format!("{ROLES}role_supervisor"),
                "--ignored".to_owned(),
                "--nocapture".to_owned(),
            ],
            home.join("runs"),
        )
    }

    /// Turns as a newly started daemon builds them.
    fn turns(&self) -> SupervisedTurns {
        let exe = std::env::current_exe().unwrap().display().to_string();
        let cli_turn = vec![
            exe,
            "--exact".to_owned(),
            format!("{ROLES}role_cli_turn"),
            "--ignored".to_owned(),
            "--nocapture".to_owned(),
            // With one test thread, libtest writes its status on the CLI's first JSON line.
            "--quiet".to_owned(),
            "--".to_owned(),
            self.home.display().to_string(),
        ];
        let launcher = TurnLauncher::with_command(
            Arc::new(Self::supervised(&self.home)),
            cli_turn,
            self.home.clone(),
            self.home.clone(),
        );
        SupervisedTurns::new(launcher, self.home.clone())
            .with_test_supervisors(self.supervisors.clone())
    }

    /// Replaces the daemon with a new one over the same database and Ignite home, as a
    /// restarted daemon is built, with `turns` starting its turns.
    fn restart_with(&mut self, turns: Arc<dyn Turns>) {
        let setup = CliSetup::from_config(self.home.clone(), &self.ctx.config);
        self.daemon = Arc::new(
            CliDaemon::new(
                setup,
                Arc::new(Pool::new(Self::open(&self.home))),
                self.host.clone(),
                self.clock.clone(),
                turns,
            )
            .with_effects(self.effects.clone()),
        );
    }

    fn restart(&mut self) {
        let turns = Arc::new(self.turns());
        self.restart_with(turns);
    }

    /// Writes the script of invocation `n` of the program.
    fn script(&self, n: u32, body: &str) {
        fs::write(self.cli.join(format!("turn.{n}")), body).unwrap();
    }

    /// Makes the FIFO that invocation `n` writes to when its script calls `started`.
    fn fifo(&self, n: u32) {
        let fifo = self.cli.join(format!("fifo.{n}"));
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
    }

    /// Waits until invocation `n` has called `started`.
    fn wait_started(&self, n: u32) {
        let text = fs::read_to_string(self.cli.join(format!("fifo.{n}"))).unwrap();
        assert_eq!(text, "started\n");
    }

    fn open_gate(&self, n: u32) {
        fs::write(self.cli.join(format!("gate.{n}")), "").unwrap();
    }

    /// How many times the program has run.
    fn invocations(&self) -> u32 {
        fs::read_to_string(self.cli.join("count")).map_or(0, |text| text.trim().parse().unwrap())
    }

    /// The arguments of invocation `n`.
    fn args(&self, n: u32) -> Vec<String> {
        let bytes = fs::read(self.cli.join(format!("args.{n}"))).unwrap();
        String::from_utf8(bytes)
            .unwrap()
            .split_terminator('\0')
            .map(str::to_owned)
            .collect()
    }

    /// The prompt of invocation `n`, its last argument.
    fn prompt(&self, n: u32) -> String {
        self.args(n).pop().unwrap()
    }

    /// Begins the ticket on slot 1, as `ignite start` would claim it, and returns its session.
    fn begin(&self) -> SessionId {
        self.begin_on("claude", MODEL)
    }

    fn begin_on(&self, provider: &str, model: &str) -> SessionId {
        let session = self.try_begin_on(provider, model).unwrap();
        lock(&self.host.holders).insert(session.clone(), TicketId::new(TICKET).unwrap());
        self.set_hub_claim(&session);
        session
    }

    fn set_hub_claim(&self, session: &SessionId) {
        let detail = serde_json::from_value(serde_json::json!({
            "id": TICKET, "next_child": 1, "title": "Test ticket", "type": "bug",
            "status": "in_progress", "priority": 2, "assignee": session, "session": session,
            "machine": self.ctx.machine, "created_at": self.clock.now(), "created_by": "test",
            "updated_at": self.clock.now(), "version": 1, "edges": [], "children": [],
            "ready": false, "blocked_by": [],
        }))
        .unwrap();
        self.hub.insert_ticket(detail);
    }

    fn try_begin(&self) -> Result<SessionId> {
        self.try_begin_on("claude", MODEL)
    }

    fn reservation(&self, provider: &str) -> Reservation {
        Reservation {
            master_blocker: false,
            provider: Some(provider.to_owned()),
            ticket: TicketId::new(TICKET).unwrap(),
            machine: MachineId::new("m-7f3a").unwrap(),
            session: SessionId::new("reservation-e2e").unwrap(),
            expires_at: self.clock.now().plus_seconds(600.0),
            code: "c0de".to_owned(),
        }
    }

    fn try_begin_on(&self, provider: &str, model: &str) -> Result<SessionId> {
        self.daemon.begin(
            &self.reservation(provider),
            model,
            Some("high"),
            None,
            false,
        )
    }

    /// Blocks until the supervisor of turn `turn` of `session` has exited.
    fn wait_turn(&self, session: &SessionId, turn: u32) {
        find_run(&self.home, session, turn)
            .unwrap_or_else(|| panic!("turn {turn} of {session} has no run"))
            .wait();
    }

    fn slot(&self) -> Slot {
        self.daemon.slots("claude")[0].clone()
    }

    fn ends(&self) -> Vec<TurnEnd> {
        lock(&self.effects.ends).clone()
    }

    fn releases(&self) -> Vec<(ReleaseCause, String)> {
        lock(&self.host.releases).clone()
    }
}

#[test]
fn cli_e2e_teardown_reaps_a_supervisor_whose_turn_is_still_waiting() {
    let rig = Rig::new("teardown");
    rig.script(1, "init; started; hang; result\n");
    rig.fifo(1);
    let session = rig.begin();
    rig.wait_started(1);
    assert_eq!(start::recover(&rig.home, &session, 1), Recovery::Running);

    let supervisors = lock(&rig.supervisors);
    let [started] = supervisors.as_slice() else {
        panic!("expected one supervisor, found {}", supervisors.len());
    };
    let pid = started.supervisor_pid().to_string();
    drop(supervisors);
    drop(rig);

    let output = Command::new("/bin/ps")
        .args(["-p", &pid, "-o", "pid="])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "supervisor {pid} still exists after teardown: {output:?}"
    );
}

/// The placeholder turns of a rig before its first daemon is built.
struct NoTurns;

impl Turns for NoTurns {
    fn start(&self, _session: &SessionId, _turn: u32, _command: &CommandSpec) -> Result<()> {
        Err(Error::Invalid("no turns".into()))
    }
    fn recover(&self, _session: &SessionId, _turn: u32) -> Recovery {
        Recovery::NotStarted
    }
    fn output(&self, _session: &SessionId, _turn: u32) -> Option<String> {
        None
    }
    fn grew_at(&self, _session: &SessionId, _turn: u32) -> Option<Timestamp> {
        None
    }
    fn process(&self, _session: &SessionId, _turn: u32) -> Option<u32> {
        None
    }
    fn end(&self, _session: &SessionId, _turn: u32) -> Result<()> {
        Ok(())
    }
}

#[path = "clid_e2e_cases.rs"]
mod cases;
