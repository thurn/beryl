//! The sender loop and its once-a-minute machine readings.

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::time::Duration;

use rusqlite::Connection;

use ignite_core::capacity::CapacityReport;
use ignite_core::protocol::hub::Heartbeat;
use ignite_core::protocol::machine_state::MachineSample;
use ignite_core::time::Timestamp;
use ignite_core::traits::Result;

use super::loops::Parts;
use super::selfupdate::Busy;
use crate::server::Server;

/// How often the outbox is sent and the heartbeat beats. Transcript lines are sent sooner,
/// as soon as they are queued (see `spawn_sender`).
const SEND_EVERY: Duration = Duration::from_secs(10);
/// How long the sender may run without finishing a round before it is reported as stuck.
const SENDER_STALLS_AFTER: Duration = Duration::from_mins(10);

const SAMPLE_EVERY_SECONDS: f64 = 60.0;

#[derive(Default)]
pub(super) struct Sampling {
    last: Cell<Option<Timestamp>>,
}

impl Sampling {
    /// Send ordinary heartbeats between samples. Retry a sample on the next heartbeat if
    /// the hub did not accept it.
    pub(super) fn send<T>(
        &self,
        now: Timestamp,
        mut beat: Heartbeat,
        collect: impl FnOnce() -> (MachineSample, CapacityReport),
        send: impl FnOnce(&Heartbeat) -> Result<T>,
    ) -> Result<T> {
        let due = self
            .last
            .get()
            .is_none_or(|last| now.seconds_since(last) >= SAMPLE_EVERY_SECONDS);
        if due {
            let (sample, capacity) = collect();
            beat.sample = Some(sample);
            beat.capacity = Some(capacity);
        }
        let reply = send(&beat)?;
        if due {
            self.last.set(Some(now));
        }
        Ok(reply)
    }
}

pub(super) struct SnapshotSection(pub Arc<crate::hubclient::HubClient>);

impl crate::doctor::StatusSection for SnapshotSection {
    fn title(&self) -> &'static str {
        "machine snapshots"
    }

    fn lines(&self) -> Vec<String> {
        let view = match self.0.fleet_view() {
            Ok(view) => view,
            Err(error) => return vec![format!("the hub did not answer: {error}")],
        };
        view.machines
            .into_iter()
            .map(|machine| {
                let Some(snapshot) = machine.snapshot else {
                    return format!("{} has no snapshot", machine.name);
                };
                let [(_, disk), (_, cpu), (_, workers), (_, ready), (_, next)] =
                    snapshot.readings();
                format!(
                    "{} at {} · {disk} · {cpu} · {workers} · {ready} ready · {next}",
                    machine.name, snapshot.at
                )
            })
            .collect()
    }
}

