//! The greedy agent and the rollout policy behind it (architecture §19.1, decision D21), in
//! the WU Control mirror.

mod common;

use beryl_cards::{CARDS, names};
use beryl_engine::card::{ObjectAbility, Purpose};
use beryl_engine::{
    Answers, Candidates, Choice, Decision, DecisionKind, GameTable, Observation, Opt, Outcome,
    Profile, Replay, Scratch, Seat, State, Step, TurnStep, Zone, apply, next,
};
use beryl_scenario::{Action, Live, P0, P1, Permanent, Scenario};
use beryl_search::{Agent, GreedyAgent, Inline, RandomAgent, RolloutPolicy};
use common::{fingerprint, mirror, play};

/// Greedy against random, seats alternated: at least 90%. The CLI's 200-game tournament
/// measures the gate; this pins a smaller seeded sample so a weakened policy fails here.
#[test]
fn greedy_beats_random() {
    let t = mirror();
    let mut wins = 0;
    let games = 40;
    for seed in 0..games {
        let greedy = if seed % 2 == 0 { Seat::P0 } else { Seat::P1 };
        let mut agents: [Box<dyn Agent>; 2] = [
            Box::new(GreedyAgent::new(&t)),
            Box::new(RandomAgent::new(seed)),
        ];
        if greedy == Seat::P1 {
            agents.swap(0, 1);
        }
        let mut s = State::new(&t, seed, Profile::SMART).unwrap();
        let outcome = play(&t, &mut s, &mut Scratch::new(), &mut agents, |_, _| {});
        if outcome == Outcome::Win(greedy) {
            wins += 1;
        }
        assert!(
            !matches!(outcome, Outcome::Stalled(_)),
            "seed {seed}: {outcome:?}"
        );
    }
    assert!(wins * 10 >= games * 9, "greedy won {wins} of {games}");
}

/// The inline answers, recorded in the order they were given.
struct Recording<'a> {
    inline: Inline<'a>,
    answers: Vec<Choice>,
}

impl Answers for Recording<'_> {
    fn inline(&self) -> bool {
        true
    }

    fn answer(&mut self, obs: Observation<'_>, d: &Decision) -> Option<Choice> {
        let c = self.inline.answer(obs, d);
        self.answers.extend(c);
        c
    }
}

/// A rollout answers mid-resolution questions inline, running each body once on the
/// committed state; replaying the same answers through pauses must reach the same game
/// (§7.3, §17.4).
#[test]
fn inline_answers_replay_to_the_same_game() {
    let t = mirror();
    let policy = RolloutPolicy::new(&t);
    let mut inlined = 0;
    for seed in 0..8 {
        let mut s = State::new(&t, seed, Profile::SMART).unwrap();
        let mut x = Scratch::new();
        let mut asked = 0;
        let mut rec = Recording {
            inline: Inline {
                policy: &policy,
                t: &t,
            },
            answers: Vec::new(),
        };
        let outcome = loop {
            match next(&mut s, &t, &mut x, &mut rec) {
                Step::Over(outcome) => break outcome,
                Step::Decide(d) => {
                    asked += 1;
                    let c = policy.choose(&t, Observation::new(&s, d.player), &d);
                    rec.answers.push(c);
                    apply(&mut s, &t, &mut x, &mut rec, c);
                }
            }
        };
        let (final_hash, answers) = (fingerprint(&s), rec.answers);
        inlined += answers.len() - asked;

        let mut r = State::new(&t, seed, Profile::SMART).unwrap();
        let mut x = Scratch::new();
        let mut answers = answers.into_iter();
        let replayed = loop {
            match next(&mut r, &t, &mut x, &mut Replay) {
                Step::Over(outcome) => break outcome,
                Step::Decide(_) => {
                    let c = answers.next().expect("as many answers as decisions");
                    apply(&mut r, &t, &mut x, &mut Replay, c);
                }
            }
        };
        assert_eq!(answers.next(), None, "seed {seed}: answers left over");
        assert_eq!(replayed, outcome, "seed {seed}");
        assert_eq!(fingerprint(&r), final_hash, "seed {seed}");
    }
    // Bodies really asked questions inline: the test has something to compare.
    assert!(inlined > 8, "{inlined} inline answers");
}

