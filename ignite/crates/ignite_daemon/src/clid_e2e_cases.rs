//! The end-to-end cases of CLI mode (see the parent module for the rig and the scripted
//! program).

use super::*;

/// A turn's first line and then a wait for the gate, with the FIFO told the turn runs.
const HELD: &str = "init; started; hang; say done; result\n";

/// Every turn of `session` and how each ended, in order.
fn turns_of(rig: &Rig, session: &SessionId) -> Vec<(i64, String, Option<String>)> {
    let conn = Rig::open(&rig.home);
    store::turns(&conn, session.as_str())
        .unwrap()
        .into_iter()
        .map(|t| (t.turn, t.state, t.end_kind))
        .collect()
}

#[test]
fn cli_e2e_a_daemon_restart_during_a_turn_neither_starts_it_again_nor_ends_it_twice() {
    let mut rig = Rig::new("restart");
    rig.script(1, HELD);
    rig.script(2, HELD);
    rig.fifo(1);
    rig.fifo(2);
    let session = rig.begin();
    rig.wait_started(1);

    rig.restart();
    rig.daemon.recover().unwrap();
    rig.daemon.step().unwrap();
    assert_eq!(
        rig.invocations(),
        1,
        "the running turn was not started again"
    );
    assert_eq!(rig.slot().state, SlotState::Running);

    rig.open_gate(1);
    rig.wait_turn(&session, 1);
    rig.restart();
    rig.daemon.step().unwrap();
    rig.wait_started(2);
    rig.daemon.step().unwrap();
    assert_eq!(rig.ends(), vec![TurnEnd::Clean], "the end was applied once");
    assert_eq!(rig.invocations(), 2);
    assert_eq!(rig.prompt(2), block_text(&TicketId::new(TICKET).unwrap()));
    let turns = turns_of(&rig, &session);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].1, "ended");
}

#[test]
fn cli_e2e_a_program_killed_mid_turn_resumes_its_transcript_as_interrupted() {
    let rig = Rig::new("killed");
    // The program ends itself with SIGKILL after its first line; its supervisor survives.
    rig.script(1, "init; kill -9 $$\n");
    let session = rig.begin();
    rig.wait_turn(&session, 1);
    rig.daemon.step().unwrap();
    rig.wait_turn(&session, 2);

    assert_eq!(rig.ends(), vec![TurnEnd::Interrupted]);
    assert_eq!(rig.invocations(), 2);
    let first = rig.args(1);
    let transcript = &first[first.iter().position(|a| a == "--session-id").unwrap() + 1];
    let second = rig.args(2);
    let resumed = second.iter().position(|a| a == "--resume").unwrap();
    assert_eq!(&second[resumed + 1], transcript);
    assert_eq!(rig.prompt(2), INTERRUPTED_TEXT);
}

#[test]
fn cli_e2e_a_reboot_that_left_no_supervisor_and_no_result_resumes_as_interrupted() {
    let mut rig = Rig::new("reboot");
    // The program kills its supervisor, as a reboot ends it, and goes on running.
    rig.script(1, "init; kill -9 $PPID; hang\n");
    let session = rig.begin();
    rig.wait_turn(&session, 1);
    let run = find_run(&rig.home, &session, 1).unwrap();
    assert!(
        !run.dir().join("result.json").exists(),
        "the run wrote no result"
    );

    rig.restart();
    rig.daemon.recover().unwrap();
    rig.daemon.step().unwrap();
    rig.wait_turn(&session, 2);
    assert_eq!(rig.ends(), vec![TurnEnd::Interrupted]);
    assert_eq!(rig.invocations(), 2);
    assert!(rig.args(2).contains(&"--resume".to_owned()));
    assert_eq!(rig.prompt(2), INTERRUPTED_TEXT);
}

