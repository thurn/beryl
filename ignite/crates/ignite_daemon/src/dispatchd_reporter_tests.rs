//! The daemon's justiciar: crash records and stuck loops reported to the hub, a justiciar
//! prompt handed to a local worker while the hub is down, and `ignite justiciar`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ignite_core::protocol::client::Exit;

use super::tests::{Rig, request, rig};
use crate::crash::{self, CrashRecord};
use crate::justiciar::{HUB_DOWN_S, JusticiarVerb, Reporter};
use crate::loop_health::LoopHealth;
use crate::server::Verb;

fn home(name: &str) -> PathBuf {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-justiciar-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn reporter(rig: &Rig, home: &Path) -> (Reporter, LoopHealth) {
    let health = LoopHealth::new(rig.clock.clone());
    let reporter = Reporter::new(
        home.to_path_buf(),
        "jarvis".to_owned(),
        "abc1234".to_owned(),
        health.clone(),
        rig.headless_dispatch.clone(),
    );
    (reporter, health)
}

fn panic_record() -> CrashRecord {
    CrashRecord {
        at: "2026-11-18T01:00:00Z".to_owned(),
        commit: "abc1234".to_owned(),
        thread: "ignite-tip".to_owned(),
        message: "index out of bounds".to_owned(),
        location: "src/tip.rs:12".to_owned(),
        backtrace: "0: frame".to_owned(),
    }
}

#[test]
fn a_crash_record_is_reported_once_and_marked() {
    let rig = rig(&[]);
    let home = home("crash");
    crash::write(&home, &panic_record()).unwrap();
    let (reporter, _) = reporter(&rig, &home);
    reporter.tick(&rig.ctx, true);
    reporter.tick(&rig.ctx, true);
    let reports = rig.hub.justiciar_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].subject, "crash:ignite-tip:src/tip.rs:12");
    assert!(!reports[0].on_machine);
    assert!(reports[0].details.contains("index out of bounds"));
    assert!(crash::pending(&home).is_empty());
}

#[test]
fn a_crash_record_waits_while_the_hub_is_down() {
    let rig = rig(&[]);
    let home = home("crash-down");
    crash::write(&home, &panic_record()).unwrap();
    let (reporter, _) = reporter(&rig, &home);
    rig.hub.set_unreachable(true);
    reporter.tick(&rig.ctx, true);
    assert_eq!(crash::pending(&home).len(), 1);
    rig.hub.set_unreachable(false);
    reporter.tick(&rig.ctx, true);
    assert!(crash::pending(&home).is_empty());
}

#[test]
fn a_stuck_loop_is_reported_once_for_this_machine() {
    let rig = rig(&[]);
    let home = home("loop");
    let (reporter, health) = reporter(&rig, &home);
    let _ticker = health.register("sender", Duration::from_secs(600));
    rig.clock.advance(601.0);
    reporter.tick(&rig.ctx, true);
    rig.clock.advance(60.0);
    reporter.tick(&rig.ctx, true);
    let reports = rig.hub.justiciar_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].subject, "loop:sender");
    assert!(reports[0].on_machine);
}

#[test]
fn main_loop_reports_the_ref_while_git_waits_for_a_fetch() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    use crate::main::main_round;
    use crate::trees::tests::{Fixture, git};

    let rig = rig(&[]);
    let home = home("main-fetch");
    let (reporter, health) = reporter(&rig, &home);
    let ticker = health.register("ignite-main", Duration::from_mins(10));
    let fixture = Fixture::new(0, 1);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let remote = format!("http://{}", listener.local_addr().unwrap());
    git(&fixture.repo, &["remote", "set-url", "origin", &remote]);
    git(&fixture.repo, &["config", "remote.origin.proxy", ""]);

    std::thread::scope(|scope| {
        let round = scope.spawn(|| {
            ticker.round(|| {
                main_round(&fixture.trees, || {
                    let status = &health.lines()[0];
                    assert!(status.contains("reloading the check manifests"), "{status}");
                    let problem = &health.problems()[0];
                    assert!(
                        problem.contains("reloading the check manifests"),
                        "{problem}"
                    );
                    Ok(false)
                })
            })
        });

        let (connection, _) = listener.accept().unwrap();
        drop(listener);
        let mut request = BufReader::new(connection);
        let mut line = String::new();
        request.read_line(&mut line).unwrap();
        assert!(line.starts_with("GET /info/refs"), "{line}");
        loop {
            line.clear();
            if request.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                break;
            }
        }

        rig.clock.advance(601.0);
        let status = health.lines();
        reporter.tick(&rig.ctx, true);
        let reports = rig.hub.justiciar_reports();

        // A plain list of refs suffices because the checkout already has the advertised commit.
        let refs = format!("{}\trefs/heads/main\n", fixture.tip());
        write!(
            request.get_mut(),
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{refs}",
            refs.len()
        )
        .unwrap();
        drop(request);

        let step = "fetching refs/heads/main from origin for the main checkout";
        assert!(status[0].contains(step), "{status:?}");
        assert_eq!(reports.len(), 1, "{reports:?}");
        assert_eq!(reports[0].subject, "loop:ignite-main");
        assert!(reports[0].details.contains(step), "{reports:?}");
        assert_eq!(round.join().unwrap(), Some(()), "{:?}", health.reports());
    });
}

