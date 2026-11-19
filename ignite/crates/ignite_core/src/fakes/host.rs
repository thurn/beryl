//! [`FakeHost`]: an in-memory daemon for testing verb processes against the host protocol.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::host::HostClient;
use crate::ids::{Commit, Handle, TreeId};
use crate::protocol::host::{
    Ack, AllowStop, Build, BuildReply, Finished, Hello, HelloReply, HostCall, LeaseLanding,
    LeaseLandingReply, PROTOCOL_VERSION, Poll, RefuseStop, RefuseStopReply, ReleaseLanding,
    RequestId, Run, RunReply, StepUpdate, StopState,
};
use crate::traits::{Error, Result};

use super::lock;

/// A daemon that answers host calls from memory and records each one.
///
/// By default every command starts at once in `runs/<handle>-<counter>`, every build is
/// current, and every landing lease gets `landing-1` at the commit asked for. A test can script
/// waits, builds and leases, ask the verb to stop, take the daemon away, or make it drop the
/// first attempt of every distinct call, as a restart in the middle of each would.
///
/// Like the real daemon, it starts one command per request ID and builds once per request ID,
/// however often a call is repeated.
#[derive(Debug)]
pub struct FakeHost {
    state: Mutex<State>,
}

// Each flag is a switch a test throws on its own, so they do not combine into one enum.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
struct State {
    protocol: u32,
    running: bool,
    down: bool,
    drop_each_once: bool,
    /// The calls whose first attempt was dropped, by verb and body.
    dropped: HashSet<(String, String)>,
    /// Every attempt, answered or not: the verb and its body as JSON.
    calls: Vec<(String, serde_json::Value)>,
    hellos: Vec<Hello>,
    runs_folder: PathBuf,
    /// How many times each new `host-run` answers `waiting` before its command starts.
    run_waits: u32,
    waits_left: HashMap<RequestId, u32>,
    started: BTreeMap<RequestId, (Run, PathBuf)>,
    scripted_builds: HashMap<(TreeId, String), BuildReply>,
    builds: BTreeMap<RequestId, (Build, BuildReply)>,
    scripted_leases: VecDeque<LeaseLandingReply>,
    /// Each tree leased and not released, with the holder and commit it was leased for.
    leased: BTreeMap<TreeId, (String, Commit, LeaseLandingReply)>,
    released: Vec<ReleaseLanding>,
    steps: Vec<StepUpdate>,
    stop: bool,
    daemon_started: Option<String>,
    refusing: Option<String>,
    finished: Vec<Handle>,
}

impl Default for FakeHost {
    fn default() -> Self {
        FakeHost {
            state: Mutex::new(State {
                protocol: PROTOCOL_VERSION,
                running: true,
                down: false,
                drop_each_once: false,
                dropped: HashSet::new(),
                calls: Vec::new(),
                hellos: Vec::new(),
                runs_folder: PathBuf::from("runs"),
                run_waits: 0,
                waits_left: HashMap::new(),
                started: BTreeMap::new(),
                scripted_builds: HashMap::new(),
                builds: BTreeMap::new(),
                scripted_leases: VecDeque::new(),
                leased: BTreeMap::new(),
                released: Vec::new(),
                steps: Vec::new(),
                stop: false,
                daemon_started: None,
                refusing: None,
                finished: Vec::new(),
            }),
        }
    }
}

impl FakeHost {
    /// Reports `protocol` as the daemon's newest version in `hello`.
    pub fn set_protocol(&self, protocol: u32) {
        lock(&self.state).protocol = protocol;
    }

    /// Says in `hello` that the handle no longer runs, as after `ignite stop` while no daemon
    /// ran.
    pub fn set_running(&self, running: bool) {
        lock(&self.state).running = running;
    }

    /// Reports `started` as the instant the daemon started in every answer that carries it, as
    /// a daemon restarted into a new build would.
    pub fn set_daemon_started(&self, started: &str) {
        lock(&self.state).daemon_started = Some(started.to_owned());
    }

    /// While down, every call fails as unreachable and changes nothing.
    pub fn set_down(&self, down: bool) {
        lock(&self.state).down = down;
    }