#[test]
fn cli_e2e_a_crash_between_recording_a_turn_and_starting_it_runs_it_once() {
    let mut rig = Rig::new("recorded");
    rig.restart_with(Arc::new(FailFirst {
        inner: rig.turns(),
        failed: AtomicBool::new(false),
    }));
    // The session exists once its turn is recorded, so the hand-off succeeds and the
    // dispatcher adds the session to the pool; the turn starts later.
    rig.try_begin().unwrap();
    assert_eq!(rig.invocations(), 0);
    let session = rig.slot().session.unwrap();
    assert_eq!(rig.slot().state, SlotState::Starting);

    rig.restart();
    assert_eq!(rig.daemon.recover().unwrap(), 1);
    rig.wait_turn(&session, 1);
    rig.daemon.recover().unwrap();
    assert_eq!(rig.invocations(), 1, "the recorded turn ran once");
    assert!(
        rig.prompt(1)
            .contains("ignite start T.1 --reservation c0de")
    );
}

#[test]
fn cli_e2e_recover_and_the_loop_starting_one_turn_at_once_run_it_once() {
    let mut rig = Rig::new("race");
    // The turn waits for its gate, so the loop cannot see it end and start the next one.
    rig.script(1, "init; hang; say done; result\n");
    rig.restart_with(Arc::new(FailFirst {
        inner: rig.turns(),
        failed: AtomicBool::new(false),
    }));
    let _ = rig.try_begin();
    let session = rig.slot().session.unwrap();
    lock(&rig.host.holders).insert(session.clone(), TicketId::new(TICKET).unwrap());

    rig.restart();
    std::thread::scope(|scope| {
        let daemon = &rig.daemon;
        let recovering = scope.spawn(move || daemon.recover());
        let stepping = scope.spawn(move || daemon.step());
        recovering.join().unwrap().unwrap();
        stepping.join().unwrap().unwrap();
    });
    rig.open_gate(1);
    rig.wait_turn(&session, 1);
    assert_eq!(rig.invocations(), 1, "one invocation of the program");
    rig.daemon.step().unwrap();
    assert_eq!(rig.ends(), vec![TurnEnd::Clean]);
}

#[test]
fn cli_e2e_a_row_an_older_build_wrote_is_read_and_continued() {
    let rig = Rig::new("older");
    // The columns of the first schema that had CLI slots, without any added since.
    let conn = Rig::open(&rig.home);
    conn.execute_batch(
        "INSERT INTO cli_slots (app, slot, session, ticket, state, turn, model) \
         VALUES ('claude', 1, 'cli-claude-0a1b2c3d', 'T.1', 'starting', 1, 'claude-test-1');
         INSERT INTO cli_turns (session, turn, app, slot, ticket, prompt) \
         VALUES ('cli-claude-0a1b2c3d', 1, 'claude', 1, 'T.1', 'Run the ticket.');",
    )
    .unwrap();
    let session = SessionId::new("cli-claude-0a1b2c3d").unwrap();
    lock(&rig.host.holders).insert(session.clone(), TicketId::new(TICKET).unwrap());

    assert_eq!(rig.daemon.recover().unwrap(), 1);
    rig.wait_turn(&session, 1);
    rig.daemon.step().unwrap();
    rig.wait_turn(&session, 2);
    assert_eq!(rig.prompt(1), "Run the ticket.");
    assert!(rig.args(1).contains(&"--session-id".to_owned()));
    assert_eq!(rig.ends(), vec![TurnEnd::Clean]);
    assert_eq!(rig.invocations(), 2, "the slot went on to its next turn");
    assert!(rig.args(2).contains(&"--resume".to_owned()));
}

