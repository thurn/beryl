//! Telling the hub when a tip waits on checks only another platform's machines can run (see
//! "Tip verification" in `docs/ignite.md`).

use std::collections::BTreeMap;

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::hub::TipWaiting;
use ignite_core::traits::Result;

use super::{MACHINE_WAIT_S, TipVerifier};
use crate::server::Context;

impl TipVerifier {
    /// Starts the wait for each platform `tip` waits on that no earlier tip was waiting on,
    /// forgets platforms it no longer waits on, and reports each platform waited on for
    /// [`MACHINE_WAIT_S`], once per tip.
    ///
    /// A platform none of whose declared machines lists a waited check's family in its
    /// `families` can never pass those checks, so waiting an hour tells nobody anything: it is
    /// reported at once, naming the families, and returned as one line per platform and
    /// family for the round's outcome.
    pub(super) fn flag_missing_machines(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        elsewhere: BTreeMap<Os, Vec<(CheckName, String)>>,
    ) -> Result<Vec<String>> {
        let now = ctx.clock.now();
        self.waiting_since
            .retain(|os, _| elsewhere.contains_key(os));
        let mut lines = Vec::new();
        for (platform, waited) in elsewhere {
            let since = *self.waiting_since.entry(platform).or_insert(now);
            let waited_s = now.seconds_since(since);
            let mut unrunnable: Vec<String> = waited
                .iter()
                .filter(|(_, family)| !ctx.config.runs_family(Some(platform), family))
                .map(|(_, family)| family.clone())
                .collect();
            unrunnable.sort();
            unrunnable.dedup();
            lines.extend(unrunnable.iter().map(|family| {
                format!(
                    "family {family} must pass on {os}, but no {os} machine in ignite.toml \
                     lists it in `families`",
                    os = platform.as_str()
                )
            }));
            let due = waited_s >= MACHINE_WAIT_S || !unrunnable.is_empty();
            if !due || self.flagged.contains(&(tip.clone(), platform)) {
                continue;
            }
            let mut checks: Vec<CheckName> = waited.into_iter().map(|(check, _)| check).collect();
            checks.sort();
            checks.dedup();
            ctx.hub.tip_waiting(&TipWaiting {
                commit: tip.clone(),
                machine: ctx.machine.clone(),
                platform,
                checks,
                waited_s,
                unrunnable,
            })?;
            self.flagged.insert((tip.clone(), platform));
        }
        Ok(lines)
    }
}
