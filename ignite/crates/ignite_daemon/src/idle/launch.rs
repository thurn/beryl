//! Running idle jobs in verb processes. The daemon's idle loop decides whether idle work may run
//! and which job is next ([`Idle::next_job`]); [`Idle::drive`] then starts the job as the
//! internal verb `idle-job` in a verb process with a handle (see `crate::verbproc`) and waits for
//! the handle. The verb process outlives a restart of the daemon, so the loop of a restarted
//! daemon finds the job's handle in the idle folder and waits for it instead of starting a
//! second job.
//!
//! A request that idle work stop (a grant of the `idle` class that a higher class needs) still
//! ends the job's commands from the daemon's side; the job then fails, the verb process exits,
//! and the job runs again later from the state file, as it did when it ran in the daemon.

use std::path::PathBuf;
use std::sync::Arc;

use ignite_core::ids::Handle;
use ignite_core::protocol::Class;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::traits::{Error, Result};

use super::{Idle, IdleJob, error_reply};
use crate::handles::Waited;
use crate::server::{Context, Server, Verb};
use crate::steps;

/// How long the loop waits for a job's handle before it looks again.
const WAIT_S: u32 = 30;

/// How the idle loop starts a job and waits for it: the server, or a fake in tests.
pub trait JobRunner: Send + Sync {
    /// Starts `job` in a verb process and returns its handle. `named` hears the handle before
    /// the job starts, so that it is recorded before the process can outlive the daemon.
    fn start(&self, job: &IdleJob, named: &mut dyn FnMut(&Handle)) -> Result<Handle>;
    /// Waits at most `seconds` for the job of `handle`.
    fn wait(&self, handle: &Handle, seconds: u32) -> JobWait;
}

/// How a wait on an idle job's handle ended.
#[derive(Clone, Debug, PartialEq)]
pub enum JobWait {
    /// The job finished with this reply.
    Done(Reply),
    /// The job still runs.
    Running,
    /// The handle is unknown, so no job runs under it.
    Gone,
}

/// [`JobRunner`] over the daemon's server: the job's request is made from the main checkout,
/// where no tree is leased.
pub struct ServerJobs {
    pub server: Arc<Server>,
    pub repo: PathBuf,
}

impl JobRunner for ServerJobs {
    fn start(&self, job: &IdleJob, named: &mut dyn FnMut(&Handle)) -> Result<Handle> {
        let request = Request {
            verb: IdleJobVerb::NAME.to_owned(),
            args: job.to_args(),
            cwd: self.repo.clone(),
            session: None,
            // The loop waits on the handle itself.
            wait_s: 0,
            pid: None,
        };
        let mut handle = None;
        let reply = self.server.handle_answering(
            &request,
            None,
            None,
            Some(&mut |named_handle: &Handle| {
                named(named_handle);
                handle = Some(named_handle.clone());
            }),
        );
        handle.ok_or(Error::Unreachable(reply.text))
    }

    fn wait(&self, handle: &Handle, seconds: u32) -> JobWait {
        match self.server.handles().wait(handle, seconds, None) {
            Waited::Done(reply) | Waited::Interrupted(reply) => JobWait::Done(reply),
            Waited::Running(_) => JobWait::Running,
            Waited::Unknown => JobWait::Gone,
        }
    }
}

impl Idle {
    /// The file that names the handle of the idle job in progress, beside the state file.
    fn handle_file(&self) -> PathBuf {
        self.settings.state.with_file_name("idle-job.handle")
    }

    /// The handle of the idle job that started and was not seen to finish, if any.
    fn recorded_handle(&self) -> Option<Handle> {
        let text = std::fs::read_to_string(self.handle_file()).ok()?;
        Handle::new(text.trim()).ok()
    }

    fn forget_handle(&self) {
        let _ = std::fs::remove_file(self.handle_file());
    }

    /// One round of the idle loop's work, once it has decided that idle work may run. If a
    /// job's handle is recorded, as after a restart of the daemon, waits for that job to
    /// finish. Otherwise starts the next job, if there is one, and waits for it. Returns the
    /// job's failure, as [`Idle::run`] does for a job in this process.
    pub fn drive(&self, ctx: &Context, runner: &dyn JobRunner) -> Result<()> {
        if let Some(handle) = self.recorded_handle() {
            return match runner.wait(&handle, 0) {
                JobWait::Running => self.await_job(runner, &handle),
                JobWait::Done(reply) => {
                    self.forget_handle();
                    reply_result(&reply)
                }
                JobWait::Gone => {
                    self.forget_handle();
                    Ok(())
                }
            };
        }
        // Other processes saved their changes, which decide what is due.
        self.refresh();
        let Some(job) = self.next_job(ctx) else {
            return Ok(());
        };
        steps::step(format!("idle work {job:?}"));
        let handle = runner.start(&job, &mut |handle| {
            let _ = std::fs::create_dir_all(self.handle_file().parent().unwrap_or(".".as_ref()));
            if let Err(error) = std::fs::write(self.handle_file(), handle.as_str()) {
                eprintln!("ignite: cannot record the idle job {handle}: {error}");
            }
        });
        let handle = match handle {
            Ok(handle) => handle,
            Err(error) => {
                self.forget_handle();
                return Err(error);
            }
        };
        self.await_job(runner, &handle)
    }

    /// Waits until the job of `handle` has finished.
    fn await_job(&self, runner: &dyn JobRunner, handle: &Handle) -> Result<()> {
        steps::step(format!("idle job {handle}"));
        loop {
            match runner.wait(handle, WAIT_S) {
                JobWait::Running => {}
                JobWait::Done(reply) => {
                    self.forget_handle();
                    return reply_result(&reply);
                }
                JobWait::Gone => {
                    self.forget_handle();
                    return Ok(());
                }
            }
        }
    }
}

/// The failure a job's reply says, if it failed.
fn reply_result(reply: &Reply) -> Result<()> {
    if reply.exit == Exit::Success {
        Ok(())
    } else {
        Err(Error::Io(reply.text.clone()))
    }
}

/// The internal verb `idle-job`: runs one idle job, named by the request's arguments (see
/// [`IdleJob::to_args`]), to its end. It runs in a verb process, in the `idle` class.
pub struct IdleJobVerb {
    pub idle: Arc<Idle>,
}

impl IdleJobVerb {
    pub const NAME: &'static str = "idle-job";
}

impl Verb for IdleJobVerb {
    fn name(&self) -> &str {
        Self::NAME
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let Some(job) = IdleJob::from_args(&req.args) else {
            return Reply::text(Exit::Usage, format!("not an idle job: {:?}", req.args));
        };
        // Builds ask the daemon for cores in the class of the verb that asks.
        steps::set_class(Class::Idle);
        match self.idle.run(ctx, &job) {
            Ok(()) => Reply::text(Exit::Success, format!("done   idle work {job:?}")),
            Err(error) => error_reply(&error),
        }
    }

    fn needs_the_tree_lease(&self) -> bool {
        false
    }
}
