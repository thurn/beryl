//! What `stuck::tip_waiting` and `stuck::tips_waiting` file for platform and family waits.

use ignite_core::protocol::tickets::Status;
use ignite_core::traits::Clock;
use ignite_store::hub;

use super::tests::{all_tickets, start, ticket};

#[test]
fn a_tip_waiting_on_a_family_no_machine_runs_files_a_ticket_naming_it() {
    use super::filing::UPKEEP;
    use super::stuck;
    use ignite_core::config::Os;
    use ignite_core::ids::{CheckName, Commit, MachineId};
    use ignite_core::protocol::hub::TipWaiting;
    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let filed = stuck::tip_waiting(
        &mut conn,
        UPKEEP,
        &TipWaiting {
            commit: Commit::new("abcd").unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            platform: Os::Windows,
            checks: vec![CheckName::new("ignite-platform:wrap").unwrap()],
            waited_s: 0.0,
            unrunnable: vec!["ignite-platform".to_owned()],
        },
        clock.now(),
    )
    .unwrap();
    assert_eq!(filed.len(), 1);
    let filed = ticket(&conn, &filed[0].ticket);
    assert_eq!(filed.status, Status::Open);
    assert_eq!(filed.priority, 1);
    assert!(
        filed.title.contains("windows") && filed.title.contains("ignite-platform"),
        "{}",
        filed.title
    );
    assert!(filed.body.contains("families"), "{}", filed.body);
}

#[test]
fn upkeep_skips_waiting_tickets_for_an_off_platform() {
    use super::{filing::UPKEEP, stuck};
    use ignite_core::config::Os;
    use ignite_core::fleet::MachineState;
    use ignite_core::ids::{CheckName, Commit, MachineId};
    use ignite_core::protocol::hub::{MachineRegister, TipWaiting};

    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    hub::fleet::register(
        &conn,
        &MachineRegister {
            machine: MachineId::new("m-w").unwrap(),
            name: "cygnus".to_owned(),
            os: Os::Windows,
            cores: 4,
            memory_mb: 4096,
            families: Vec::new(),
            version: "abc1234".to_owned(),
        },
        clock.now(),
    )
    .unwrap();
    hub::fleet_settings::set_machine_state(
        &mut conn,
        "m-w",
        MachineState::Off,
        "operator",
        clock.now(),
    )
    .unwrap();

    clock.advance(60.0);
    let tip = Commit::new("abcd").unwrap();
    hub::tips::seen(&conn, tip.as_str(), clock.now()).unwrap();
    clock.advance(3_700.0);
    let waiting = TipWaiting {
        commit: tip,
        machine: MachineId::new("m-m").unwrap(),
        platform: Os::Windows,
        checks: vec![CheckName::new("app:smoke").unwrap()],
        waited_s: 3_700.0,
        unrunnable: Vec::new(),
    };

    assert!(
        stuck::tips_waiting(&mut conn, clock.now())
            .unwrap()
            .is_empty()
    );
    assert!(
        stuck::tip_waiting(&mut conn, UPKEEP, &waiting, clock.now())
            .unwrap()
            .is_empty()
    );
    assert!(all_tickets(&conn, clock.now()).is_empty());
}

