//! CLI holds, probe turns, the plan-limit sample, usage samples and turn costs, against a fake
//! CLI app whose scripted turns are written to real `output.log` files.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ignite_core::cli::codex::Codex;
use ignite_core::cli::{HoldReason, Slot, TokenCounts, TurnEvents, UsageSample};
use ignite_core::config::Config;
use ignite_core::fakes::{
    FakeBuilds, FakeCliApp, FakeClock, FakeGit, FakeHub, ScriptedLauncher, ScriptedTurn,
};
use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Launcher, RunResult};
use ignite_store::local::rows::CliSlot;
use ignite_store::local::{cli_slots, plan_usage};

use super::*;
use crate::clid::Effects;
use crate::dispatchd::usage_report;

const START: &str = "2026-11-20T12:00:00Z";

/// A launcher that plays [`ScriptedLauncher`]'s turns and writes each one's lines to a real
/// file, as a supervised run's `output.log` is.
struct FileLauncher {
    scripted: ScriptedLauncher,
    dir: PathBuf,
}

impl Launcher for FileLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        let result = self.scripted.run(spec);
        let text = self.scripted.output(&result.output).unwrap_or_default();
        let path = self.dir.join(format!("{}.log", self.scripted.runs().len()));
        fs::write(&path, text).unwrap();
        RunResult {
            output: path,
            ..result
        }
    }
}

struct Rig {
    dir: PathBuf,
    ctx: Context,
    clock: Arc<FakeClock>,
    hub: Arc<FakeHub>,
    launcher: Arc<FileLauncher>,
    pool: Arc<Pool>,
    health: Health,
    notes: Arc<Mutex<Vec<(String, String)>>>,
    app: FakeCliApp,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn rig(name: &str) -> Rig {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-clid-health-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("runs")).unwrap();
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
        [pool.cli.fake]
        dialect = "claude"
        program = "fake-cli"
        args = ["--yes"]
        [pool.models.fake]
        opus = "fake-opus"
        light = "fake-light"
        [pool.cli.codex]
        program = "fake-codex"
        [pool.models.codex]
        light = "codex-light"
        [pool.cli.agy-gemini]
        dialect = "agy"
        program = "fake-agy"
        [pool.models.agy-gemini]
        sonnet = "gemini-4-neon"
        light = "gemini-3.8-flash"
        [pool.cli.agy-claude]
        dialect = "agy"
        program = "fake-agy"
        [pool.models.agy-claude]
        sonnet = "claude-sonnet-5-5"
    "#;
    let clock = Arc::new(FakeClock::new(Timestamp::parse(START).unwrap()));
    let hub = Arc::new(FakeHub::default());
    let launcher = Arc::new(FileLauncher {
        scripted: ScriptedLauncher::default(),
        dir: dir.join("runs"),
    });
    let ctx = Context {
        config: Config::parse("ignite.toml", text).unwrap(),
        repo: dir.join("checkout"),
        machine: MachineId::new("m-7f3a").unwrap(),
        clock: clock.clone(),
        hub: hub.clone(),
        git: Arc::new(FakeGit::default()),
        launcher: launcher.clone(),
        builds: Arc::new(FakeBuilds::default()),
        session: None,
        tree: None,
    };
    let pool = Arc::new(Pool::new(ignite_store::local::open_in_memory().unwrap()));
    let notes = Arc::new(Mutex::new(Vec::new()));
    let sink = notes.clone();
    let health = Health::new(pool.clone(), dir.join("home"), "jarvis".to_owned()).with_notify(
        Box::new(move |title, text| {
            sink.lock()
                .unwrap()
                .push((title.to_owned(), text.to_owned()));
        }),
    );
    Rig {
        dir,
        ctx,
        clock,
        hub,
        launcher,
        pool,
        health,
        notes,
        app: FakeCliApp::default(),
    }
}

impl Rig {
    /// A second `Health` over the same database and home, as the daemon shares one.
    fn health_clone(&self) -> Health {
        Health::new(
            self.pool.clone(),
            self.dir.join("home"),
            "jarvis".to_owned(),
        )
        .with_notify(Box::new(|_, _| {}))
    }