impl Parts {
    /// Sends the outbox, the heartbeat and any landing the hub has not heard of. After each
    /// heartbeat it compares the daemon's version with the hub's and restarts into a new
    /// executable once one is ready and no landing holds it back (see
    /// [`SelfUpdate::after_heartbeat`](super::selfupdate::SelfUpdate::after_heartbeat)).
    /// A release build runs on a thread of its own, so a round never waits for Cargo; the round
    /// only looks whether the build has finished and names it in the loop's step.
    pub(super) fn spawn_sender(&self, db: Connection, server: Arc<Server>) {
        let (ctx, client, landing, dispatcher, version) = (
            self.ctx.clone(),
            self.client.clone(),
            self.landing.clone(),
            self.dispatcher.clone(),
            self.version.clone(),
        );
        let (scheduler, trees) = (self.scheduler.clone(), self.trees.clone());
        let facts = self.facts.clone();
        let update = self.update.clone();
        let reporter = self.reporter.clone();
        let emergencies = self.emergencies.clone();
        let titles = self.titles.clone();
        let hostname = self.hostname.clone();
        self.start_loop("ignite-sender", SENDER_STALLS_AFTER, move |ticker| {
            // The hub's version from the last heartbeat it answered.
            let hub_version: RefCell<Option<String>> = RefCell::new(None);
            let samples = Sampling::default();
            let beat = || {
                // A daemon that took on a release without restarting reports that release.
                let version = update
                    .version()
                    .map_or_else(|| version.clone(), |commit| commit.to_string());
                let pool = dispatcher.pool();
                if let Err(error) =
                    crate::dispatchd::post_usage(pool, ctx.hub.as_ref(), &ctx.machine)
                {
                    crate::hubclient::HUB_WRITES.failed("plan usage not sent", &error);
                }
                let now = ctx.clock.now();
                let reply = samples.send(
                    now,
                    Heartbeat {
                        sample: None,
                        capacity: None,
                        machine: ctx.machine.clone(),
                        version,
                        workers: dispatcher.view().unwrap_or_default(),
                        cli_apps: dispatcher.cli_apps().map(str::to_owned).collect(),
                        families: current_families(&ctx.repo, &hostname),
                    },
                    || dispatcher.machine_sample(now, &scheduler, &trees, &facts),
                    |beat| client.heartbeat(beat),
                )?;
                if let Err(error) = crate::fleet_notice::heard(pool, &reply) {
                    eprintln!("ignite: fleet notice not saved: {error}");
                }
                *hub_version.borrow_mut() = Some(reply.hub_version);
                emergencies.heard(reply.emergency);
                Ok(())
            };
            loop {
                ticker.round(|| {
                    let sent = crate::outbox::tick(&db, ctx.hub.as_ref(), &beat);
                    titles.report_holders(&ctx, &db);
                    landing.deliver_pending(
                        &ctx,
                        |landed, next| dispatcher.chain(&ctx, &landed.ticket, next, landed.fresh),
                        || Ok(dispatcher.pressure(ctx.clock.now())),
                    );
                    if let Err(error) = update.poll_release() {
                        eprintln!("ignite: the release branch was not checked: {error}");
                    }
                    update.after_heartbeat(hub_version.borrow().as_deref(), busy(server.steps()));
                    reporter.tick(&ctx, sent.is_ok());
                    sent
                });
                // Between rounds, transcript lines go out as soon as they are queued, so someone
                // following a CLI worker on the dashboard sees them within about a second. Such
                // a send flushes the whole outbox, which only sends other entries sooner; the
                // heartbeat and the rest of the round keep their period.
                update.nap(SEND_EVERY, |most| {
                    if crate::outbox::wait_for_transcripts(most)
                        && let Err(error) = crate::outbox::flush(&db, ctx.hub.as_ref())
                    {
                        crate::hubclient::HUB_WRITES.failed("outbox not sent", &error);
                    }
                });
            }
        });
    }
}

/// What runs that a restart waits for: a verb in a part that may not be cut short, and any
/// landing, whether it runs on the daemon's threads or in a verb process of its own.
fn busy(steps: &crate::steps::Steps) -> Busy {
    let landing = steps.live().iter().any(
        |activity| matches!(activity.kind(), crate::steps::Kind::Verb(verb) if verb == "land"),
    );
    Busy {
        critical: steps.critical_now(),
        landing,
    }
}

/// The families the main checkout's `ignite.toml` lists for this machine now, empty for every
/// family, for the heartbeat to report: the hub otherwise holds the list the machine registered
/// with, however its `ignite.toml` changed since. `None`, which leaves the hub's list, when the
/// file cannot be read or parsed (such as halfway through a merge).
fn current_families(repo: &std::path::Path, hostname: &str) -> Option<Vec<String>> {
    crate::manifests::machine_families(repo, hostname)
        .ok()
        .flatten()
        .map(Option::unwrap_or_default)
}

#[cfg(test)]
mod tests {
    use ignite_core::capacity::{MachineFacts, ProviderCapacity};
    use ignite_core::config::Os;
    use ignite_core::dispatch_pressure::DispatchPressure;
    use ignite_core::protocol::hub::{
        MachineRegister, ProviderOffer, Room, WorkerRole, WorkerState,
    };
    use ignite_core::protocol::machine_state::{ReclaimableDisk, StartBlock};
    use ignite_hub::{
        auth::Writer,
        routes::{Request, dispatch},
    };
    use ignite_store::hub;

    use super::*;
    use crate::dispatchd::capacity_report;
    use crate::sched::Status;