/// Aang's Iceberg's waterbend scries 2 over a Plains and Jace, the Mind Sculptor, with six
/// lands out: the inline answers send the Plains to the bottom and keep Jace on top, ahead of
/// the Island under them.
#[test]
fn a_rollout_scry_keeps_the_card_worth_drawing() {
    use names::{AANGS_ICEBERG, ISLAND, JACE_THE_MIND_SCULPTOR, PLAINS};
    let deck = [
        (1, AANGS_ICEBERG),
        (4, PLAINS),
        (4, ISLAND),
        (1, JACE_THE_MIND_SCULPTOR),
    ];
    let t = GameTable::new(&CARDS, [&deck, &[(1, PLAINS)]]).unwrap();
    let mut s = State::new(&t, 0, Profile::SMART).unwrap();
    s.board().begin_at(3, Seat::P0, TurnStep::PrecombatMain);
    let named = |s: &State, name: &str| {
        s.library(Seat::P0)
            .iter()
            .copied()
            .find(|&slot| s.definition_name(&t, slot) == name)
            .unwrap()
    };
    let iceberg = named(&s, AANGS_ICEBERG);
    s.board().place(iceberg, Zone::Battlefield).unwrap();
    for name in [PLAINS, PLAINS, PLAINS, ISLAND, ISLAND, ISLAND] {
        let slot = named(&s, name);
        s.board().place(slot, Zone::Battlefield).unwrap();
    }
    let top_first = [
        named(&s, PLAINS),
        named(&s, JACE_THE_MIND_SCULPTOR),
        named(&s, ISLAND),
    ];
    s.board().order_library(Seat::P0, &top_first);

    let policy = RolloutPolicy::new(&t);
    let mut x = Scratch::new();
    let mut inline = Inline {
        policy: &policy,
        t: &t,
    };
    let waterbend = Opt::Activate {
        slot: iceberg,
        ability: ObjectAbility::own(1),
    };
    let mut activated = false;
    while s.object(iceberg).zone() != Zone::Graveyard {
        let Step::Decide(d) = next(&mut s, &t, &mut x, &mut inline) else {
            panic!("the game ended");
        };
        let offered = match &d.candidates {
            Candidates::Options(o) => o.iter().position(|&o| o == waterbend),
            Candidates::Objects(_) => None,
        };
        let c = match offered {
            Some(i) if !activated && d.kind == DecisionKind::Priority => {
                activated = true;
                Choice(i as u16)
            }
            _ => policy.choose(&t, Observation::new(&s, d.player), &d),
        };
        apply(&mut s, &t, &mut x, &mut inline, c);
    }
    let top_first: Vec<&str> = s
        .library(Seat::P0)
        .iter()
        .rev()
        .map(|&slot| s.definition_name(&t, slot))
        .collect();
    assert_eq!(top_first, [JACE_THE_MIND_SCULPTOR, ISLAND, PLAINS]);
}

/// The policy's answer at the decision `scenario`'s actions lead to, which must be for
/// `purpose`: the card it picks, by name, or else the option.
fn answer_for(scenario: &Scenario, purpose: Purpose) -> (Option<&'static str>, Option<Opt>) {
    let mut live = Live::new(&CARDS, scenario).unwrap();
    for &action in scenario.actions {
        live.act(action).unwrap();
    }
    let Step::Decide(d) = live.position(Action::pass(P0)).unwrap() else {
        panic!("the game ended");
    };
    assert_eq!(d.prompt.purpose, purpose, "{d:?}");
    let policy = RolloutPolicy::new(live.table());
    let c = policy.choose(live.table(), Observation::new(live.state(), d.player), &d);
    (live.name_of(&d, c), d.candidates.option(c))
}

/// A crew cost is paid with the creature least worth keeping untapped, where any other
/// choice of a permanent takes the one worth most: of a Snapcaster Mage, a Trinket Mage and
/// an Elite Spellbinder, the policy values the Trinket Mage least and the Spellbinder most.
#[test]
fn a_crew_cost_taps_the_least_valuable_creature() {
    use names::{
        ELITE_SPELLBINDER, JACE_VRYNS_PRODIGY, PLAINS, SNAPCASTER_MAGE, SWIFT_RECONFIGURATION,
        TRINKET_MAGE,
    };
    const CREW: Scenario = Scenario::new("crew")
        .at(3, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(JACE_VRYNS_PRODIGY),
                Permanent::new(SNAPCASTER_MAGE),
                Permanent::new(TRINKET_MAGE),
                Permanent::new(ELITE_SPELLBINDER),
                Permanent::new(PLAINS),
            ],
        )
        .hand(P0, &[SWIFT_RECONFIGURATION])
        .library(P0, &[PLAINS; 4])
        .library(P1, &[PLAINS; 4])
        .actions(&[
            Action::cast(P0, SWIFT_RECONFIGURATION),
            Action::choose(P0, DecisionKind::Target, JACE_VRYNS_PRODIGY),
            Action::Resolve,
            Action::activate_granted(P0, JACE_VRYNS_PRODIGY, 0),
        ]);
    let (card, _) = answer_for(&CREW, Purpose::Crew);
    assert_eq!(card, Some(TRINKET_MAGE));
}

/// The life a land asks so as to enter untapped is paid only from above 10 life, where any
/// other "may" is taken.
#[test]
fn life_for_an_untapped_land_is_paid_only_from_above_ten() {
    const SPRINGS: Scenario = Scenario::new("pay life")
        .hand(P0, &[names::SINK_INTO_STUPOR])
        .library(P1, &[names::PLAINS])
        .actions(&[Action::PlayLand {
            player: P0,
            card: names::SINK_INTO_STUPOR,
        }]);
    let pays = |life| answer_for(&SPRINGS.life(P0, life), Purpose::PayLife).1;
    assert_eq!(pays(11), Some(Opt::Yes));
    assert_eq!(pays(10), Some(Opt::No));
}