    fn now(&self) -> Timestamp {
        self.ctx.clock.now()
    }

    fn advance(&self, seconds: f64) {
        self.clock.advance(seconds);
    }

    fn tick(&self) {
        self.health.tick(&self.ctx, &[("fake", &self.app)]);
    }

    fn passing_probe(&self) {
        self.launcher.scripted.push(ScriptedTurn::clean(vec![
            FakeCliApp::init_line("t-probe", "fake-light", "9.1"),
            FakeCliApp::text_line("ready"),
            FakeCliApp::result_line(0.001),
        ]));
    }

    fn failing_probe(&self) {
        self.launcher.scripted.push(ScriptedTurn {
            lines: Vec::new(),
            exit: Some(1),
            hung: false,
        });
    }

    fn put_slot(&self, index: i64, park_reason: Option<String>) {
        let slot = CliSlot {
            app: "fake".to_owned(),
            slot: index,
            session: Some(format!("cli-fake-0000000{index}")),
            ticket: Some(format!("T.{index}")),
            state: "parked".to_owned(),
            turn: 2,
            transcript: Some(format!("t-{index}")),
            model: Some("fake-opus".to_owned()),
            effort: Some("high".to_owned()),
            since: Some(START.to_owned()),
            reservation: None,
            next_prompt: None,
            wait_handle: None,
            resume_at: None,
            park_reason,
            interrupted_in_row: 0,
            block_sent: 0,
            backed_off: 0,
            fallback_prompt: None,
            handoff_note: None,
        };
        self.pool
            .with_database(|conn| cli_slots::put(conn, &slot))
            .unwrap();
    }

    fn slots(&self) -> Vec<CliSlot> {
        self.pool
            .with_database(|conn| cli_slots::list(conn))
            .unwrap()
    }

    fn hold_now(&self) {
        self.health
            .start_hold(
                "fake",
                HoldReason::NotLoggedIn,
                "Invalid API key · Please run /login\nmore detail",
                Some("9.1"),
                self.now(),
            )
            .unwrap();
    }
}

#[test]
fn a_hold_stops_starts_and_offers_and_releases_nothing() {
    let rig = rig("stops");
    rig.passing_probe();
    rig.tick();
    assert!(rig.health.offers("fake"));
    assert!(rig.health.may_start("fake"));

    rig.put_slot(1, None);
    rig.hold_now();
    let reason = hold_park_reason(&rig.health.hold("fake").unwrap());
    rig.put_slot(1, Some(reason));
    // A second failure while held is the same hold: one notification, the first reason.
    rig.health
        .start_hold("fake", HoldReason::ApiError, "overloaded", None, rig.now())
        .unwrap();

    assert!(!rig.health.may_start("fake"));
    assert!(!rig.health.offers("fake"));
    let hold = rig.health.hold("fake").unwrap();
    assert_eq!(hold.reason, HoldReason::NotLoggedIn);
    assert_eq!(hold.message, "Invalid API key · Please run /login");
    assert_eq!(hold.cli_version.as_deref(), Some("9.1"));
    let notes = rig.notes.lock().unwrap().clone();
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].0.contains("fake CLI workers on jarvis"),
        "{notes:?}"
    );
    assert!(notes[0].1.contains("with CLI 9.1"), "{notes:?}");

    // No justiciar report before a probe confirms the hold.
    rig.tick();
    rig.advance(60.0);
    rig.tick();
    assert!(rig.hub.justiciar_reports().is_empty());

    // The slot keeps its ticket and stays parked; no probe ran before 5 minutes.
    let slots = rig.slots();
    assert_eq!(
        (slots[0].ticket.as_deref(), slots[0].state.as_str()),
        (Some("T.1"), "parked")
    );
    // `Health` has no `Host`, through which alone a ticket is released, so it cannot release
    // one; the slot's row is what would show a release.
    assert_eq!(rig.launcher.scripted.runs().len(), 1);
}

#[test]
fn a_hold_the_next_probe_ends_is_never_reported() {
    let rig = rig("transient");
    rig.hold_now();
    rig.tick();
    rig.advance(PROBE_EVERY_S);
    rig.passing_probe();
    rig.tick();
    rig.tick();
    assert!(rig.health.hold("fake").is_none());
    assert!(rig.hub.justiciar_reports().is_empty());
}

