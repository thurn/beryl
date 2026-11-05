//! The expert system's card knowledge, pinned on hand-built positions (Milestone 1 decision
//! 6), and its strength against the random agent.

mod common;

use beryl_cards::{CARDS, names};
use beryl_engine::card::AbilityIx;
use beryl_engine::{
    Candidates, Choice, Decision, DecisionKind, GameTable, Observation, Opt, Outcome, Profile,
    Replay, Scratch, Seat, Slot, State, Step, TurnStep, apply, next, view,
};
use beryl_scenario::{P0, P1, Permanent, Scenario, position};
use beryl_search::{Agent, ExpertAgent, RandomAgent};
use common::{mirror, play};

const ISLAND: &str = names::ISLAND;
const PLAINS: &str = names::PLAINS;

/// Libraries deep enough that no one decks in these positions.
const LIBRARY: &[&str] = &[PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND];

/// The game's position from `scenario`, under the smart stops the agents play with.
fn arrange(scenario: &Scenario) -> (GameTable, State) {
    let (t, mut s) = position(&CARDS, scenario).unwrap();
    s.set_profile(Profile::SMART);
    (t, s)
}

fn name(t: &GameTable, s: &State, slot: Slot) -> &'static str {
    t.def(s.card(slot)).face_name(s.object(slot).face)
}

/// Plays on from `s`, the opponent (P1) answering with `script`, until the expert (P0) faces
/// a decision `stop` accepts; returns that decision and the expert's answer, the state left
/// at it.
fn expert_at(
    t: &GameTable,
    s: &mut State,
    mut script: impl FnMut(&GameTable, &State, &Decision) -> Choice,
    stop: impl Fn(&State, &Decision) -> bool,
) -> (Decision, Choice) {
    let mut expert = ExpertAgent::new(t, 7);
    let mut x = Scratch::new();
    loop {
        let Step::Decide(d) = next(s, t, &mut x, &mut Replay) else {
            panic!("the game ended before the expert's decision");
        };
        let shown = *view(s, t, &mut x);
        let c = if d.player == P1 {
            script(t, &shown, &d)
        } else {
            let c = expert.choose(Observation::new(&shown, P0), &d);
            if stop(&shown, &d) {
                return (d, c);
            }
            c
        };
        apply(s, t, &mut x, &mut Replay, c);
    }
}

/// The opponent casts the card named `card` the first time it can, and otherwise takes the
/// first option: passing, the first plan, the first target.
fn casts(card: &'static str) -> impl FnMut(&GameTable, &State, &Decision) -> Choice {
    move |t, s, d| match &d.candidates {
        Candidates::Options(o) if d.kind == DecisionKind::Priority => o
            .iter()
            .position(|&o| matches!(o, Opt::Cast { slot, .. } if name(t, s, slot) == card))
            .map_or(Choice(0), |i| Choice(i as u8)),
        _ => Choice(0),
    }
}

/// The expert's priority with a spell of the opponent's on the stack.
fn facing_a_spell(s: &State, d: &Decision) -> bool {
    d.kind == DecisionKind::Priority && !s.stack().is_empty()
}

fn chosen(d: &Decision, c: Choice) -> Opt {
    d.candidates.option(c).expect("an option")
}

fn cast_name(t: &GameTable, s: &State, o: Opt) -> Option<&'static str> {
    match o {
        Opt::Cast { slot, .. } => Some(name(t, s, slot)),
        _ => None,
    }
}

/// The opponent casts `spell` on their turn, holding mana up as given, while the expert holds
/// `hand` with `lands` untapped.
const fn counter_scenario(
    spell: &'static [&'static str],
    their_lands: &'static [Permanent],
    hand: &'static [&'static str],
    lands: &'static [Permanent],
) -> Scenario {
    Scenario::new("counterspell discipline")
        .at(4, P1, TurnStep::PrecombatMain)
        .hand(P1, spell)
        .battlefield(P1, their_lands)
        .library(P1, LIBRARY)
        .hand(P0, hand)
        .battlefield(P0, lands)
        .library(P0, LIBRARY)
}

const FOUR_ISLANDS: &[Permanent] = &[
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
];
const FIVE_ISLANDS: &[Permanent] = &[
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
];
const TWO_ISLANDS: &[Permanent] = &[Permanent::new(ISLAND), Permanent::new(ISLAND)];

const DRAIN: &[&str] = &[names::MANA_DRAIN];
const TITHE: &[&str] = &[names::MANA_TITHE];
const ONE_PLAINS: &[Permanent] = &[Permanent::new(PLAINS)];

