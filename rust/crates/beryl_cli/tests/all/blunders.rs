use crate::common::TEST;
use beryl_cli::blunders::*;
use beryl_cli::game::mirror;
use beryl_cli::transcript::labels;
use beryl_engine::{Decision, GameTable, Scratch, observe, view};
use beryl_search::{Agent, AgentDecision, DEEP_SEARCHER};

/// Detects any search, including one that ultimately plays the policy's answer.
struct PolicyOnly(beryl_search::LongSearch);

impl Agent for PolicyOnly {
    fn choose(
        &mut self,
        obs: beryl_engine::Observation<'_>,
        d: &Decision,
        at: AgentDecision,
    ) -> beryl_engine::Choice {
        let choice = self.0.choose(obs, d, at);
        assert!(self.0.last_search().is_none(), "{:?}", d.kind);
        choice
    }
}

#[test]
fn sure_plays_the_policy_at_free_choices() {
    use beryl_engine::DecisionKind;
    use beryl_engine::card::Purpose;

    let family = "pimc72/hidden/rollout/tuned/anchor+guard+sure"
        .parse()
        .unwrap();
    let settings = beryl_search::Settings {
        family,
        work: 100_000,
        threads: 1,
    };
    for (name, kind) in [
        ("decline-mana-crypt", DecisionKind::YesNo),
        ("decline-both-teferi-untaps", DecisionKind::ChooseUpTo),
        ("decline-second-teferi-untap", DecisionKind::ChooseUpTo),
    ] {
        let e = entry(name);
        let at = reach(&TEST, &e).unwrap();
        assert_eq!(at.d.kind, kind, "{name}");
        if kind == DecisionKind::YesNo {
            assert_eq!(at.d.prompt.purpose, Purpose::MaySearch, "{name}");
        }
        let mut greedy = build(&at.t, "greedy", 0).unwrap();
        let expected = play(&e, &at, greedy.as_mut());
        assert!(passes(&e, &expected), "{name}: {expected:?}");
        for index in 0..3 {
            let seed = run_seed(name, index);
            let mut agent = PolicyOnly(beryl_search::LongSearch::new(&at.t, settings, seed));
            assert_eq!(play(&e, &at, &mut agent), expected, "{name} seed {index}");
        }
    }
}

#[test]
fn sure_still_searches_other_decisions() {
    let at = reach(&TEST, &entry("skip-teferi-plus-one")).unwrap();
    let mut scratch = Scratch::new();
    let obs = observe(&at.s, &at.t, &mut scratch, at.d.decider);
    let mut family: beryl_search::Family =
        "pimc72/hidden/rollout/tuned/anchor+guard".parse().unwrap();
    let mut run = |sure| {
        family.options.sure = sure;
        let settings = beryl_search::Settings {
            family,
            work: 100_000,
            threads: 1,
        };
        let mut agent = beryl_search::LongSearch::new(&at.t, settings, 0);
        let choice = agent.choose(obs, &at.d, AgentDecision(0));
        (choice, agent.last_search().unwrap().rollouts)
    };
    let baseline = run(false);
    assert!(baseline.1 > 0);
    assert_eq!(run(true), baseline);
}

#[test]
fn first_keeps_teferis_plus_one_when_search_prefers_to_pass() {
    use beryl_engine::Opt;

    let name = "skip-teferi-plus-one";
    let e = entry(name);
    let at = reach(&TEST, &e).unwrap();
    let family = "pimc72/hidden/rollout/tuned/anchor+guard+paired+first"
        .parse()
        .unwrap();
    let settings = beryl_search::Settings {
        family,
        work: 300_000,
        threads: 1,
    };
    let seed = run_seed(name, 14);
    let mut agent = beryl_search::LongSearch::new(&at.t, settings, seed);
    let choice = play(&e, &at, &mut agent);
    let search = agent.last_search().unwrap();
    let check = search.anchor.expect("paired override check");
    assert_eq!(at.d.candidates.option(check.search), Some(Opt::Pass));
    assert!(check.pairs.is_some_and(|count| count >= 2));
    assert_eq!(choice, ["Teferi, Hero of Dominaria +1"]);
}

#[test]
fn paired_search_keeps_the_default_at_reported_blunders() {
    for (name, rules, index) in [
        ("beza-trinket-into-blockers", "combat", 0),
        ("manifest-into-samurai", "combat", 0),
        ("spirits-stay-home", "hold+combat", 0),
        ("subtlety-main-phase-empty-stack", "hold", 7),
    ] {
        let e = entry(name);
        let at = reach(&TEST, &e).unwrap();
        let family = format!("pimc72/hidden/rollout/tuned/anchor+guard+{rules}+paired")
            .parse()
            .unwrap();
        let settings = beryl_search::Settings {
            family,
            work: 300_000,
            threads: 1,
        };
        let seed = run_seed(name, index);
        let mut agent = beryl_search::LongSearch::new(&at.t, settings, seed);
        let choice = play(&e, &at, &mut agent);
        if name == "subtlety-main-phase-empty-stack" {
            let check = agent.last_search().unwrap().anchor.unwrap();
            assert!(check.pairs.is_some_and(|count| count >= 2));
        }
        let mut default = build(&at.t, &format!("greedy+{rules}"), seed).unwrap();
        assert_eq!(choice, play(&e, &at, default.as_mut()), "{name}");
        assert!(passes(&e, &choice), "{name}: {choice:?}");
    }
}