/// On 2026-11-20 cygnus filed a P0 justiciar ticket because its idle loop had waited ten
/// minutes in the scheduler's queue for 12 cores while a tip round and an agent's build held all
/// 18. That is how idle work is meant to behave, so a round waiting for cores is not reported;
/// the same round is reported once it has been stuck on anything else for the stall time. The
/// time it waited for cores does not count towards that: cygnus reported its idle round stuck
/// three minutes into a build that had waited 25 minutes for cores.
#[test]
fn a_loop_waiting_for_cores_is_not_reported_until_it_waits_on_something_else() {
    use std::sync::{Arc, Barrier};

    let rig = rig(&[]);
    let home = home("loop-cores");
    let (reporter, health) = reporter(&rig, &home);
    let idle = health.register("ignite-idle", Duration::from_secs(600));
    let barriers: Vec<Arc<Barrier>> = (0..4).map(|_| Arc::new(Barrier::new(2))).collect();
    let round = {
        let barriers = barriers.clone();
        std::thread::spawn(move || {
            idle.round(|| {
                crate::steps::step("building cli in tree-01");
                let cores = crate::steps::waiting_for_cores("12 cores for idle work");
                barriers[0].wait();
                barriers[1].wait();
                drop(cores);
                let _lease = crate::steps::waiting("the tree-01 lease");
                barriers[2].wait();
                barriers[3].wait();
                Ok::<_, String>(())
            })
        })
    };
    barriers[0].wait();
    rig.clock.advance(601.0);
    reporter.tick(&rig.ctx, true);
    assert!(rig.hub.justiciar_reports().is_empty());
    barriers[1].wait();
    barriers[2].wait();
    rig.clock.advance(599.0);
    reporter.tick(&rig.ctx, true);
    assert!(rig.hub.justiciar_reports().is_empty());
    rig.clock.advance(2.0);
    reporter.tick(&rig.ctx, true);
    barriers[3].wait();
    round.join().unwrap();
    let reports = rig.hub.justiciar_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].subject, "loop:ignite-idle");
}

#[test]
fn a_hub_down_for_fifteen_minutes_gets_one_local_justiciar() {
    let rig = rig(&[]);
    let fake = super::cli_tests::cli(&rig, 1);
    let home = home("hub-down");
    let (reporter, _) = reporter(&rig, &home);
    reporter.tick(&rig.ctx, false);
    rig.clock.advance(HUB_DOWN_S - 1.0);
    reporter.tick(&rig.ctx, false);
    assert!(fake.justiciars().is_empty());
    rig.clock.advance(1.0);
    reporter.tick(&rig.ctx, false);
    reporter.tick(&rig.ctx, false);
    assert_eq!(fake.justiciars().len(), 1);
    assert!(fake.justiciars()[0].prompt.contains("has not answered"));
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn asking_for_a_justiciar_files_it_and_keeps_it_while_the_hub_is_down() {
    let rig = rig(&[]);
    let fake = super::cli_tests::cli(&rig, 1);
    let home = home("verb");
    let verb = JusticiarVerb {
        home: home.clone(),
        host: "jarvis".to_owned(),
        headless_dispatch: rig.headless_dispatch.clone(),
    };
    let ask = request(
        "justiciar",
        &["the remote rejects every push"],
        Some("local_w9"),
    );
    let reply = verb.run(&rig.ctx, &ask);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(rig.hub.justiciar_reports()[0].subject, "request:local_w9");

    rig.hub.set_unreachable(true);
    let reply = verb.run(&rig.ctx, &ask);
    assert_eq!(reply.exit, Exit::Unreachable, "{}", reply.text);
    assert_eq!(fake.justiciars().len(), 1);
    rig.hub.set_unreachable(false);
    let (reporter, _) = reporter(&rig, &home);
    reporter.tick(&rig.ctx, true);
    assert_eq!(rig.hub.justiciar_reports().len(), 2);
    assert!(!home.join("justiciar-request.json").exists());
}