#[test]
fn mana_drain_counters_a_planeswalker() {
    const JACE: Scenario = counter_scenario(
        &[names::JACE_THE_MIND_SCULPTOR],
        FOUR_ISLANDS,
        DRAIN,
        TWO_ISLANDS,
    );
    let (t, mut s) = arrange(&JACE);
    let (d, c) = expert_at(
        &t,
        &mut s,
        casts(names::JACE_THE_MIND_SCULPTOR),
        facing_a_spell,
    );
    assert_eq!(cast_name(&t, &s, chosen(&d, c)), Some(names::MANA_DRAIN));
}

/// With one counterspell, a minor spell is let through: the counter waits for a threat.
#[test]
fn the_last_counterspell_waits_for_a_threat() {
    const TRAINER: Scenario = counter_scenario(
        &[names::THUNDERTRAP_TRAINER],
        FOUR_ISLANDS,
        DRAIN,
        TWO_ISLANDS,
    );
    let (t, mut s) = arrange(&TRAINER);
    let (d, c) = expert_at(
        &t,
        &mut s,
        casts(names::THUNDERTRAP_TRAINER),
        facing_a_spell,
    );
    assert_eq!(chosen(&d, c), Opt::Pass);
}

/// Mana Tithe counters only a spell whose caster tapped out for it.
#[test]
fn mana_tithe_only_against_a_tapped_out_caster() {
    const TAPPED_OUT: Scenario = counter_scenario(
        &[names::JACE_THE_MIND_SCULPTOR],
        FOUR_ISLANDS,
        TITHE,
        ONE_PLAINS,
    );
    const ONE_UP: Scenario = counter_scenario(
        &[names::JACE_THE_MIND_SCULPTOR],
        FIVE_ISLANDS,
        TITHE,
        ONE_PLAINS,
    );
    let (t, mut s) = arrange(&TAPPED_OUT);
    let (d, c) = expert_at(
        &t,
        &mut s,
        casts(names::JACE_THE_MIND_SCULPTOR),
        facing_a_spell,
    );
    assert_eq!(cast_name(&t, &s, chosen(&d, c)), Some(names::MANA_TITHE));

    let (t, mut s) = arrange(&ONE_UP);
    let (d, c) = expert_at(
        &t,
        &mut s,
        casts(names::JACE_THE_MIND_SCULPTOR),
        facing_a_spell,
    );
    assert_eq!(chosen(&d, c), Opt::Pass, "the caster can pay {{1}}");
}

/// Strip Mine takes the dual land of an opponent short of lands, while we keep two.
#[test]
fn strip_mine_takes_the_dual_of_an_opponent_short_of_lands() {
    const STRIP: Scenario = Scenario::new("strip")
        .at(5, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(names::STRIP_MINE),
                Permanent::new(PLAINS),
                Permanent::new(ISLAND),
            ],
        )
        .library(P0, LIBRARY)
        .battlefield(P1, &[Permanent::new(names::TUNDRA), Permanent::new(ISLAND)])
        .library(P1, LIBRARY);
    let (t, mut s) = arrange(&STRIP);
    let (d, c) = expert_at(&t, &mut s, casts(""), |_, d| {
        d.kind == DecisionKind::Priority
    });
    let Opt::Activate { slot, .. } = chosen(&d, c) else {
        panic!("expected Strip Mine, got {:?}", chosen(&d, c));
    };
    assert_eq!(name(&t, &s, slot), names::STRIP_MINE);
    let (d, c) = expert_at(&t, &mut s, casts(""), |_, d| d.kind == DecisionKind::Target);
    assert_eq!(name(&t, &s, d.candidates.object(c).unwrap()), names::TUNDRA);
}

/// Get Lost destroys: aimed at a Sphinx of False Conclusions, it would only trade the Sphinx
/// for its token copy, so it takes the next creature instead.
#[test]
fn get_lost_leaves_a_sphinx_that_comes_back() {
    const SPHINX: Scenario = Scenario::new("sphinx")
        .at(5, P1, TurnStep::PrecombatMain)
        .battlefield(
            P1,
            &[
                Permanent::new(names::SPHINX_OF_FALSE_CONCLUSIONS),
                Permanent::new(names::ELITE_SPELLBINDER),
            ],
        )
        .library(P1, LIBRARY)
        .hand(P0, &[names::GET_LOST])
        .battlefield(P0, &[Permanent::new(PLAINS), Permanent::new(PLAINS)])
        .library(P0, LIBRARY);
    let (t, mut s) = arrange(&SPHINX);
    let (d, c) = expert_at(&t, &mut s, casts(""), |s, d| {
        d.kind == DecisionKind::Target && !s.stack().is_empty()
    });
    let target = d.candidates.object(c).expect("a creature");
    assert_eq!(name(&t, &s, target), names::ELITE_SPELLBINDER);
}