    /// Fails the first attempt of every distinct call (the same verb and body) as unreachable,
    /// without acting on it, and answers the repeat.
    pub fn drop_each_once(&self, drop: bool) {
        lock(&self.state).drop_each_once = drop;
    }

    /// Makes each later new `host-run` answer `waiting` this many times before starting.
    pub fn wait_runs(&self, times: u32) {
        lock(&self.state).run_waits = times;
    }

    /// The folder that run directories are named under.
    pub fn set_runs_folder(&self, folder: PathBuf) {
        lock(&self.state).runs_folder = folder;
    }

    /// Answers every later new build of `product` in `tree` with `reply`.
    pub fn script_build(&self, tree: &TreeId, product: &str, reply: BuildReply) {
        lock(&self.state)
            .scripted_builds
            .insert((tree.clone(), product.to_owned()), reply);
    }

    /// Answers the next new landing lease with `reply`, before the default.
    pub fn script_lease(&self, reply: LeaseLandingReply) {
        lock(&self.state).scripted_leases.push_back(reply);
    }

    /// Asks the verb to stop, unless it refuses; returns the refusal's reason if it does.
    pub fn request_stop(&self) -> std::result::Result<(), String> {
        let mut state = lock(&self.state);
        if let Some(reason) = &state.refusing {
            return Err(reason.clone());
        }
        state.stop = true;
        Ok(())
    }

    /// Every attempt so far, answered or not: the verb and its body.
    pub fn calls(&self) -> Vec<(String, serde_json::Value)> {
        lock(&self.state).calls.clone()
    }

    /// The verbs of [`FakeHost::calls`], in order.
    pub fn verbs(&self) -> Vec<String> {
        lock(&self.state)
            .calls
            .iter()
            .map(|(verb, _)| verb.clone())
            .collect()
    }

    pub fn hellos(&self) -> Vec<Hello> {
        lock(&self.state).hellos.clone()
    }

    /// Each command started, once per request ID, with its run directory.
    pub fn started(&self) -> Vec<(Run, PathBuf)> {
        lock(&self.state).started.values().cloned().collect()
    }

    /// Each build made, once per request ID, with its answer.
    pub fn builds(&self) -> Vec<(Build, BuildReply)> {
        lock(&self.state).builds.values().cloned().collect()
    }

    /// The landing trees leased and not yet released.
    pub fn leased(&self) -> Vec<TreeId> {
        lock(&self.state).leased.keys().cloned().collect()
    }

    pub fn released(&self) -> Vec<ReleaseLanding> {
        lock(&self.state).released.clone()
    }

    pub fn steps(&self) -> Vec<StepUpdate> {
        lock(&self.state).steps.clone()
    }

    /// The reason stopping is refused for, while it is.
    pub fn refusing(&self) -> Option<String> {
        lock(&self.state).refusing.clone()
    }

    pub fn finished(&self) -> Vec<Handle> {
        lock(&self.state).finished.clone()
    }

    /// Records the attempt, and fails it as the daemon's absence would. On success the caller
    /// holds the state to answer.
    fn attempt<C: HostCall>(&self, call: &C) -> Result<std::sync::MutexGuard<'_, State>> {
        let mut state = lock(&self.state);
        let body = serde_json::to_value(call).expect("host calls serialise");
        state.calls.push((C::VERB.to_owned(), body.clone()));
        if state.down {
            return Err(Error::Unreachable(format!(
                "{}: the daemon is down",
                C::VERB
            )));
        }
        if state.drop_each_once && state.dropped.insert((C::VERB.to_owned(), body.to_string())) {
            return Err(Error::Unreachable(format!(
                "{}: the daemon restarted",
                C::VERB
            )));
        }
        Ok(state)
    }
}

impl HostClient for FakeHost {
    fn hello(&self, call: &Hello) -> Result<HelloReply> {
        let mut state = self.attempt(call)?;
        state.hellos.push(call.clone());
        Ok(HelloReply {
            protocol: state.protocol,
            running: state.running,
            stop: state.stop,
            daemon_started: state.daemon_started.clone(),
        })
    }