#[test]
fn a_successful_probe_ends_the_hold_and_unparks_its_slots() {
    let rig = rig("unparks");
    rig.hold_now();
    let hold = rig.health.hold("fake").unwrap();
    rig.put_slot(1, Some(hold_park_reason(&hold)));
    rig.put_slot(
        2,
        Some("fake is on a CLI hold (not_logged_in) since then, so no turn starts".to_owned()),
    );
    rig.put_slot(3, Some("exit Some(7) that decide cannot place".to_owned()));

    // A failed probe keeps the original hold and parked slots while adding its own cause.
    let parked = rig.slots();
    rig.advance(PROBE_EVERY_S);
    rig.failing_probe();
    rig.tick();
    let updated = rig.health_clone().hold("fake").unwrap();
    assert_eq!(
        updated,
        Hold {
            latest_probe_failure: Some(ProbeFailure {
                reason: HoldReason::NoFirstLine,
                message: "exited 1 before its first line".to_owned(),
            }),
            ..hold
        }
    );
    assert_eq!(rig.slots(), parked);
    assert!(!rig.health.may_start("fake"));
    assert!(!rig.health.offers("fake"));
    assert_eq!(rig.notes.lock().unwrap().len(), 1);
    assert_eq!(rig.launcher.scripted.runs().len(), 1);
    // The failed probe confirms the hold: one justiciar report, however many rounds follow.
    rig.advance(60.0);
    rig.tick();
    let reports = rig.hub.justiciar_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].subject, "cli-hold:fake");
    assert!(reports[0].on_machine);
    assert!(reports[0].details.contains("Invalid API key"));
    rig.advance(PROBE_EVERY_S - 61.0);
    rig.tick();
    assert_eq!(rig.launcher.scripted.runs().len(), 1, "probed too early");

    rig.advance(1.0);
    rig.passing_probe();
    rig.tick();
    assert_eq!(rig.launcher.scripted.runs().len(), 2);
    assert!(rig.health.hold("fake").is_none());
    assert!(rig.health.offers("fake"));

    let slots = rig.slots();
    let now = rig.now().to_string();
    for slot in &slots[..2] {
        assert_eq!(slot.state, "backing_off", "{slot:?}");
        assert_eq!(slot.resume_at.as_deref(), Some(now.as_str()));
        assert_eq!(
            slot.ticket.as_deref(),
            Some(format!("T.{}", slot.slot).as_str())
        );
        assert!(
            slot.next_prompt
                .as_deref()
                .unwrap()
                .contains("ignite status")
        );
        assert!(parked_by_hold(slot.park_reason.as_deref()));
    }
    // A slot parked for a reason other than the hold waits for a person.
    assert_eq!(slots[2].state, "parked");

    // The probe ran the cheapest tier, in the Ignite home, never in the checkout.
    let run = &rig.launcher.scripted.runs()[1];
    assert_eq!(run.cwd, rig.dir.join("home").join("probe"));
    assert_eq!(
        rig.app.commands()[1].model,
        "fake-light",
        "{:?}",
        rig.app.commands()
    );
    assert_eq!(rig.app.commands()[1].prompt, PROBE_PROMPT);
}

#[test]
fn no_slot_is_offered_before_the_first_probe_passes() {
    let rig = rig("first-probe");
    assert!(!rig.health.offers("fake"));
    assert!(rig.health.may_start("fake"));

    rig.failing_probe();
    rig.tick();
    assert!(!rig.health.offers("fake"));
    let hold = rig.health.hold("fake").unwrap();
    assert_eq!(hold.reason, HoldReason::NoFirstLine);

    rig.advance(PROBE_EVERY_S);
    rig.passing_probe();
    rig.tick();
    assert!(rig.health.offers("fake"));
    // Once passed, no further probe runs while the app is not held.
    rig.advance(PROBE_EVERY_S * 4.0);
    rig.tick();
    assert_eq!(rig.launcher.scripted.runs().len(), 2);
}

