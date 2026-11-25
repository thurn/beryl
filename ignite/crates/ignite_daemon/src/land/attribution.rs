//! Historical attribution of a failure a landing already confirmed at its exact base. The hub
//! keeps the pending search. Each attempt runs as an internal verb with its own tree leases.

use std::sync::Arc;

use ignite_core::ids::{Commit, Handle};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::{LandingAttribution, RegressionAttribution, RegressionReport};
use ignite_core::protocol::{Class, Outcome};
use ignite_core::tipround::CulpritSearch;
use ignite_core::traits::{Error, Result};

use super::Landing;
use crate::checks::RunOptions;
use crate::handles::Waited;
use crate::server::{Context, Server, Verb, error_reply};
use crate::steps;
use crate::tip::{bisect, ticketed_landings, with_culprit};

pub(crate) const NAME: &str = "landing-attribution";

pub(crate) struct AttributionVerb {
    pub landing: Arc<Landing>,
    pub landing_trees: usize,
}

impl AttributionVerb {
    /// Starts one queued search at a time, without waiting for its historical checks.
    pub fn drive(
        self: &Arc<Self>,
        ctx: &Context,
        server: &Server,
        active: &mut Option<Handle>,
    ) -> Result<()> {
        if let Some(handle) = active
            .take()
            .or_else(|| server.running_internal(NAME).map(|(handle, _)| handle))
        {
            let handles = server.handles();
            match handles.wait(&handle, 0, None) {
                Waited::Running(_) => {
                    *active = Some(handle);
                    return Ok(());
                }
                Waited::Done(reply) | Waited::Interrupted(reply) => {
                    handles.delivered(&handle);
                    if reply.exit != Exit::Success {
                        return Err(Error::Io(reply.text));
                    }
                }
                Waited::Unknown => {}
            }
        }
        if let Some(pending) = ctx.hub.next_landing_attribution()? {
            *active = Some(server.start_internal(self.clone(), Self::request(ctx, &pending)?));
        }
        Ok(())
    }

    pub fn request(ctx: &Context, pending: &LandingAttribution) -> Result<Request> {
        Ok(Request {
            verb: NAME.to_owned(),
            args: vec![serde_json::to_string(pending).map_err(|e| Error::Invalid(e.to_string()))?],
            cwd: ctx.repo.clone(),
            session: None,
            wait_s: 0,
            pid: None,
        })
    }

    /// Returns false if the search needs a spare landing tree and none is free yet.
    fn attribute(&self, ctx: &Context, pending: LandingAttribution) -> Result<bool> {
        let report = pending.baseline.report;
        steps::step(format!("searching for the culprit of {}", report.check));
        let range = match self
            .landing
            .trees
            .commit_paths(&ctx.repo, &report.last_good, &report.first_bad)
            .and_then(|commits| ticketed_landings(ctx, commits))
        {
            Err(error @ Error::Unreachable(_)) => return Err(error),
            range => range,
        };
        let mut busy = false;
        let culprit = range.map(|landings| {
            bisect(
                &landings,
                &pending.baseline.inputs,
                &report.last_good,
                |at| {
                    if steps::stop_requested() {
                        return Some(Outcome::Incomplete);
                    }
                    match self.probe(ctx, &report, at) {
                        Ok(Probe::Checked(outcome)) => outcome,
                        Ok(Probe::Busy) => {
                            busy = true;
                            None
                        }
                        Err(error) => {
                            eprintln!("ignite: probing {} at {at}: {error}", report.check);
                            Some(Outcome::Incomplete)
                        }
                    }
                },
            )
        });
        if steps::stop_requested() || (busy && self.landing_trees > 1) {
            return Ok(false);
        }
        let report = if busy {
            let mut report = report;
            report.search = Some(CulpritSearch::NotRun {
                why: "this machine has no spare landing tree for historical checks".to_owned(),
            });
            report
        } else {
            with_culprit(report, culprit)
        };
        ctx.hub.attribute_regression(&RegressionAttribution {
            ticket: pending.ticket,
            report,
        })?;
        Ok(true)
    }

    fn probe(&self, ctx: &Context, report: &RegressionReport, at: &Commit) -> Result<Probe> {
        let Some(tree) = self.landing.trees.lease_landing_keeping(NAME, at, 1)? else {
            return Ok(Probe::Busy);
        };
        let options = RunOptions {
            verb: NAME.to_owned(),
            class: Class::Tip,
            rerun: false,
            ticket: None,
            session: None,
            reserve: None,
        };
        let outcome = self
            .landing
            .gate
            .run_checks(ctx, &tree, std::slice::from_ref(&report.check), &options)
            .into_iter()
            .find(|probe| probe.check == report.check && probe.platform == report.platform)
            .map(|probe| probe.outcome);
        self.landing.trees.release_landing(&tree.tree, Some(at))?;
        Ok(Probe::Checked(outcome))
    }
}

enum Probe {
    Checked(Option<Outcome>),
    Busy,
}

impl Verb for AttributionVerb {
    fn name(&self) -> &str {
        NAME
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        steps::set_class(Class::Tip);
        let [pending] = req.args.as_slice() else {
            return Reply::text(Exit::Usage, "expected a pending landing attribution");
        };
        let pending = match serde_json::from_str(pending) {
            Ok(pending) => pending,
            Err(error) => return error_reply(&Error::Invalid(error.to_string())),
        };
        match self.attribute(ctx, pending) {
            Ok(true) => Reply::text(Exit::Success, "recorded historical attribution"),
            Ok(false) => Reply::text(Exit::Success, "historical attribution remains queued"),
            Err(error) => error_reply(&error),
        }
    }
}