    fn run(&self, call: &Run) -> Result<RunReply> {
        let mut state = self.attempt(call)?;
        if let Some((_, run)) = state.started.get(&call.request_id) {
            return Ok(RunReply::Started { run: run.clone() });
        }
        if state.stop {
            return Ok(RunReply::Refused {
                reason: "a stop was asked".to_owned(),
            });
        }
        let waits = state.run_waits;
        let left = state
            .waits_left
            .entry(call.request_id.clone())
            .or_insert(waits);
        if *left > 0 {
            *left -= 1;
            return Ok(RunReply::Waiting {
                what: format!("{} cores", call.spec.cores),
            });
        }
        let id = &call.request_id;
        let run = state
            .runs_folder
            .join(format!("{}-{}", id.handle(), id.counter()));
        state
            .started
            .insert(call.request_id.clone(), (call.clone(), run.clone()));
        Ok(RunReply::Started { run })
    }

    fn build(&self, call: &Build) -> Result<BuildReply> {
        let mut state = self.attempt(call)?;
        if let Some((_, reply)) = state.builds.get(&call.request_id) {
            return Ok(reply.clone());
        }
        let reply = state
            .scripted_builds
            .get(&(call.tree.clone(), call.product.clone()))
            .cloned()
            .unwrap_or(BuildReply::Current);
        if !matches!(reply, BuildReply::Waiting { .. }) {
            state
                .builds
                .insert(call.request_id.clone(), (call.clone(), reply.clone()));
        }
        Ok(reply)
    }

    fn lease_landing(&self, call: &LeaseLanding) -> Result<LeaseLandingReply> {
        let mut state = self.attempt(call)?;
        let held = state
            .leased
            .values()
            .find(|(holder, commit, _)| *holder == call.holder && *commit == call.commit);
        if let Some((_, _, reply)) = held {
            return Ok(reply.clone());
        }
        let reply =
            state
                .scripted_leases
                .pop_front()
                .unwrap_or_else(|| LeaseLandingReply::Leased {
                    tree: TreeId::new("landing-1").expect("a valid tree ID"),
                    path: PathBuf::from("/trees/landing-1"),
                    branch: "landing-1".to_owned(),
                    at: call.commit.clone(),
                });
        if let LeaseLandingReply::Leased { tree, .. } = &reply {
            if state.leased.contains_key(tree) {
                return Ok(LeaseLandingReply::Busy);
            }
            let entry = (call.holder.clone(), call.commit.clone(), reply.clone());
            state.leased.insert(tree.clone(), entry);
        }
        Ok(reply)
    }

    fn release_landing(&self, call: &ReleaseLanding) -> Result<Ack> {
        let mut state = self.attempt(call)?;
        if state.leased.remove(&call.tree).is_some() {
            state.released.push(call.clone());
        }
        Ok(Ack {})
    }

    fn step(&self, call: &StepUpdate) -> Result<StopState> {
        let mut state = self.attempt(call)?;
        state.steps.push(call.clone());
        Ok(StopState {
            stop: state.stop,
            daemon_started: state.daemon_started.clone(),
        })
    }

    fn refuse_stop(&self, call: &RefuseStop) -> Result<RefuseStopReply> {
        let mut state = self.attempt(call)?;
        if state.stop {
            return Ok(RefuseStopReply { refused: false });
        }
        state.refusing = Some(call.reason.clone());
        Ok(RefuseStopReply { refused: true })
    }

    fn allow_stop(&self, call: &AllowStop) -> Result<Ack> {
        self.attempt(call)?.refusing = None;
        Ok(Ack {})
    }

    fn poll(&self, call: &Poll) -> Result<StopState> {
        let state = self.attempt(call)?;
        Ok(StopState {
            stop: state.stop,
            daemon_started: state.daemon_started.clone(),
        })
    }