#[test]
fn a_codex_plan_limit_blocks_only_codex_until_a_probe_passes() {
    let rig = rig("plan-limit");
    let health = Arc::new(rig.health_clone());
    let effects = HealthEffects {
        health: health.clone(),
        ctx: rig.ctx.clone(),
    };
    let apps: [(&str, &dyn CliApp); 2] = [("fake", &rig.app), ("codex", &Codex)];
    let ready = r#"{"type":"turn.completed","usage":{}}"#;
    let limit = r#"{"type":"turn.failed","error":{"message":"You have hit your limit"}}"#;
    rig.passing_probe();
    rig.launcher
        .scripted
        .push(ScriptedTurn::clean(vec![ready.to_owned()]));
    health.tick(&rig.ctx, &apps);
    assert!(health.offers("fake") && health.offers("codex"));

    let slot = Slot {
        app: "codex".to_owned(),
        index: 1,
        session: None,
        ticket: None,
        state: SlotState::Running,
        turn: 1,
        transcript: None,
        model: None,
        effort: None,
        since: None,
    };
    let events = Codex.read(limit);
    let end = Codex.classify(&events, Some(1));
    effects.ended(&slot, &end, &events);

    assert_eq!(health.hold("codex").unwrap().reason, HoldReason::PlanLimit);
    assert!(!health.may_start("codex"));
    assert!(!health.offers("codex"));
    assert!(
        health.offers("fake"),
        "the Claude-dialect provider stays eligible"
    );

    rig.advance(PROBE_EVERY_S);
    rig.launcher.scripted.push(ScriptedTurn {
        lines: vec![limit.to_owned()],
        exit: Some(1),
        hung: false,
    });
    health.tick(&rig.ctx, &apps);
    assert_eq!(rig.launcher.scripted.runs().len(), 3);
    assert!(!health.offers("codex"));
    assert!(health.offers("fake"));
    assert!(
        rig.pool
            .with_database(|conn| plan_usage::latest(conn))
            .unwrap()
            .is_empty()
    );
    assert!(rig.hub.metrics_imports().is_empty());

    rig.advance(PROBE_EVERY_S);
    rig.launcher
        .scripted
        .push(ScriptedTurn::clean(vec![ready.to_owned()]));
    health.tick(&rig.ctx, &apps);
    assert_eq!(rig.launcher.scripted.runs().len(), 4);
    assert!(health.hold("codex").is_none());
    assert!(health.may_start("codex") && health.offers("codex"));
}

#[test]
fn a_seven_day_sample_at_0_81_is_stored_as_weekly_all_models_at_81_percent() {
    let rig = rig("samples");
    let samples = vec![
        UsageSample {
            window: "seven_day".to_owned(),
            utilization: 0.81,
            resets_at: Some("2026-11-24T08:00:00Z".to_owned()),
            status: Some("allowed".to_owned()),
        },
        UsageSample {
            window: "seven_day_opus".to_owned(),
            utilization: 0.5,
            resets_at: None,
            status: Some("allowed_warning".to_owned()),
        },
    ];
    rig.health
        .record_usage(&rig.ctx, "claude", &samples, rig.now())
        .unwrap();
    let rows = rig
        .pool
        .with_database(|conn| plan_usage::latest(conn))
        .unwrap();
    let weekly = rows
        .iter()
        .find(|row| row.label.as_deref() == Some("Weekly · all models"))
        .unwrap();
    assert_eq!(weekly.percent_used, Some(81.0));
    assert_eq!(weekly.note, None);
    let opus = rows
        .iter()
        .find(|row| row.label.as_deref() == Some("Weekly · Opus"))
        .unwrap();
    assert_eq!(opus.note.as_deref(), Some("CLI status allowed_warning"));
    let report = usage_report(&rig.pool, rig.now());
    assert!(
        report.text.contains("Weekly · all models 81%"),
        "{}",
        report.text
    );

    // A second observation can supersede a higher sample from another machine.
    let imports = rig.hub.metrics_imports();
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].source, "plan_usage");
    assert_eq!(imports[0].rows.len(), 2);
    rig.advance(5.0);
    rig.health
        .record_usage(&rig.ctx, "claude", &samples, rig.now())
        .unwrap();
    assert_eq!(rig.hub.metrics_imports().len(), 2);
}