/// Jace bounces the creature whose attack would kill him when that saves him, and grows when
/// he is safe.
#[test]
fn jace_bounces_only_when_that_saves_him() {
    const JACE: &[Permanent] = &[
        Permanent::new(names::JACE_THE_MIND_SCULPTOR),
        Permanent::new(ISLAND),
    ];
    const THREATENED: Scenario = Scenario::new("threatened")
        .at(5, P0, TurnStep::PrecombatMain)
        .battlefield(P0, JACE)
        .hand(P0, &[PLAINS, ISLAND, PLAINS])
        .library(P0, LIBRARY)
        .battlefield(
            P1,
            &[
                Permanent::new(names::SPHINX_OF_FALSE_CONCLUSIONS),
                Permanent::new(names::THUNDERTRAP_TRAINER),
            ],
        )
        .library(P1, LIBRARY);
    const SAFE: Scenario = Scenario::new("safe")
        .at(5, P0, TurnStep::PrecombatMain)
        .battlefield(P0, JACE)
        .hand(P0, &[PLAINS, ISLAND, PLAINS, ISLAND])
        .library(P0, LIBRARY)
        .battlefield(P1, &[Permanent::new(names::THUNDERTRAP_TRAINER)])
        .library(P1, LIBRARY);
    let walker = |d: &Decision| d.kind == DecisionKind::Priority;
    let ability = |t: &GameTable, s: &State, o: Opt| match o {
        Opt::Activate {
            slot,
            ability: AbilityIx::Own(i),
        } if name(t, s, slot) == names::JACE_THE_MIND_SCULPTOR => {
            t.def(s.card(slot)).front().abilities[usize::from(i)]
                .cost
                .loyalty
        }
        _ => None,
    };
    let (t, mut s) = arrange(&THREATENED);
    let (d, c) = expert_at(&t, &mut s, casts(""), |s, d| {
        walker(d) && s.zone(P0, beryl_engine::Zone::Hand).count() == 2
    });
    assert_eq!(ability(&t, &s, chosen(&d, c)), Some(-1));

    let (t, mut s) = arrange(&SAFE);
    let (d, c) = expert_at(&t, &mut s, casts(""), |s, d| {
        walker(d) && s.zone(P0, beryl_engine::Zone::Hand).count() == 3
    });
    assert_eq!(ability(&t, &s, chosen(&d, c)), Some(2));
}

/// The lethal check: every flier attacks when together they kill past the one blocker, though
/// any of them attacking alone would die to it.
#[test]
fn attacks_with_everything_for_lethal() {
    const LETHAL: Scenario = Scenario::new("lethal")
        .at(7, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(names::ELITE_SPELLBINDER),
                Permanent::new(names::THIEVING_SKYDIVER),
                Permanent::new(names::MALCOLM_ALLURING_SCOUNDREL),
            ],
        )
        .library(P0, LIBRARY)
        .life(P1, 4)
        .battlefield(P1, &[Permanent::new(names::SPHINX_OF_FALSE_CONCLUSIONS)])
        .library(P1, LIBRARY);
    let (t, mut s) = arrange(&LETHAL);
    let mut attackers = 0;
    loop {
        let (d, c) = expert_at(&t, &mut s, casts(""), |_, d| {
            d.kind == DecisionKind::Attacker
        });
        match chosen(&d, c) {
            Opt::Attack(_) => attackers += 1,
            _ => break,
        }
        let mut x = Scratch::new();
        apply(&mut s, &t, &mut x, &mut Replay, c);
        if attackers == 3 {
            break;
        }
    }
    assert_eq!(attackers, 3);
}

