//! Where a landing's time went, for its reply: each pass that did not land and why it ended,
//! the runs that daemon restarts cut off, and the last pass.
//!
//! A landing's reply once described only its last gate. On 2026-11-20 a landing of a one-card
//! comment change ran for 13.4 minutes and its reply said "took 28 s": the tip had moved during
//! earlier passes, and five restarts of the daemon into new releases had each cut the landing
//! off and run it again from its start under the same handle, and the reply named none of it.
//! The reply now says how long the landing took since its handle began, and where that time
//! went.

use ignite_core::protocol::client::Reply;
use ignite_core::time::Timestamp;
use serde_json::{Value, json};

use super::env::LandGate;
use super::line;
use crate::checks::verbs::render;
use crate::server::Context;
use crate::steps;

/// The passes a landing has run in this run, and when the landing began.
pub(super) struct Timeline {
    /// When the landing's handle began, which is before `run_began` when a daemon restart cut
    /// off an earlier run of it.
    began: Timestamp,
    /// When this run of the landing began.
    run_began: Timestamp,
    /// When the pass running now began, once one has.
    pass_began: Option<Timestamp>,
    /// Each pass that ended without landing: its seconds and why it ended.
    ended: Vec<(f64, String)>,
}

impl Timeline {
    /// The timeline of a landing whose run begins now.
    pub(super) fn start(ctx: &Context, gate: &dyn LandGate) -> Timeline {
        let now = ctx.clock.now();
        let began = steps::current()
            .and_then(|activity| activity.handle())
            .and_then(|handle| gate.began(&handle))
            .filter(|began| *began <= now)
            .unwrap_or(now);
        Timeline {
            began,
            run_began: now,
            pass_began: None,
            ended: Vec::new(),
        }
    }

    /// A new pass begins. The one before it, if any, ended without landing because of `cause`.
    pub(super) fn next_pass(&mut self, ctx: &Context, cause: &str) {
        let now = ctx.clock.now();
        if let Some(began) = self.pass_began.replace(now) {
            self.ended
                .push((now.seconds_since(began).max(0.0), cause.to_owned()));
        }
    }

    /// The lines that account for the landing's time up to `now`, given that the last pass's
    /// gate, or express run, took `gate_s` seconds.
    pub(super) fn lines(&self, now: Timestamp, gate_s: f64) -> Vec<(String, String)> {
        let total = now.seconds_since(self.began).max(0.0);
        let cut_off = self.run_began.seconds_since(self.began).max(0.0);
        let earlier: f64 = self.ended.iter().map(|(seconds, _)| seconds).sum();
        let last = self
            .pass_began
            .map_or(0.0, |began| now.seconds_since(began).max(0.0));
        let mut lines = Vec::new();
        for (index, (seconds, cause)) in self.ended.iter().enumerate() {
            lines.push(line(
                "pass",
                format!("{} took {seconds:.0} s and ended: {cause}", index + 1),
            ));
        }
        let mut parts = Vec::new();
        if cut_off >= 1.0 {
            parts.push(format!(
                "{cut_off:.0} s in earlier runs that daemon restarts cut off"
            ));
        }
        if !self.ended.is_empty() {
            let passes = if self.ended.len() == 1 { "pass" } else { "passes" };
            parts.push(format!(
                "{earlier:.0} s in the {} {passes} before it",
                self.ended.len()
            ));
        }
        parts.push(format!(
            "{last:.0} s in pass {} ({gate_s:.0} s gating, {:.0} s in its other steps: \
             rebasing, formatting, the lease, the push)",
            self.ended.len() + 1,
            (last - gate_s).max(0.0)
        ));
        lines.push(line(
            "landing",
            format!("took {total:.0} s: {}", parts.join(", ")),
        ));
        lines
    }

    /// The same account as JSON.
    pub(super) fn json(&self, now: Timestamp) -> Value {
        json!({
            "took_s": now.seconds_since(self.began).max(0.0),
            "cut_off_s": self.run_began.seconds_since(self.began).max(0.0),
            "passes": self.ended.iter().map(|(seconds, cause)| json!({
                "took_s": seconds, "ended": cause,
            })).chain(std::iter::once(json!({
                "took_s": self.pass_began.map_or(0.0, |b| now.seconds_since(b).max(0.0)),
            }))).collect::<Vec<_>>(),
        })
    }

    /// `reply`, a refusal at `now`, with the account inserted before its `rerun` or `next` line
    /// and added to its JSON.
    pub(super) fn onto(&self, now: Timestamp, mut reply: Reply, gate_s: f64) -> Reply {
        let added = render(&self.lines(now, gate_s));
        let at = ["rerun ", "next "]
            .iter()
            .filter_map(|label| {
                reply
                    .text
                    .match_indices(&format!("\n{label}"))
                    .next()
                    .map(|(at, _)| at + 1)
            })
            .min()
            .unwrap_or(reply.text.len());
        reply.text.insert_str(at, &added);
        if let Some(Value::Object(fields)) = reply.json.as_mut() {
            fields.insert("landing".to_owned(), self.json(now));
        }
        reply
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_core::protocol::client::Exit;

    fn at(seconds: f64) -> Timestamp {
        Timestamp::parse("2026-11-20T00:26:08Z")
            .unwrap()
            .plus_seconds(seconds)
    }

    /// The refused landing of the report: begun 776 s before its last run by restarts, one pass
    /// lost to the tip, and a last pass whose gate took 28 s. The reply accounts for all 804 s.
    #[test]
    fn a_refusal_accounts_for_restarts_and_every_pass() {
        let timeline = Timeline {
            began: at(0.0),
            run_began: at(706.0),
            pass_began: Some(at(776.0)),
            ended: vec![(70.0, "the tip moved from 5955ef1 to 7520466".to_owned())],
        };
        let refused = Reply {
            exit: Exit::CheckFailed,
            text: "gate     1 failed\nrerun    ignite test x\nnext     fix it\n".to_owned(),
            json: Some(json!({ "failures": [] })),
            handle: None,
        };

        let reply = timeline.onto(at(804.0), refused, 28.0);

        assert_eq!(
            reply.text,
            "gate     1 failed\n\
             pass     1 took 70 s and ended: the tip moved from 5955ef1 to 7520466\n\
             landing  took 804 s: 706 s in earlier runs that daemon restarts cut off, 70 s in the \
             1 pass before it, 28 s in pass 2 (28 s gating, 0 s in its other steps: rebasing, \
             formatting, the lease, the push)\n\
             rerun    ignite test x\n\
             next     fix it\n"
        );
        let landing = &reply.json.unwrap()["landing"];
        assert_eq!(landing["took_s"], 804.0);
        assert_eq!(landing["cut_off_s"], 706.0);
        assert_eq!(landing["passes"].as_array().unwrap().len(), 2);
    }

    /// A landing that lands on its first pass, in its only run, has one line.
    #[test]
    fn a_landing_in_one_pass_has_one_line() {
        let timeline = Timeline {
            began: at(0.0),
            run_began: at(0.0),
            pass_began: Some(at(0.0)),
            ended: Vec::new(),
        };
        assert_eq!(
            timeline.lines(at(40.0), 31.0),
            [line(
                "landing",
                "took 40 s: 40 s in pass 1 (31 s gating, 9 s in its other steps: rebasing, \
                 formatting, the lease, the push)"
            )]
        );
    }
}