    fn fixture_sample(start: Timestamp) -> MachineSample {
        MachineSample {
            pressure: DispatchPressure {
                free_gb: Some(47),
                disk_reserve_gib: Some(60),
                ..DispatchPressure::default()
            },
            reclaimable: Some(ReclaimableDisk {
                bytes: 123_456,
                measured_at: start,
            }),
            cpu_percent: Some(79),
            queued_cores: 12,
            swap_used_mb: Some(256),
            idle_workers: 4,
            room: Room {
                start: 3,
                providers: vec![ProviderOffer {
                    name: "codex".into(),
                    tiers: vec![],
                    model_ids: std::collections::BTreeMap::new(),
                    default_model: None,
                }],
                memory_pressed: false,
            },
            free_ticket_trees: 3,
            admit_cores: 16,
            draining: false,
            shared_failure: None,
        }
    }

    fn beat(machine: &ignite_core::ids::MachineId) -> Heartbeat {
        Heartbeat {
            sample: None,
            capacity: None,
            machine: machine.clone(),
            version: "v1".into(),
            workers: ["working", "stalled", "down"]
                .into_iter()
                .map(|state| WorkerState {
                    session: format!("cli-codex-{state}").parse().unwrap(),
                    app: "codex".into(),
                    role: WorkerRole::Worker,
                    state: state.into(),
                    ticket: None,
                    files: vec![],
                    command: None,
                    command_since: None,
                    command_running: false,
                })
                .collect(),
            cli_apps: vec!["codex".into()],
            families: Some(vec![]),
        }
    }

    fn post(conn: &mut Connection, beat: &Heartbeat, at: Timestamp) {
        let request = Request {
            method: "POST".into(),
            path: "/machines/heartbeat".into(),
            query: vec![],
            writer: Some(Writer {
                machine: beat.machine.clone(),
                actor: "test".into(),
            }),
            body: serde_json::to_vec(beat).unwrap(),
            headers: vec![],
        };
        assert_eq!(
            dispatch::handle(conn, &request, at, &[]).unwrap().status,
            200
        );
    }

    fn register(
        conn: &Connection,
        machine: &ignite_core::ids::MachineId,
        cores: u32,
        at: Timestamp,
    ) {
        hub::fleet::register(
            conn,
            &MachineRegister {
                machine: machine.clone(),
                name: "test".into(),
                os: Os::Macos,
                cores,
                memory_mb: 32_000,
                families: vec![],
                version: "v1".into(),
            },
            at,
        )
        .unwrap();
    }