#[test]
fn cli_e2e_a_plan_limit_releases_the_ticket_and_starts_the_usage_hold() {
    let rig = Rig::new("limit");
    let resets = START / 1000 + 3600;
    rig.script(
        1,
        &format!(
            "init\necho '{{\"type\":\"rate_limit_event\",\"rate_limit_info\":{{\"unifiedWindows\":\
             {{\"five_hour\":{{\"utilization\":1.0,\"resetsAt\":{resets},\"status\":\"rejected\"\
             }}}}}}}}'\necho '{{\"type\":\"result\",\"subtype\":\"error_during_execution\",\
             \"is_error\":true,\"result\":\"You have hit your limit\"}}'\nexit 1\n"
        ),
    );
    let session = rig.begin();
    rig.wait_turn(&session, 1);
    rig.daemon.step().unwrap();

    assert_eq!(
        rig.ends(),
        vec![TurnEnd::PlanLimit {
            window: Some("five_hour".to_owned())
        }]
    );
    let releases = rig.releases();
    assert_eq!(releases.len(), 1);
    assert!(releases[0].contains("plan's limit"), "{}", releases[0]);
    assert_eq!(rig.slot().state, SlotState::Free);
    assert_eq!(rig.invocations(), 1);
    let pool = Pool::new(Rig::open(&rig.home));
    let rows = pool
        .with_database(|conn| ignite_store::local::plan_usage::latest(conn))
        .unwrap();
    assert_eq!(rows[0].provider, "claude");
    assert_eq!(rows[0].percent_used, Some(100.0));
}

#[test]
fn cli_e2e_a_refusal_holds_the_app_and_a_passing_probe_resumes_the_slot() {
    let rig = Rig::new("refusal");
    rig.script(
        1,
        "init\necho '{\"type\":\"result\",\"subtype\":\"error_during_execution\",\
         \"is_error\":true,\"api_error_status\":404,\"result\":\"There is an issue with the \
         selected model\"}'\nexit 1\n",
    );
    let session = rig.begin();
    rig.wait_turn(&session, 1);
    rig.daemon.step().unwrap();

    assert!(matches!(
        rig.ends()[..],
        [TurnEnd::Hold {
            reason: HoldReason::ModelRefused,
            ..
        }]
    ));
    assert_eq!(rig.slot().state, SlotState::Parked);
    assert!(rig.releases().is_empty(), "a hold releases no ticket");
    assert!(rig.daemon.held("claude").is_some());
    rig.daemon.step().unwrap();
    assert_eq!(rig.invocations(), 1, "no turn starts during the hold");

    // The probe is due five minutes into the hold; it runs as invocation 2 and passes.
    rig.clock.advance(super::super::health::PROBE_EVERY_S + 1.0);
    let claude: &dyn CliApp = &Claude;
    rig.health.tick(&rig.ctx, &[claude]);
    assert_eq!(rig.invocations(), 2);
    assert!(rig.args(2).contains(&MODEL.to_owned()));
    assert!(rig.daemon.held("claude").is_none());

    rig.hub.set_unreachable(true);
    assert!(matches!(rig.daemon.step(), Err(Error::Unreachable(_))));
    assert_eq!(rig.slot().state, SlotState::BackingOff);
    assert!(find_run(&rig.home, &session, 2).is_none());
    rig.hub.set_unreachable(false);

    rig.daemon.step().unwrap();
    rig.wait_turn(&session, 2);
    assert_eq!(rig.invocations(), 3, "the parked slot resumed");
    assert!(rig.args(3).contains(&"--resume".to_owned()));
    assert!(
        rig.prompt(3).contains("works again now"),
        "{}",
        rig.prompt(3)
    );
    assert!(rig.releases().is_empty());
}

#[test]
fn cli_e2e_a_passing_probe_retires_a_slot_reassigned_at_the_hub() {
    let rig = Rig::new("reassigned");
    rig.script(1, "exit 1\n");
    let session = rig.begin();
    rig.wait_turn(&session, 1);
    rig.daemon.step().unwrap();
    assert_eq!(rig.slot().state, SlotState::Parked);
    assert!(rig.daemon.held("claude").is_some());

    // The hub reassigned the ticket while the old worker's local tree still shows it leased.
    let new_holder = SessionId::new("cli-claude-replacement").unwrap();
    rig.set_hub_claim(&new_holder);
    assert_eq!(
        rig.host.holders().get(&session),
        Some(&TicketId::new(TICKET).unwrap())
    );
    rig.clock.advance(super::super::health::PROBE_EVERY_S + 1.0);
    rig.health.tick(&rig.ctx, &[&Claude]);
    assert_eq!(rig.invocations(), 2);
    assert!(rig.daemon.held("claude").is_none());

    rig.daemon.step().unwrap();
    assert_eq!(rig.daemon.check(&session), SessionCheck::Gone);
    assert_eq!(rig.slot().state, SlotState::Free);
    assert!(find_run(&rig.home, &session, 2).is_none());
    assert!(rig.releases().is_empty(), "the new holder keeps its claim");
}