#[test]
fn windows_take_the_labels_get_usage_gives_them() {
    assert_eq!(window_label("seven_day"), "Weekly · all models");
    assert_eq!(window_label("five_hour"), "5-hour");
    assert_eq!(window_label("seven_day_opus"), "Weekly · Opus");
    assert_eq!(window_label("seven_day_fable"), "Weekly · Fable");
    assert_eq!(window_label("overage"), "overage");
}

#[test]
fn a_turns_cost_goes_to_the_hub_through_the_outbox() {
    let rig = rig("cost");
    let session = SessionId::new("cli-fake-0a1b2c3d").unwrap();
    let ticket = TicketId::new("T.9").unwrap();
    let events = TurnEvents {
        model: Some("fake-opus".to_owned()),
        cost_usd: Some(0.42),
        tokens: Some(TokenCounts {
            input: 100,
            output: 20,
            cache_read: 3000,
            cache_write: 400,
        }),
        result_seen: true,
        ..TurnEvents::default()
    };
    record_turn(
        &rig.pool,
        &rig.ctx,
        "fake",
        &session,
        Some(&ticket),
        3,
        &events,
    )
    .unwrap();
    // A turn with no result has nothing to send.
    record_turn(
        &rig.pool,
        &rig.ctx,
        "fake",
        &session,
        Some(&ticket),
        4,
        &TurnEvents::default(),
    )
    .unwrap();
    assert!(rig.hub.metrics_imports().is_empty());

    let sent = rig
        .pool
        .with_database(|conn| crate::outbox::flush(conn, rig.hub.as_ref()))
        .unwrap();
    assert_eq!(sent, 1);
    let imports = rig.hub.metrics_imports();
    assert_eq!(imports[0].source, TURN_SOURCE);
    let row = &imports[0].rows[0];
    assert_eq!(row["ticket"], "T.9");
    assert_eq!(row["turn"], 3);
    assert_eq!(row["cost_usd"], 0.42);
    assert_eq!(row["cache_read_tokens"], 3000);
}

#[test]
fn the_loops_effects_reach_health() {
    let rig = rig("effects");
    let health = Arc::new(rig.health_clone());
    let effects = HealthEffects {
        health: health.clone(),
        ctx: rig.ctx.clone(),
    };
    let slot = ignite_core::cli::Slot {
        app: "fake".to_owned(),
        index: 1,
        session: Some(SessionId::new("cli-fake-0a1b2c3d").unwrap()),
        ticket: Some(TicketId::new("T.1").unwrap()),
        state: ignite_core::cli::SlotState::Running,
        turn: 2,
        transcript: Some("t-1".to_owned()),
        model: Some("fake-opus".to_owned()),
        effort: None,
        since: None,
    };
    assert!(!effects.probed("fake"));

    effects.usage(
        &slot,
        &[UsageSample {
            window: "seven_day".to_owned(),
            utilization: 0.81,
            resets_at: None,
            status: None,
        }],
    );
    let events = TurnEvents {
        cost_usd: Some(0.1),
        result_seen: true,
        result_error: true,
        ..TurnEvents::default()
    };
    effects.ended(
        &slot,
        &ignite_core::cli::TurnEnd::PlanLimit {
            window: Some("seven_day".to_owned()),
        },
        &events,
    );
    let rows = rig
        .pool
        .with_database(|conn| plan_usage::latest(conn))
        .unwrap();
    assert_eq!(rows[0].provider, "fake");
    assert_eq!(rows[0].label.as_deref(), Some("Weekly · all models"));
    assert_eq!(rows[0].percent_used, Some(100.0));
    assert!(health.hold("fake").is_none());
    let queued = rig
        .pool
        .with_database(|conn| crate::outbox::flush(conn, rig.hub.as_ref()))
        .unwrap();
    assert_eq!(queued, 1, "the turn's cost was queued");

    effects.hold(
        "fake",
        HoldReason::ModelRefused,
        "model not supported",
        Some("0.154.0"),
    );
    assert_eq!(
        health.hold("fake").unwrap().reason,
        HoldReason::ModelRefused
    );

    rig.advance(PROBE_EVERY_S);
    rig.passing_probe();
    health.tick(&rig.ctx, &[("fake", &rig.app)]);
    assert!(effects.probed("fake"));
    assert!(health.offers("fake"));
}

