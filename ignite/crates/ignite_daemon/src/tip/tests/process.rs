//! A round as the internal verb `tip-round`: what it answers, how it remembers between rounds,
//! and the claims it renews while a batch runs.

use std::path::Path;
use std::time::Duration;

use ignite_core::protocol::client::Exit;

use super::*;
use crate::server::Verb;
use crate::tip::round::RoundReply;

/// [`FakeWork`] whose runs each take `pause` of real time, as a batch that runs long does.
struct Slow {
    work: Arc<FakeWork>,
    pause: Duration,
}

impl TipWork for Slow {
    fn checks(&self, ctx: &Context, commit: &Commit) -> Result<TipSet> {
        self.work.checks(ctx, commit)
    }

    fn run(&self, ctx: &Context, at: &Commit, checks: &[CheckName], rerun: bool) -> Vec<TipRun> {
        std::thread::sleep(self.pause);
        self.work.run(ctx, at, checks, rerun)
    }

    fn landings(&self, ctx: &Context, from: &Commit, to: &Commit) -> Result<Vec<TipLanding>> {
        self.work.landings(ctx, from, to)
    }
}

/// A batch that runs longer than the renewal period has its claim asked for again while it runs,
/// and no more once the round has finished.
#[test]
fn a_running_batch_has_its_claim_renewed_until_it_finishes() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    world.land(&tip);
    world.work.set_checks(&tip, vec![check("a", 'a')]);
    let slow = Slow {
        work: world.work.clone(),
        pause: Duration::from_millis(400),
    };
    let mut verifier =
        TipVerifier::new(Arc::new(slow), "test").renewing_claims_every(Duration::from_millis(50));

    assert_eq!(verifier.verify(&ctx, &tip).unwrap(), TipOutcome::Verified);

    let asks = world.hub.tip_claim_asks();
    assert!(asks.len() >= 3, "the claim was renewed: {asks:?}");
    assert!(
        asks.iter()
            .all(|ask| ask.batch == 0 && ask.family == "unit")
    );
    assert_eq!(world.hub.tip_claims().len(), 1, "one batch was claimed");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(world.hub.tip_claim_asks().len(), asks.len());
}

/// The verb answers with the tip and how it ended, and a tip it settled is not due again for
/// the next verifier built over the same memory.
#[test]
fn a_round_answers_with_its_outcome_and_settles_the_tip_in_its_memory() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    world.land(&tip);
    world.work.set_checks(&tip, vec![check("a", 'a')]);
    let dir = ignite_core::test_files::temp_dir().join(format!("ignite-tip-round-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let verb = TipRoundVerb::new(
        world.work.clone(),
        "test",
        "jarvis",
        Some(dir.join("daemon.db")),
    );
    assert!(verb.verifier().due(&ctx, &tip));

    let reply = verb.run(
        &ctx,
        &TipRoundVerb::request(&tip, Seq(7), Path::new("/repo")),
    );

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    let read: RoundReply = serde_json::from_value(reply.json.unwrap()).unwrap();
    assert_eq!(
        read,
        RoundReply {
            tip: tip.clone(),
            outcome: TipOutcome::Verified
        }
    );
    let after = verb.verifier();
    assert!(!after.due(&ctx, &tip));
    assert_eq!(after.since(), Seq(7));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A request without a commit and a feed position is refused before anything runs.
#[test]
fn a_round_without_its_arguments_runs_nothing() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let verb = TipRoundVerb::new(world.work.clone(), "test", "jarvis", None);
    let mut request = TipRoundVerb::request(&commit("bbbb000"), Seq(0), Path::new("/repo"));
    request.args.pop();
    let reply = verb.run(&ctx, &request);
    assert_ne!(reply.exit, Exit::Success);
    assert!(world.work.runs().is_empty());
}