#[test]
fn cli_e2e_a_restarted_daemon_rechecks_the_claim_before_launching_a_recorded_resume() {
    let mut rig = Rig::new("reassigned-after-recording");
    rig.script(1, "exit 1\n");
    let session = rig.begin();
    rig.wait_turn(&session, 1);
    rig.daemon.step().unwrap();
    assert_eq!(rig.slot().state, SlotState::Parked);

    rig.clock.advance(super::super::health::PROBE_EVERY_S + 1.0);
    rig.health.tick(&rig.ctx, &[&Claude]);
    rig.restart_with(Arc::new(FailFirst {
        inner: rig.turns(),
        failed: AtomicBool::new(false),
    }));
    assert!(rig.daemon.step().is_err());
    assert_eq!(rig.slot().state, SlotState::Starting);

    rig.set_hub_claim(&SessionId::new("cli-claude-replacement").unwrap());
    rig.restart();
    rig.daemon.step().unwrap();
    assert_eq!(rig.slot().state, SlotState::Free);
    assert!(find_run(&rig.home, &session, 2).is_none());
    assert_eq!(
        rig.daemon.turn_row(&session, 2).unwrap().unwrap().state,
        "ended"
    );
}

#[test]
fn cli_e2e_tell_now_ends_the_running_turn_and_the_next_prompt_holds_the_text() {
    let rig = Rig::new("tell");
    rig.script(1, HELD);
    rig.fifo(1);
    let session = rig.begin();
    rig.wait_started(1);
    let text = "Message about T.1: stop and land what you have.";
    rig.daemon.message(&session, text, true).unwrap();
    rig.wait_turn(&session, 1);
    rig.daemon.step().unwrap();
    rig.wait_turn(&session, 2);

    assert_eq!(rig.ends(), vec![TurnEnd::Interrupted]);
    assert_eq!(rig.invocations(), 2);
    let prompt = rig.prompt(2);
    assert!(prompt.starts_with(INTERRUPTED_TEXT), "{prompt}");
    assert_eq!(prompt.matches(text).count(), 1, "{prompt}");
    let conn = Rig::open(&rig.home);
    assert!(
        store::undelivered(&conn, session.as_str())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn cli_e2e_a_clean_end_holding_the_ticket_gets_the_block_text_once_then_releases() {
    let rig = Rig::new("block");
    rig.script(2, "init; say 'I stopped for the night'; result\n");
    let session = rig.begin();
    rig.wait_turn(&session, 1);
    rig.daemon.step().unwrap();
    rig.wait_turn(&session, 2);
    assert_eq!(rig.invocations(), 2);
    assert_eq!(rig.prompt(2), block_text(&TicketId::new(TICKET).unwrap()));
    assert!(rig.releases().is_empty());

    rig.daemon.step().unwrap();
    let releases = rig.releases();
    assert_eq!(releases.len(), 1);
    assert!(
        releases[0].contains("I stopped for the night"),
        "{}",
        releases[0]
    );
    assert_eq!(rig.slot().state, SlotState::Free);
    assert_eq!(rig.invocations(), 2);
}

#[test]
fn cli_e2e_a_hand_off_begun_again_after_a_restart_starts_no_second_turn() {
    let mut rig = Rig::new("rebegin");
    rig.script(1, HELD);
    rig.fifo(1);
    let session = rig.begin();
    rig.wait_started(1);

    // The dispatcher saves a hand-off before `begin` and its new session after it, so a daemon
    // that died in between asks again for the same reservation.
    rig.restart();
    rig.daemon.recover().unwrap();
    assert_eq!(rig.try_begin().unwrap(), session);
    assert_eq!(rig.invocations(), 1, "no second turn started");
    assert_eq!(rig.daemon.slots("claude").len(), 1);

    rig.open_gate(1);
    rig.wait_turn(&session, 1);
    assert_eq!(turns_of(&rig, &session).len(), 1);
}