#[test]
fn agy_probe_and_plan_limit_record_separate_gemini_and_claude_usage() {
    use ignite_core::cli::agy::Agy;

    let rig = rig("agy-usage");
    // Probe turn + /usage call for agy-gemini.
    rig.launcher.scripted.push(ScriptedTurn::clean(vec![
        r#"{"event":"init","conversation_id":"c-1","init":{"model":"gemini-3.8-flash"}}"#.to_owned(),
        r#"{"event":"result","result":{"conversation_id":"c-1","status":"SUCCESS","response":"ready"}}"#
            .to_owned(),
    ]));
    let usage_json = r#"{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[{"name":"Gemini Models","buckets":[{"id":"gemini-weekly","window":"weekly","remaining_fraction":0.8,"reset_time":"2026-11-27T00:00:00Z"},{"id":"gemini-5h","window":"5h","remaining_fraction":0.5,"reset_time":"2026-11-20T17:00:00Z"}]},{"name":"Claude and GPT models","buckets":[{"id":"3p-weekly","window":"weekly","remaining_fraction":0.9,"reset_time":"2026-11-27T01:00:00Z"},{"id":"3p-5h","window":"5h","remaining_fraction":0.0,"reset_time":"2026-11-20T16:00:00Z"}]}]}}}"#;
    rig.launcher
        .scripted
        .push(ScriptedTurn::clean(vec![usage_json.to_owned()]));

    rig.health.tick(&rig.ctx, &[("agy-gemini", &Agy)]);
    assert!(rig.health.offers("agy-gemini"));
    let rows = rig
        .pool
        .with_database(|conn| plan_usage::latest(conn))
        .unwrap();
    let find = |provider: &str, label: &str| {
        rows.iter()
            .find(|r| r.provider == provider && r.label.as_deref() == Some(label))
            .cloned()
            .unwrap()
    };
    assert_eq!(
        find("agy-gemini", "Weekly · all models").percent_used,
        Some(20.0)
    );
    assert_eq!(find("agy-gemini", "5-hour").percent_used, Some(50.0));
    assert_eq!(
        find("agy-claude", "Weekly · all models").percent_used,
        Some(10.0)
    );
    assert_eq!(find("agy-claude", "5-hour").percent_used, Some(100.0));

    // A PlanLimit end for agy-claude refreshes /usage and records a 100% limit sample
    // with the exact reset_time, without placing agy-claude on a local CLI hold.
    rig.launcher
        .scripted
        .push(ScriptedTurn::clean(vec![usage_json.to_owned()]));
    let health = Arc::new(rig.health_clone());
    let effects = HealthEffects {
        health: health.clone(),
        ctx: rig.ctx.clone(),
    };
    let slot = ignite_core::cli::Slot {
        app: "agy-claude".to_owned(),
        index: 1,
        session: Some(SessionId::new("cli-agy-claude-0a1b2c3d").unwrap()),
        ticket: Some(TicketId::new("T.2").unwrap()),
        state: ignite_core::cli::SlotState::Running,
        turn: 1,
        transcript: Some("c-2".to_owned()),
        model: Some("claude-sonnet-5-5".to_owned()),
        effort: None,
        since: None,
    };
    effects.ended(
        &slot,
        &ignite_core::cli::TurnEnd::PlanLimit { window: None },
        &TurnEvents::default(),
    );
    assert!(health.hold("agy-claude").is_none());
    let rows = rig
        .pool
        .with_database(|conn| plan_usage::latest(conn))
        .unwrap();
    let limit = rows
        .iter()
        .find(|r| r.provider == "agy-claude" && r.label.as_deref() == Some("5-hour"))
        .unwrap();
    assert_eq!(limit.status, "limit");
    assert_eq!(limit.percent_used, Some(100.0));
    assert_eq!(limit.resets_at.as_deref(), Some("2026-11-20T16:00:00Z"));
}