#[test]
fn paired_checks_are_budgeted_and_thread_independent() {
    let at = reach(&TEST, &entry("beza-trinket-into-blockers")).unwrap();
    let mut scratch = Scratch::new();
    let obs = observe(&at.s, &at.t, &mut scratch, at.d.decider);
    let policy = DEEP_SEARCHER.rollout_policy(&at.t);
    let pairs = beryl_search::PairedRollouts::new(&at.t, &policy, obs, DEEP_SEARCHER.sight, 0);
    let [a, b] = [beryl_engine::Choice(0), beryl_engine::Choice(1)];
    let budget = 150_000;
    let (check, spent) = pairs.check([a, b], budget, 1);
    assert!(check.pairs.unwrap() > 8);
    assert!(spent > 0 && spent <= budget);
    assert_eq!((check, spent), pairs.check([a, b], budget, 8));
    let (same, _) = pairs.check([b, b], budget, 1);
    assert!(same.pairs.unwrap() > 8);
    assert_eq!((same.lead, same.error), (0.0, 0.0));
    let (empty, spent) = pairs.check([a, b], 1, 1);
    assert_eq!((empty.pairs, spent), (Some(0), 0));
    assert_eq!(empty.played(), b);
}

/// An agent that plays the first candidate whose label it lists, else the first.
struct Scripted {
    t: GameTable,
    picks: Vec<&'static str>,
}

impl Agent for Scripted {
    fn choose(
        &mut self,
        obs: beryl_engine::Observation<'_>,
        d: &Decision,
        _: AgentDecision,
    ) -> beryl_engine::Choice {
        let mut x = Scratch::new();
        let root = obs.unmasked_root();
        let shown = labels(&self.t, view(root, &self.t, &mut x), d);
        let i = shown
            .iter()
            .position(|l| self.picks.contains(&l.as_str()))
            .unwrap_or(0);
        beryl_engine::Choice(u16::try_from(i).unwrap())
    }
}

fn entry(name: &str) -> Entry {
    entries()
        .unwrap()
        .into_iter()
        .find(|e| e.name == name)
        .unwrap()
}