#[test]
fn a_wait_report_requires_work_matching_its_platform_and_scope() {
    use super::{filing::UPKEEP, stuck};
    use ignite_core::config::Os;
    use ignite_core::ids::{CheckName, Commit, InputHash, MachineId, RecordId};
    use ignite_core::protocol::Outcome;
    use ignite_core::protocol::hub::{
        MachineRegister, NewResult, ResultCost, ResultKey, TipWaiting,
    };

    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let windows = MachineId::new("m-w").unwrap();
    let mac = MachineId::new("m-m").unwrap();
    for (id, os) in [(&windows, Os::Windows), (&mac, Os::Macos)] {
        hub::fleet::register(
            &conn,
            &MachineRegister {
                machine: id.clone(),
                name: id.to_string(),
                os,
                cores: 4,
                memory_mb: 4096,
                families: vec!["platform".to_owned()],
                version: "v".to_owned(),
            },
            clock.now(),
        )
        .unwrap();
    }
    let result = |check, platform| NewResult {
        key: ResultKey {
            check: CheckName::new(check).unwrap(),
            hash: InputHash::new("a".repeat(64)).unwrap(),
            platform,
        },
        outcome: Outcome::Pass,
        machine: windows.clone(),
        record: RecordId(1),
        output: None,
        cost: ResultCost::default(),
        areas: Vec::new(),
    };
    let waiting = |name, commit| TipWaiting {
        commit: Commit::new(commit).unwrap(),
        machine: mac.clone(),
        platform: Os::Windows,
        checks: vec![CheckName::new(name).unwrap()],
        waited_s: 3_700.0,
        unrunnable: Vec::new(),
    };
    let at = clock.now().to_string();
    hub::results::store(
        &mut conn,
        &[
            result("platform:wrap", None),
            result("platform:other", Some(Os::Windows)),
        ],
        clock.now(),
    )
    .unwrap();
    hub::listings::put(&conn, "unrelated", "other", "[]", windows.as_str(), &at).unwrap();
    hub::listings::put(&conn, "platform", "mac", "[]", mac.as_str(), &at).unwrap();

    let listing = waiting("platform:listing", "abcd");
    let check = waiting("platform:wrap", "abcd");
    let first = stuck::tip_waiting(&mut conn, UPKEEP, &listing, clock.now()).unwrap();
    assert_eq!(first.len(), 1);
    // A second report for the same tip must update the ticket when it names different work.
    let second = stuck::tip_waiting(&mut conn, UPKEEP, &check, clock.now()).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].ticket, second[0].ticket);
    assert_eq!(second[0].change, super::filing::Change::Commented);
    let duplicate = stuck::tip_waiting(&mut conn, UPKEEP, &check, clock.now()).unwrap();
    assert_eq!(duplicate[0].change, super::filing::Change::Unchanged);

    hub::listings::put(&conn, "platform", "win", "[]", windows.as_str(), &at).unwrap();
    assert!(
        stuck::tip_waiting(&mut conn, UPKEEP, &listing, clock.now())
            .unwrap()
            .is_empty()
    );
    hub::results::store(
        &mut conn,
        &[result("platform:wrap", Some(Os::Windows))],
        clock.now(),
    )
    .unwrap();
    assert!(
        stuck::tip_waiting(&mut conn, UPKEEP, &check, clock.now())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_slowly_progressing_platform_wait_escalates_past_the_age_bound() {
    use super::clearing::HOLD_SECONDS;
    use super::stuck::{self, TIP_SLOW_WAIT_SECONDS, TIP_WAIT_SECONDS};
    use super::{Upkeep, filing::UPKEEP};
    use ignite_core::config::Os;
    use ignite_core::ids::{CheckName, Commit, InputHash, MachineId, RecordId};
    use ignite_core::protocol::Outcome;
    use ignite_core::protocol::hub::{
        MachineRegister, NewResult, ResultCost, ResultKey, TipState, TipVerified, TipWaiting,
    };

    let clock = start();
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    let windows = MachineId::new("m-w").unwrap();
    let mac = MachineId::new("m-m").unwrap();
    let reg = MachineRegister {
        machine: windows.clone(),
        name: "cygnus".to_owned(),
        os: Os::Windows,
        cores: 4,
        memory_mb: 4096,
        families: vec!["platform".to_owned()],
        version: "v".to_owned(),
    };
    hub::fleet::register(&conn, &reg, clock.now()).unwrap();
    let tip = Commit::new("abcd").unwrap();
    hub::tips::seen(&conn, tip.as_str(), clock.now()).unwrap();
    let awaited: Vec<CheckName> = ["platform:one", "platform:two", "platform:three"]
        .into_iter()
        .map(|name| CheckName::new(name).unwrap())
        .collect();
    let store_one = |conn: &mut rusqlite::Connection, seq: usize, at| {
        let key = ResultKey {
            check: awaited[0].clone(),
            hash: InputHash::new(format!("{seq:064x}")).unwrap(),
            platform: Some(Os::Windows),
        };
        let result = NewResult {
            key,
            outcome: Outcome::Pass,
            machine: windows.clone(),
            record: RecordId(seq as u64),
            output: None,
            cost: ResultCost::default(),
            areas: Vec::new(),
        };
        hub::results::store(conn, &[result], at).unwrap();
    };
    let report = |waited_s| TipWaiting {
        commit: tip.clone(),
        machine: mac.clone(),
        platform: Os::Windows,
        checks: awaited.clone(),
        waited_s,
        unrunnable: Vec::new(),
    };

    // Store one awaited result every 10 minutes: suppressed at 1 hour, filed at 2 hours.
    let (step, mut seq, mut elapsed) = (600.0, 1usize, 0.0);
    while elapsed < TIP_SLOW_WAIT_SECONDS {
        clock.advance(step);
        elapsed += step;
        store_one(&mut conn, seq, clock.now());
        seq += 1;
        if (elapsed - TIP_WAIT_SECONDS).abs() < f64::EPSILON {
            let early = stuck::tip_waiting(&mut conn, UPKEEP, &report(elapsed), clock.now());
            assert!(early.unwrap().is_empty());
        }
    }
    let filed = stuck::tip_waiting(&mut conn, UPKEEP, &report(elapsed), clock.now()).unwrap();
    assert_eq!(filed.len(), 1);
    let id = &filed[0].ticket;
    let tk = ticket(&conn, id);
    assert_eq!((tk.status, tk.priority), (Status::Open, 1));
    assert!(tk.title.contains("cygnus"), "{}", tk.title);
    assert!(tk.body.contains("3 awaited checks remain"), "{}", tk.body);
    assert!(tk.body.contains("7.0/hour"), "{}", tk.body);

    // Continuing slow progress must not auto-close the slow-progress ticket.
    for _ in 0..6 {
        clock.advance(step);
        store_one(&mut conn, seq, clock.now());
        seq += 1;
        let pass = upkeep.pass(&mut conn, clock.now()).unwrap();
        assert!(!pass.tickets_closed.contains(id));
    }

    // Verifying the tip settles the slow-progress ticket.
    let verified = TipVerified {
        commit: tip,
        machine: mac,
        state: TipState::Verified,
        regression_passes: None,
        failed_descendant: None,
        failure: None,
    };
    hub::tips::verified(&conn, &verified, clock.now()).unwrap();
    let mut closed = Vec::new();
    for _ in 0..=((2.0 * HOLD_SECONDS / step) as usize) {
        clock.advance(step);
        closed.extend(upkeep.pass(&mut conn, clock.now()).unwrap().tickets_closed);
    }
    assert!(closed.contains(id), "{closed:?}");
}
