//! A verb process against the daemon's own side of the host protocol (`crate::host`), served
//! over a socket by a real [`Server`], with a starter that writes each run's result at once in
//! place of a supervisor.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ignite_core::fakes::{FakeBuilds, FakeClock};
use ignite_core::ids::{Commit, Handle, TreeId};
use ignite_core::protocol::Usage;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::host::RequestId;
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Result};
use ignite_os::ipc::Ipc;

use super::tests::{FakeVerb, Scratch, T0, assembled, request, row, spec, timing};
use super::{Outcome, Process, SocketHost};
use crate::handles::{Waited, lock};
use crate::host::{Host, HostParts, LandingTrees, RunStarter, Timing as HostTiming};
use crate::server::{Registry, Server, serve_listener};
use crate::steps::Steps;
use crate::supervise::{ResultFile, SpecFile};
use crate::trees::TreeLease;

/// Starts each command by writing its run directory with a result that says it exited 0.
struct Finisher {
    runs: PathBuf,
    started: Mutex<Vec<(String, PathBuf)>>,
}

impl RunStarter for Finisher {
    fn begin(
        &self,
        spec: &CommandSpec,
        _since: Timestamp,
        _cancelled: &dyn Fn() -> bool,
    ) -> io::Result<Option<PathBuf>> {
        let mut started = lock(&self.started);
        let dir = self.runs.join(format!("run-{}", started.len() + 1));
        fs::create_dir_all(&dir)?;
        let file = SpecFile {
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
            queued_s: 0.0,
            jobserver: None,
            build_jobs: None,
            hang_limit_s: None,
            grant: None,
        };
        fs::write(dir.join("spec.json"), serde_json::to_vec(&file)?)?;
        let result = ResultFile {
            exit: Some(0),
            output: dir.join("output.log"),
            usage: Usage::default(),
            hung: false,
        };
        fs::write(dir.join("result.json"), serde_json::to_vec(&result)?)?;
        let request = spec
            .env
            .iter()
            .find(|(name, _)| name == "IGNITE_REQUEST")
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        started.push((request, dir.clone()));
        Ok(Some(dir))
    }

    fn find(&self, request: &RequestId) -> Option<PathBuf> {
        let id = request.to_string();
        lock(&self.started)
            .iter()
            .find(|(found, _)| *found == id)
            .map(|(_, dir)| dir.clone())
    }

    fn runs_of(&self, _handle: &Handle) -> Vec<PathBuf> {
        Vec::new()
    }

    fn end(&self, _run: &std::path::Path) {}

    fn kill(&self, _run: &std::path::Path) {}

    fn waiting_for(&self, spec: &CommandSpec) -> String {
        format!("{} cores", spec.cores)
    }
}

/// No landing tree is ever free.
struct NoTrees;

impl LandingTrees for NoTrees {
    fn lease(&self, _: &str, _: &Handle, _: &Commit, _: usize) -> Result<Option<TreeLease>> {
        Ok(None)
    }

    fn release(&self, _: &Handle, _: &TreeId, _: Option<&Commit>) -> Result<()> {
        Ok(())
    }
}

/// The real daemon side answers the verb process over the socket: its command is started once
/// with the request ID in its environment, its step reaches the daemon's registry, and the
/// reply it writes reaches whoever waits on the handle in the daemon once it says it finished.
#[test]
fn a_verb_process_finishes_through_the_daemon_over_the_socket() {
    let scratch = Scratch::new("socket");
    let socket = scratch.home.join("d.sock").to_string_lossy().into_owned();
    let db = Arc::new(Mutex::new(ignite_store::local::open_in_memory().unwrap()));
    let steps = Arc::new(Steps::new(Arc::new(FakeClock::new(T0))));
    let starter = Arc::new(Finisher {
        runs: scratch.runs(),
        started: Mutex::new(Vec::new()),
    });
    let host = Host::with_timing(
        HostParts {
            starter: starter.clone(),
            builds: Arc::new(FakeBuilds::default()),
            landing: Arc::new(NoTrees),
            db: db.clone(),
            steps: steps.clone(),
            clock: Arc::new(FakeClock::new(T0)),
        },
        HostTiming {
            block: Duration::from_millis(200),
            abandon: Duration::from_secs(5),
            watch: Duration::from_millis(20),
            grace: Duration::from_millis(50),
        },
    );
    let mut registry = Registry::default();
    host.register(&mut registry);
    let ctx = super::tests::context();
    let server =
        Arc::new(Server::with_steps(ctx, registry, Some(db.clone()), steps.clone()).unwrap());
    host.set_handles(server.handles().clone());
    let handle = server
        .handles()
        .create_for("test", None, None, Some(&request()));
    let listener = ignite_os::Native.listen(&socket).unwrap();
    {
        let server = server.clone();
        std::thread::spawn(move || serve_listener(&server, listener.as_ref()));
    }

    let process = Process {
        handle: handle.clone(),
        home: scratch.home.clone(),
        db: db.clone(),
        host: Arc::new(SocketHost {
            socket,
            cwd: scratch.home.clone(),
        }),
        executable: "1a2b3c4".to_owned(),
        clock: Arc::new(FakeClock::new(T0)),
        timing: timing(),
    };
    let watched = steps.clone();
    let named = handle.clone();
    let outcome = process.run(|remote, _, _| {
        Ok(assembled(
            remote,
            FakeVerb(Box::new(move |ctx| {
                crate::steps::step("running cargo test");
                let result = ctx.launcher.run(&spec(&["cargo", "test"]));
                // The daemon lists the verb with its step, as for a verb on its own thread.
                let shown = super::tests::until(|| {
                    watched.by_handle(&named).is_some_and(|activity| {
                        activity.view().step.as_deref() == Some("running cargo test")
                    })
                });
                Reply::text(Exit::Success, format!("{:?} {shown}", result.exit))
            })),
        ))
    });
    let Outcome::Finished(reply) = outcome else {
        panic!("the verb did not finish: {outcome:?}");
    };
    assert_eq!(reply.text, "Some(0) true");
    assert_eq!(row(&db, &handle).state, "done");
    let begun = lock(&starter.started).clone();
    assert_eq!(begun.len(), 1);
    assert_eq!(begun[0].0, format!("{handle}#1"));
    match server.handles().wait(&handle, 5, None) {
        Waited::Done(waited) => assert_eq!(waited, reply),
        other => panic!("the daemon did not hand the reply on: {other:?}"),
    }
    assert!(steps.by_handle(&handle).is_none(), "the activity ended");
}
