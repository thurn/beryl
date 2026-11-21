//! CLI holds, probe turns, the plan-limit sample, usage samples and turn costs, against a fake
//! CLI app whose scripted turns are written to real `output.log` files.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ignite_core::cli::{HoldReason, TokenCounts, TurnEvents, UsageSample};
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
        program = "fake-cli"
        args = ["--yes"]
        [pool.models.fake]
        opus = "fake-opus"
        light = "fake-light"
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
        self.health.tick(&self.ctx, &[&self.app]);
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

    // One justiciar report per hold, however many rounds run while it lasts.
    rig.tick();
    rig.advance(60.0);
    rig.tick();
    let reports = rig.hub.justiciar_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].subject, "cli-hold:fake");
    assert!(reports[0].on_machine);
    assert!(reports[0].details.contains("Invalid API key"));

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

    // A failing probe five minutes in keeps the hold and records the probe's time.
    rig.advance(PROBE_EVERY_S);
    rig.failing_probe();
    rig.tick();
    assert!(rig.health.hold("fake").is_some());
    assert_eq!(rig.launcher.scripted.runs().len(), 1);
    rig.advance(PROBE_EVERY_S - 1.0);
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
fn a_codex_plan_limit_stores_a_codex_sample_and_uses_no_claude_reset() {
    let rig = rig("plan-limit");
    let resets = rig.now().plus_seconds(7200.0).to_string();
    let samples = [UsageSample {
        window: "five_hour".to_owned(),
        utilization: 0.4,
        resets_at: Some(resets.clone()),
        status: None,
    }];
    rig.health
        .record_usage(&rig.ctx, "claude", &samples, rig.now())
        .unwrap();
    let effects = HealthEffects {
        health: Arc::new(Health::new(
            rig.pool.clone(),
            rig.dir.join("home"),
            "jarvis".to_owned(),
        )),
        ctx: rig.ctx.clone(),
    };
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
    effects.ended(
        &slot,
        &TurnEnd::PlanLimit { window: None },
        &TurnEvents::default(),
    );

    let rows = rig
        .pool
        .with_database(|conn| plan_usage::latest(conn))
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].provider, "claude");
    assert_eq!(rows[0].resets_at.as_deref(), Some(resets.as_str()));
    assert_eq!(rows[0].percent_used, Some(40.0));
    assert_eq!(rows[1].provider, "codex");
    assert_eq!(rows[1].label.as_deref(), Some("5-hour"));
    assert_eq!(rows[1].percent_used, Some(100.0));
    assert_eq!(
        rows[1].resets_at,
        Some(rig.now().plus_seconds(UNKNOWN_RESET_S).to_string())
    );
    assert_eq!(rig.hub.metrics_imports()[1].rows[0]["provider"], "codex");
    let report = usage_report(&rig.pool, rig.now()).text;
    assert!(report.contains("claude · 5-hour 40%"), "{report}");
    assert!(report.contains("codex · 5-hour 100%"), "{report}");
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
    health.tick(&rig.ctx, &[&rig.app]);
    assert!(effects.probed("fake"));
    assert!(health.offers("fake"));
}