    fn finished(&self, call: &Finished) -> Result<Ack> {
        let mut state = self.attempt(call)?;
        if !state.finished.contains(&call.handle) {
            state.finished.push(call.handle.clone());
        }
        Ok(Ack {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Class;
    use crate::protocol::host::{Command, RequestIds};
    use crate::time::Timestamp;

    fn handle() -> Handle {
        Handle::new("L-6").unwrap()
    }

    fn run(ids: &RequestIds) -> Run {
        Run {
            handle: handle(),
            request_id: ids.next(),
            first_asked_at: Timestamp::from_millis(0),
            spec: Command {
                args: vec!["true".to_owned()],
                cwd: PathBuf::from("/trees/tree-07"),
                env: Vec::new(),
                class: Class::Land,
                cores: 4,
                resources: Vec::new(),
                grows: false,
                tree: None,
                ticket: None,
                hang_limit_s: None,
            },
        }
    }

    /// Sends `call` until it is answered, as a verb process does across a restart.
    fn retry<T>(mut send: impl FnMut() -> Result<T>) -> T {
        loop {
            match send() {
                Ok(answer) => return answer,
                Err(Error::Unreachable(_)) => {}
                Err(other) => panic!("{other}"),
            }
        }
    }

    #[test]
    fn a_dropped_call_repeated_starts_its_command_once() {
        let host = FakeHost::default();
        host.drop_each_once(true);
        let ids = RequestIds::new(handle());
        let first = run(&ids);
        let second = run(&ids);
        let a = retry(|| host.run(&first));
        let again = retry(|| host.run(&first));
        let b = retry(|| host.run(&second));
        assert_eq!(a, again);
        assert_ne!(a, b);
        assert_eq!(host.started().len(), 2);
        assert_eq!(
            host.verbs().len(),
            5,
            "two dropped attempts and three answers"
        );
    }

    #[test]
    fn a_waiting_run_starts_after_its_waits() {
        let host = FakeHost::default();
        host.wait_runs(2);
        let ids = RequestIds::new(handle());
        let call = run(&ids);
        assert!(matches!(host.run(&call), Ok(RunReply::Waiting { .. })));
        assert!(matches!(host.run(&call), Ok(RunReply::Waiting { .. })));
        assert!(matches!(host.run(&call), Ok(RunReply::Started { .. })));
        assert_eq!(host.started().len(), 1);
    }

    #[test]
    fn a_stop_refuses_new_commands_unless_stopping_is_refused() {
        let host = FakeHost::default();
        let refuse = RefuseStop {
            handle: handle(),
            reason: "pushing".to_owned(),
        };
        assert!(host.refuse_stop(&refuse).unwrap().refused);
        assert_eq!(host.request_stop(), Err("pushing".to_owned()));
        host.allow_stop(&AllowStop { handle: handle() }).unwrap();
        host.request_stop().unwrap();
        assert!(host.poll(&Poll { handle: handle() }).unwrap().stop);
        assert!(!host.refuse_stop(&refuse).unwrap().refused);
        let ids = RequestIds::new(handle());
        assert!(matches!(host.run(&run(&ids)), Ok(RunReply::Refused { .. })));
    }

    #[test]
    fn a_repeated_lease_returns_the_same_tree_and_another_holder_waits() {
        let host = FakeHost::default();
        let commit = Commit::new("1a2b3c4").unwrap();
        let lease = LeaseLanding {
            handle: handle(),
            holder: "m-1:T1".to_owned(),
            commit: commit.clone(),
            keep_free: 0,
        };
        let first = host.lease_landing(&lease).unwrap();
        assert_eq!(host.lease_landing(&lease).unwrap(), first);
        let other = LeaseLanding {
            holder: "m-1:T2".to_owned(),
            ..lease
        };
        assert_eq!(host.lease_landing(&other).unwrap(), LeaseLandingReply::Busy);
        let tree = TreeId::new("landing-1").unwrap();
        let release = ReleaseLanding {
            handle: handle(),
            tree,
            built: Some(commit),
        };
        host.release_landing(&release).unwrap();
        host.release_landing(&release).unwrap();
        assert_eq!(host.released().len(), 1);
        assert!(host.leased().is_empty());
    }

    #[test]
    fn a_down_daemon_answers_nothing() {
        let host = FakeHost::default();
        host.set_down(true);
        let hello = Hello {
            handle: handle(),
            protocol: PROTOCOL_VERSION,
            executable: "1a2b3c4".to_owned(),
            pid: 42,
        };
        assert!(matches!(host.hello(&hello), Err(Error::Unreachable(_))));
        host.set_down(false);
        assert!(host.hello(&hello).unwrap().running);
        assert_eq!(host.hellos().len(), 1);
    }
}