fn scripted(e: &Entry, picks: &[&'static str]) -> bool {
    let at = reach(&TEST, e).unwrap();
    let mut agent = Scripted {
        t: mirror(&TEST),
        picks: picks.to_vec(),
    };
    passes(e, &play(e, &at, &mut agent))
}

/// Every entry parses and its record reaches its position, where the game played what
/// the incident says.
#[test]
fn every_entry_reaches_its_position() {
    let all = entries().unwrap();
    assert!(all.len() >= 7);
    for e in &all {
        let at = reach(&TEST, e).unwrap_or_else(|why| panic!("{why}"));
        assert_eq!(at.d.player.get(), e.seat, "{}", e.name);
        assert!(
            root()
                .join("docs/incidents")
                .join(format!("{}.md", e.incident))
                .exists()
        );
        assert!(
            !e.forbidden.is_empty() || !e.required.is_empty(),
            "{}",
            e.name
        );
    }
}

/// A forbidden choice fails its entry and another passes; a required choice passes and a
/// missed one fails, over the several decisions of an attack too.
#[test]
fn scripted_choices_pass_and_fail() {
    let oust = entry("oust-own-token");
    assert!(!scripted(&oust, &["cast Oust"]));
    assert!(scripted(&oust, &["pass"]));
    let crypt = entry("decline-mana-crypt");
    assert!(scripted(&crypt, &["yes", "Mana Crypt"]));
    assert!(!scripted(&crypt, &["yes", "done"]));
    assert!(!scripted(&crypt, &["no"]));
    let jace = entry("spare-jace-wielder");
    assert!(scripted(
        &jace,
        &[
            "attack with Snapcaster Mage",
            "attack with Spirit Token",
            "Jace, Wielder of Mysteries"
        ]
    ));
    assert!(!scripted(
        &jace,
        &["attack with Snapcaster Mage", "Jace, the Mind Sculptor"]
    ));
}

#[test]
fn walker_sends_both_attackers_at_the_planeswalker_that_can_win() {
    let e = entry("spare-jace-wielder");
    let at = reach(&TEST, &e).unwrap();
    let seed = run_seed(&e.name, 0);
    let mut base = build(&at.t, "greedy+hold+waste+combat", seed).unwrap();
    let mut walker = build(&at.t, "greedy+hold+waste+combat+walker", seed).unwrap();
    assert!(!passes(&e, &play(&e, &at, &mut base)));
    assert!(passes(&e, &play(&e, &at, &mut walker)));

    // A zero-budget anchored search should use the same plan through both declarations.
    let family = "pimc2/hidden/rollout/tuned/anchor+hold+waste+combat+walker"
        .parse()
        .unwrap();
    let settings = beryl_search::Settings {
        family,
        work: 0,
        threads: 1,
    };
    let mut search = beryl_search::LongSearch::new(&at.t, settings, seed);
    assert!(passes(&e, &play(&e, &at, &mut search)));
}

#[test]
fn walker_preserves_the_other_combat_answers() {
    for name in [
        "beza-trinket-into-blockers",
        "manifest-into-samurai",
        "spirits-stay-home",
        "skip-beza-after-winds",
    ] {
        let e = entry(name);
        let at = reach(&TEST, &e).unwrap();
        let seed = run_seed(name, 0);
        let mut base = build(&at.t, "greedy+hold+waste+combat", seed).unwrap();
        let mut walker = build(&at.t, "greedy+hold+waste+combat+walker", seed).unwrap();
        let expected = play(&e, &at, &mut base);
        assert!(passes(&e, &expected), "{name}: {expected:?}");
        assert_eq!(play(&e, &at, &mut walker), expected, "{name}");
    }
}

#[test]
fn combat_planner_holds_back_at_the_two_blocker_positions() {
    for name in ["beza-trinket-into-blockers", "manifest-into-samurai"] {
        let at = reach(&TEST, &entry(name)).unwrap();
        let seed = run_seed(name, 0);
        let family = "pimc2/hidden/rollout/tuned/anchor+combat".parse().unwrap();
        let mut scratch = Scratch::new();
        let obs = observe(&at.s, &at.t, &mut scratch, at.d.decider);
        let shown = labels(&at.t, view(&at.s, &at.t, &mut Scratch::new()), &at.d);
        let settings = |work| beryl_search::Settings {
            family,
            work,
            threads: 1,
        };

        // With no budget for an iteration, the guarded decision uses the default.
        let mut planner = beryl_search::LongSearch::new(&at.t, settings(0), seed);
        let choice = planner.choose(obs, &at.d, AgentDecision(0));
        assert_eq!(planner.last_search().unwrap().rollouts, 0);
        assert_eq!(shown[usize::from(choice.0)], "done", "{name}");
        let mut policy = build(&at.t, "greedy+combat", seed).unwrap();
        assert_eq!(
            policy.choose(obs, &at.d, AgentDecision(0)),
            choice,
            "{name}"
        );

        let mut first = beryl_search::LongSearch::new(&at.t, settings(20_000), seed);
        let mut second = beryl_search::LongSearch::new(&at.t, settings(20_000), seed);
        assert_eq!(
            first.choose(obs, &at.d, AgentDecision(0)),
            second.choose(obs, &at.d, AgentDecision(0)),
            "{name}"
        );
    }
}

/// Reviewed hunt positions reject the recorded mistake but retain a useful alternative.
#[test]
fn reviewed_hunt_choices_and_drift() {
    for (name, bad, good) in [
        (
            "discard-emperor-over-spare-crypt",
            "The Wandering Emperor",
            "Mana Crypt",
        ),
        ("decline-second-teferi-untap", "done", "Island"),
        (
            "evoke-subtlety-empty-stack-hunt",
            "cast Subtlety for evoke",
            "pass",
        ),
    ] {
        let mut e = entry(name);
        assert!(!scripted(&e, &[bad]), "{name}");
        assert!(scripted(&e, &[good]), "{name}");
        e.played = "not the recorded choice".to_owned();
        let why = reach(&TEST, &e)
            .err()
            .expect("changed choice must report drift");
        assert!(why.contains(name), "{why}");
        assert!(why.contains(&e.incident), "{why}");
    }
}

/// A record that no longer plays what the incident says is reported with its incident.
#[test]
fn a_drifted_replay_names_its_incident() {
    let mut e = entry("evoke-subtlety-no-target");
    e.played = "pass".to_owned();
    let why = reach(&TEST, &e).err().unwrap();
    assert!(why.contains("2026-11-08-subtlety-spirits-samurai"), "{why}");
    let mut e = entry("oust-own-token");
    e.turn = 13;
    assert!(reach(&TEST, &e).is_err());
}

#[test]
fn runs_are_judged_by_their_choices() {
    let e = entry("spare-jace-wielder");
    let both = [
        "attack with Spirit Token",
        "Jace, Wielder of Mysteries",
        "attack with Snapcaster Mage",
        "Jace, Wielder of Mysteries",
    ]
    .map(str::to_owned);
    assert!(passes(&e, &both));
    assert!(!passes(&e, &both[..3]));
    assert_ne!(run_seed("a", 0), run_seed("b", 0));
    assert_ne!(run_seed("a", 0), run_seed("a", 1));
}
