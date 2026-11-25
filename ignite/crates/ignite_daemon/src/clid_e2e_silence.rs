//! End-to-end cases of a turn whose CLI stops writing to its stream while its process stays
//! alive, as an `agy` turn did for eight hours after its provider refused requests for quota.
//! The rig's host cannot list processes, so these turns end at the longer hang limit.

use super::*;
use program::Step::{Init, Result as Done, Wait};

use super::read::{HANG_ANYWAY_S, HANG_S};

/// Starts a ticket whose first turn prints its first line and then blocks, and has the daemon
/// look at it once, so the silence is timed from now.
fn silent_turn(rig: &Rig) -> SessionId {
    rig.script(1, &[Init, Wait, Done]);
    let session = rig.begin();
    rig.wait_started(1);
    rig.daemon.step().unwrap();
    assert_eq!(rig.daemon.check(&session), SessionCheck::Running);
    session
}

/// The turn was ended, its ticket released, its slot freed and no other turn started.
fn assert_released(rig: &Rig, session: &SessionId) {
    rig.wait_turn(session, 1);
    assert_eq!(rig.ends(), [TurnEnd::Interrupted]);
    let releases = rig.releases();
    assert_eq!(releases.len(), 1, "{releases:?}");
    assert_eq!(releases[0].0, ReleaseCause::TakenBack);
    assert!(releases[0].1.contains("wrote nothing"), "{}", releases[0].1);
    assert!(!rig.host.holders().contains_key(session));
    assert_eq!(rig.daemon.check(session), SessionCheck::Gone);
    rig.daemon.step().unwrap();
    assert_eq!(rig.invocations(), 1, "no turn follows a hung one");
}

#[test]
fn cli_e2e_a_silent_turn_whose_process_lives_is_ended_and_its_ticket_released() {
    let rig = Rig::new("silent");
    let session = silent_turn(&rig);

    // The process list cannot be read, so the shorter limit leaves the turn alone.
    rig.clock.advance(HANG_S);
    rig.daemon.step().unwrap();
    assert_eq!(rig.daemon.check(&session), SessionCheck::Running);
    assert!(rig.releases().is_empty());

    rig.clock.advance(HANG_ANYWAY_S - HANG_S);
    rig.daemon.step().unwrap();
    assert_released(&rig, &session);
}

#[test]
fn cli_e2e_a_usage_hold_does_not_keep_a_silent_turn_or_its_ticket() {
    let rig = Rig::new("silent-hold");
    let session = silent_turn(&rig);
    rig.effects.hold(
        "claude",
        HoldReason::PlanLimit,
        "the five-hour window is used up",
        None,
    );
    assert!(rig.daemon.held("claude").is_some());

    rig.clock.advance(HANG_ANYWAY_S);
    rig.daemon.step().unwrap();
    assert!(rig.daemon.held("claude").is_some());
    assert_released(&rig, &session);

    // The hold lifts later with no slot of the session left to resume.
    assert_eq!(rig.health.end_hold("claude", rig.clock.now()).unwrap(), 0);
    rig.daemon.step().unwrap();
    assert_eq!(rig.invocations(), 1);
}

#[test]
fn cli_e2e_a_silent_turn_is_ended_once_a_hold_that_lifted_and_the_limit_have_passed() {
    let rig = Rig::new("silent-lifted");
    let session = silent_turn(&rig);
    rig.effects.hold(
        "claude",
        HoldReason::PlanLimit,
        "the five-hour window is used up",
        None,
    );
    rig.clock.advance(HANG_S);
    rig.daemon.step().unwrap();
    assert_eq!(rig.daemon.check(&session), SessionCheck::Running);

    rig.health.end_hold("claude", rig.clock.now()).unwrap();
    assert!(rig.daemon.held("claude").is_none());
    rig.clock.advance(HANG_ANYWAY_S - HANG_S);
    rig.daemon.step().unwrap();
    assert_released(&rig, &session);
}