/// Lethal through two bounces: the opponent's two fliers are all that can block our Sphinx,
/// so Jace's −1 and Teferi's −3 return them before combat and the Sphinx attacks for the
/// last 4 life. The ground blocker left behind cannot block a flier.
#[test]
fn bounces_both_flying_blockers_for_lethal() {
    const TWO_BOUNCES: Scenario = Scenario::new("two bounces")
        .at(7, P0, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(names::SPHINX_OF_FALSE_CONCLUSIONS),
                Permanent::new(names::JACE_THE_MIND_SCULPTOR),
                Permanent::new(names::TEFERI_TIME_RAVELER),
            ],
        )
        .library(P0, LIBRARY)
        .life(P1, 4)
        .battlefield(
            P1,
            &[
                Permanent::new(names::ELITE_SPELLBINDER),
                Permanent::new(names::MALCOLM_ALLURING_SCOUNDREL),
                Permanent::new(names::THUNDERTRAP_TRAINER),
            ],
        )
        .library(P1, LIBRARY);
    let (t, mut s) = arrange(&TWO_BOUNCES);
    // The opponent blocks as the expert would.
    let mut experts = [ExpertAgent::new(&t, 7), ExpertAgent::new(&t, 8)];
    let mut x = Scratch::new();
    let outcome = loop {
        let d = match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Over(outcome) => break Some(outcome),
            Step::Decide(_) if s.turn().number > 7 => break None,
            Step::Decide(d) => d,
        };
        let shown = *view(&s, &t, &mut x);
        let c = experts[usize::from(d.player == P1)].choose(Observation::new(&shown, d.player), &d);
        apply(&mut s, &t, &mut x, &mut Replay, c);
    };
    assert_eq!(outcome, Some(Outcome::Win(P0)));
}

/// No wheel that would deck us: from an empty hand against a full one Timetwister refuels,
/// but with a library of one and three cards in the graveyard there are not seven to draw.
#[test]
fn no_wheel_without_seven_cards_to_draw() {
    const THIN: Scenario = Scenario::new("thin library")
        .at(9, P0, TurnStep::PrecombatMain)
        .hand(P0, &[names::TIMETWISTER])
        .battlefield(
            P0,
            &[
                Permanent::new(ISLAND),
                Permanent::new(ISLAND),
                Permanent::new(ISLAND),
            ],
        )
        .library(P0, &[ISLAND])
        .graveyard(P0, &[PLAINS, PLAINS, ISLAND])
        .hand(P1, &[PLAINS, ISLAND, PLAINS, ISLAND, PLAINS])
        .library(P1, LIBRARY);
    let (t, mut s) = arrange(&THIN);
    let (d, c) = expert_at(&t, &mut s, casts(""), |_, d| {
        d.kind == DecisionKind::Priority
    });
    assert_ne!(cast_name(&t, &s, chosen(&d, c)), Some(names::TIMETWISTER));
}

/// Dead on board, the opponent's Teferi on the stack is the last chance to cast an instant:
/// "Each opponent can cast spells only any time they could cast a sorcery." Aang's Iceberg
/// goes in response.
#[test]
fn flash_removal_answers_teferi_before_it_locks_us_out() {
    const LOCK: Scenario = Scenario::new("teferi lock")
        .at(9, P1, TurnStep::PrecombatMain)
        .hand(P1, &[names::TEFERI_TIME_RAVELER])
        .battlefield(
            P1,
            &[
                Permanent::new(names::TUNDRA),
                Permanent::new(PLAINS),
                Permanent::new(ISLAND),
                Permanent::new(names::SPHINX_OF_FALSE_CONCLUSIONS),
                Permanent::new(names::ADELINE_RESPLENDENT_CATHAR),
            ],
        )
        .library(P1, LIBRARY)
        .life(P0, 6)
        .hand(P0, &[names::AANGS_ICEBERG])
        .battlefield(
            P0,
            &[
                Permanent::new(PLAINS),
                Permanent::new(PLAINS),
                Permanent::new(PLAINS),
            ],
        )
        .library(P0, LIBRARY);
    let (t, mut s) = arrange(&LOCK);
    let (d, c) = expert_at(
        &t,
        &mut s,
        casts(names::TEFERI_TIME_RAVELER),
        facing_a_spell,
    );
    assert_eq!(cast_name(&t, &s, chosen(&d, c)), Some(names::AANGS_ICEBERG));
}

/// Expert against random, seats alternated. The CLI's 200-game tournament measures the gate
/// (at least 90%); this pins a smaller seeded sample.
#[test]
fn expert_beats_random() {
    let t = mirror();
    let games = 20;
    let mut wins = 0;
    for seed in 0..games {
        let expert = if seed % 2 == 0 { Seat::P0 } else { Seat::P1 };
        let mut agents: [Box<dyn Agent>; 2] = [
            Box::new(ExpertAgent::new(&t, seed)),
            Box::new(RandomAgent::new(seed)),
        ];
        if expert == Seat::P1 {
            agents.swap(0, 1);
        }
        let mut s = State::new(&t, seed, Profile::SMART);
        let outcome = play(&t, &mut s, &mut Scratch::new(), &mut agents, |_, _| {});
        wins += u64::from(outcome == Outcome::Win(expert));
    }
    assert!(wins * 10 >= games * 9, "expert won {wins} of {games}");
}
