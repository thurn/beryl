//! The verb process's view of its daemon: one method per call of the host protocol
//! ([`crate::protocol::host`]).
//!
//! The real client sends each call over the daemon's socket, one connection per call; the fake
//! in [`crate::fakes::FakeHost`] answers in memory. A method fails with
//! [`Error::Unreachable`](crate::traits::Error::Unreachable) when the daemon cannot be reached,
//! as while it restarts; the caller then sends `hello`, its current step, and the same call
//! again, with the same body, for up to five minutes.

use crate::protocol::host::{
    Ack, AllowStop, Build, BuildReply, Finished, Hello, HelloReply, LeaseLanding,
    LeaseLandingReply, Poll, RefuseStop, RefuseStopReply, ReleaseLanding, Run, RunReply,
    StepUpdate, StopState,
};
use crate::traits::Result;

pub trait HostClient: Send + Sync {
    /// `host-hello`.
    fn hello(&self, call: &Hello) -> Result<HelloReply>;
    /// `host-run`; answers `waiting` after at most 30 s.
    fn run(&self, call: &Run) -> Result<RunReply>;
    /// `host-build`; answers `waiting` after at most 30 s.
    fn build(&self, call: &Build) -> Result<BuildReply>;
    /// `host-lease-landing`.
    fn lease_landing(&self, call: &LeaseLanding) -> Result<LeaseLandingReply>;
    /// `host-release-landing`.
    fn release_landing(&self, call: &ReleaseLanding) -> Result<Ack>;
    /// `host-step`.
    fn step(&self, call: &StepUpdate) -> Result<StopState>;
    /// `host-refuse-stop`.
    fn refuse_stop(&self, call: &RefuseStop) -> Result<RefuseStopReply>;
    /// `host-allow-stop`.
    fn allow_stop(&self, call: &AllowStop) -> Result<Ack>;
    /// `host-poll`.
    fn poll(&self, call: &Poll) -> Result<StopState>;
    /// `host-finished`.
    fn finished(&self, call: &Finished) -> Result<Ack>;
}