    #[test]
    fn every_machine_sampling_round_posts_the_complete_heartbeat_sample() {
        let mut conn = hub::open_in_memory().unwrap();
        let machine = "m-test".parse::<ignite_core::ids::MachineId>().unwrap();
        let start = Timestamp::EPOCH;
        register(&conn, &machine, 16, start);
        conn.execute_batch(
            "INSERT INTO tickets
             (id, title, type, status, priority, created_at, updated_at, created_by, version, cores)
             VALUES ('work', 'Work', 'task', 'open', 2, '1970-01-01T00:00:00.000Z',
                     '1970-01-01T00:00:00.000Z', 'test', 1, 1.0)",
        )
        .unwrap();
        let sample = fixture_sample(start);
        let sampling = Sampling::default();
        for (seconds, most_recent, free, admit, queue, block, reason) in [
            (0.0, 0.0, 47, 16, 0, StartBlock::Disk, "waits for disk"),
            (10.0, 0.0, 47, 16, 0, StartBlock::Disk, "waits for disk"),
            (60.0, 60.0, 100, 0, 0, StartBlock::Cores, "cores, 0.0 free"),
            (
                120.0,
                120.0,
                100,
                16,
                120,
                StartBlock::Pacing,
                "waited 120 s for cores",
            ),
        ] {
            let at = start.plus_seconds(seconds);
            sampling
                .send(
                    at,
                    beat(&machine),
                    || {
                        let mut sample = sample.clone();
                        sample.pressure.free_gb = Some(free);
                        sample.pressure.queue_seconds = queue;
                        sample.admit_cores = admit;
                        let report = CapacityReport::default();
                        (sample, report)
                    },
                    |beat| {
                        assert_eq!(beat.capacity.is_some(), beat.sample.is_some());
                        post(&mut conn, beat, at);
                        Ok(())
                    },
                )
                .unwrap();
            let snapshot = hub::machine_snapshots::latest(&conn, machine.as_str())
                .unwrap()
                .unwrap();
            assert_eq!(snapshot.at, start.plus_seconds(most_recent));
            assert_eq!(snapshot.free_disk_gib, Some(free));
            assert_eq!(snapshot.reclaimable, sample.reclaimable);
            assert_eq!(snapshot.next_ticket.unwrap().as_str(), "work");
            assert_eq!(snapshot.next_ticket_disk_gib, Some(100));
            assert_eq!(snapshot.start_block, Some(block));
            assert!(snapshot.start_reason.unwrap().contains(reason));
            assert_eq!(snapshot.cpu_percent, Some(79));
            assert_eq!(snapshot.queued_cores, 12);
            assert_eq!(snapshot.swap_used_mb, Some(256));
            assert_eq!(snapshot.idle_workers, 4);
            assert_eq!(snapshot.working_workers, 1);
            assert_eq!(snapshot.stalled_workers, 1);
            assert_eq!(snapshot.down_workers, 1);
            assert_eq!(snapshot.ready_tickets, 1);
        }
        let rows: u32 = conn
            .query_row("SELECT count(*) FROM machine_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 3);
        let reserved: u32 = conn
            .query_row("SELECT count(*) FROM reservations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(reserved, 0);
    }
    #[test]
    fn sampled_heartbeat_carries_capacity_to_the_fleet_api() {
        let facts = MachineFacts {
            os: Os::Macos,
            logical_cores: 20,
            physical_cores: 12,
            memory_mb: 32_000,
            disk_total_gb: 930,
        };
        let status = Status {
            cores: 16,
            outside: 3,
            granted: 8,
            committed: 6,
            used: 6,
            swap_used_mb: Some(100),
            memory_pressed: false,
            max_per_command: 12,
            free_memory_mb: Some(9_000),
            tip_build: 4,
            tip_build_used: 0,
            priority_grants: std::collections::BTreeMap::new(),
            classes: vec![],
            resources: vec![],
        };
        let mut sample = fixture_sample(Timestamp::EPOCH);
        let output = "rust/target".to_owned();
        let ticket = "work".parse::<ignite_core::ids::TicketId>().unwrap();
        sample.pressure.outputs = vec![output.clone()];
        sample.pressure.free_trees = vec![vec![output.clone()], vec![]];
        sample
            .pressure
            .leased
            .insert(ticket.clone(), vec![output.clone()]);
        let provider = ProviderCapacity {
            name: "codex".into(),
            tiers: vec!["sonnet".into()],
            probed: true,
            hold_reason: Some("plan_limit".into()),
        };
        let expected = CapacityReport {
            logical_cores: 20,
            physical_cores: 12,
            memory_mb: 32_000,
            admit_cores: 16.0,
            free_cores: 7.0,
            worker_room: 3,
            free_memory_mb: Some(9_000),
            memory_pressed: false,
            free_disk_gb: Some(47),
            disk_budget_gb: 300,
            queue_seconds: 14.25,
            outputs: vec![output.clone()],
            free_trees: vec![vec![output.clone()], vec![]],
            leased_trees: [(ticket, vec![output])].into(),
            providers: vec![provider.clone()],
        };
        let mut conn = hub::open_in_memory().unwrap();
        let machine = "m-test".parse::<ignite_core::ids::MachineId>().unwrap();
        let now = Timestamp::EPOCH;
        register(&conn, &machine, facts.logical_cores, now);

        Sampling::default()
            .send(
                now,
                beat(&machine),
                || {
                    let report =
                        capacity_report(&facts, &status, &sample, 300, 14.25, vec![provider]);
                    (sample, report)
                },
                |beat| {
                    assert_eq!(beat.capacity.as_ref(), Some(&expected));
                    assert!(beat.sample.is_some());
                    post(&mut conn, beat, now);
                    Ok(())
                },
            )
            .unwrap();

        let request = Request {
            method: "GET".into(),
            path: "/fleet".into(),
            query: vec![],
            writer: None,
            body: vec![],
            headers: vec![],
        };
        let response = ignite_hub::routes::fleet::handle(&mut conn, &request, now).unwrap();
        assert_eq!(response.status, 200);
        let view: ignite_core::fleet::FleetView = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(view.machines.len(), 1);
        assert_eq!(view.machines[0].id, machine);
        assert_eq!(view.machines[0].capacity.as_ref(), Some(&expected));
    }
}
