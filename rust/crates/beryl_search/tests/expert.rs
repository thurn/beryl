//! The expert system's card knowledge, pinned on hand-built positions (Milestone 1 decision
//! 6), and its strength against the random agent.

mod common;

use beryl_cards::{CARDS, names};
use beryl_engine::card::AbilityIx;
use beryl_engine::{
    Candidates, Choice, Decision, DecisionKind, GameTable, Observation, Opt, Outcome, Profile,
    Replay, Scratch, Seat, Slot, State, Step, TurnStep, ZoneKey, apply, next, view,
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
            .map_or(Choice(0), |i| Choice(i as u16)),
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

/// The opponent's Winds of Abandon exiles the expert's Jace, Vryn's Prodigy: its controller
/// searches for a basic land, and the expert takes one rather than nothing.
#[test]
fn winds_of_abandon_fetches_a_basic() {
    const WINDS: Scenario = Scenario::new("Winds of Abandon on the expert's creature")
        .at(3, P1, TurnStep::PrecombatMain)
        .hand(P1, &[names::WINDS_OF_ABANDON])
        .battlefield(P1, &[Permanent::new(PLAINS), Permanent::new(PLAINS)])
        .library(P1, LIBRARY)
        .battlefield(
            P0,
            &[
                Permanent::new(PLAINS),
                Permanent::new(names::JACE_VRYNS_PRODIGY),
            ],
        )
        .library(P0, LIBRARY);
    let (t, mut s) = arrange(&WINDS);
    let (d, c) = expert_at(&t, &mut s, casts(names::WINDS_OF_ABANDON), |_, d| {
        d.kind == DecisionKind::ChooseUpTo
    });
    assert!(
        matches!(chosen(&d, c), Opt::Object(slot) if name(&t, &s, slot) == PLAINS || name(&t, &s, slot) == ISLAND),
        "{:?}",
        chosen(&d, c)
    );
}

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
                Permanent::new(names::ADELINE_RESPLENDENT_CATHAR),
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
    assert_eq!(name(&t, &s, target), names::ADELINE_RESPLENDENT_CATHAR);
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
        walker(d) && s.zone(ZoneKey::Of(P0, beryl_engine::Zone::Hand)).count() == 2
    });
    assert_eq!(ability(&t, &s, chosen(&d, c)), Some(-1));

    let (t, mut s) = arrange(&SAFE);
    let (d, c) = expert_at(&t, &mut s, casts(""), |s, d| {
        walker(d) && s.zone(ZoneKey::Of(P0, beryl_engine::Zone::Hand)).count() == 3
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
    let outcome = play_turn(&t, &mut s, 7);
    assert_eq!(outcome, Some(Outcome::Win(P0)));
}

/// No wheel that would deck us: from an empty hand against a full one Timetwister refuels,
/// but with a library of three and four cards in the graveyard its seven draws would leave
/// none for our next draw step.
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
        .library(P0, &[ISLAND, PLAINS, ISLAND])
        .graveyard(P0, &[PLAINS, PLAINS, ISLAND, PLAINS])
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

/// Plays the expert against itself from `s` until the turn after `turn` begins, or the game
/// ends.
fn play_turn(t: &GameTable, s: &mut State, turn: u16) -> Option<Outcome> {
    let mut experts = [ExpertAgent::new(t, 7), ExpertAgent::new(t, 8)];
    let mut x = Scratch::new();
    loop {
        let d = match next(s, t, &mut x, &mut Replay) {
            Step::Over(outcome) => return Some(outcome),
            Step::Decide(_) if s.turn().number > turn => return None,
            Step::Decide(d) => d,
        };
        let shown = *view(s, t, &mut x);
        let c = experts[usize::from(d.player == P1)].choose(Observation::new(&shown, d.player), &d);
        apply(s, t, &mut x, &mut Replay, c);
    }
}

/// No attack that wins only if the defender does not block: Elite Spellbinder trades with
/// Malcolm, and the Overlord then kills us.
#[test]
fn no_attack_that_wins_only_unblocked() {
    const TRADE: Scenario = Scenario::new("trade")
        .at(7, P0, TurnStep::PrecombatMain)
        .battlefield(P0, &[Permanent::new(names::MALCOLM_ALLURING_SCOUNDREL)])
        .library(P0, LIBRARY)
        .life(P0, 6)
        .life(P1, 2)
        .battlefield(
            P1,
            &[
                Permanent::new(names::ELITE_SPELLBINDER),
                Permanent::new(names::OVERLORD_OF_THE_MISTMOORS).tapped(),
            ],
        )
        .library(P1, LIBRARY);
    let (t, mut s) = arrange(&TRADE);
    let (d, c) = expert_at(&t, &mut s, casts(""), |_, d| {
        d.kind == DecisionKind::Attacker
    });
    assert_eq!(chosen(&d, c), Opt::Done);
}

/// Lethal through removal: Get Lost and Jace's −1 take the two blockers. Oust, whose 3 life
/// would be the 3 they survive at, stays in hand.
#[test]
fn removal_but_not_oust_opens_lethal() {
    const OPEN: Scenario = Scenario::new("open")
        .at(7, P0, TurnStep::PrecombatMain)
        .hand(P0, &[names::OUST, names::GET_LOST])
        .battlefield(
            P0,
            &[
                Permanent::new(PLAINS),
                Permanent::new(PLAINS),
                Permanent::new(PLAINS),
                Permanent::new(PLAINS),
                Permanent::new(names::OVERLORD_OF_THE_MISTMOORS),
                Permanent::new(names::SPHINX_OF_FALSE_CONCLUSIONS),
                Permanent::new(names::MALCOLM_ALLURING_SCOUNDREL),
                Permanent::new(names::JACE_THE_MIND_SCULPTOR),
            ],
        )
        .library(P0, LIBRARY)
        .life(P0, 5)
        .life(P1, 10)
        .battlefield(
            P1,
            &[
                Permanent::new(names::THUNDERTRAP_TRAINER),
                Permanent::new(names::THUNDERTRAP_TRAINER),
                Permanent::new(names::OVERLORD_OF_THE_MISTMOORS).tapped(),
            ],
        )
        .library(P1, LIBRARY);
    let (t, mut s) = arrange(&OPEN);
    assert_eq!(play_turn(&t, &mut s, 7), Some(Outcome::Win(P0)));
}

/// Removal is aimed as it was weighed: low on life, Swift Reconfiguration goes on the Sphinx,
/// the biggest attacker, not on the most valuable card.
#[test]
fn removal_aims_at_the_creature_that_kills_us() {
    const LOW: Scenario = Scenario::new("low")
        .at(7, P0, TurnStep::PrecombatMain)
        .life(P0, 6)
        .hand(P0, &[names::SWIFT_RECONFIGURATION])
        .battlefield(P0, &[Permanent::new(PLAINS)])
        .library(P0, LIBRARY)
        .battlefield(
            P1,
            &[
                Permanent::new(names::ADELINE_RESPLENDENT_CATHAR),
                Permanent::new(names::SPHINX_OF_FALSE_CONCLUSIONS),
                Permanent::new(names::THUNDERTRAP_TRAINER),
            ],
        )
        .library(P1, LIBRARY);
    let (t, mut s) = arrange(&LOW);
    let (d, c) = expert_at(&t, &mut s, casts(""), |s, d| {
        d.kind == DecisionKind::Target && !s.stack().is_empty()
    });
    let target = d.candidates.object(c).expect("a creature");
    assert_eq!(name(&t, &s, target), names::SPHINX_OF_FALSE_CONCLUSIONS);
}

/// Subtlety's evoke ("Exile a blue card from your hand") is two cards for an enters ability
/// that answers only a spell: it stays in hand through our turn and theirs while nothing is
/// cast.
#[test]
fn subtlety_is_not_evoked_on_an_empty_stack() {
    const IDLE: Scenario = Scenario::new("idle")
        .at(5, P0, TurnStep::PrecombatMain)
        .hand(P0, &[names::SUBTLETY, names::COUNTERSPELL])
        .library(P0, LIBRARY)
        .library(P1, LIBRARY);
    let (t, mut s) = arrange(&IDLE);
    let mut expert = ExpertAgent::new(&t, 7);
    let mut x = Scratch::new();
    while s.turn().number <= 6 {
        let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
            panic!("the game ended");
        };
        let shown = *view(&s, &t, &mut x);
        let c = if d.player == P1 {
            casts("")(&t, &shown, &d)
        } else {
            let c = expert.choose(Observation::new(&shown, P0), &d);
            if let Some(o) = d.candidates.option(c) {
                assert_ne!(cast_name(&t, &shown, o), Some(names::SUBTLETY));
            }
            c
        };
        apply(&mut s, &t, &mut x, &mut Replay, c);
    }
}

/// Jace, Wielder of Mysteries' +1, "Target player mills two cards": the opponent, unless
/// their own Jace, Wielder could still come down and win from the empty library; then
/// ourselves, toward our own Jace's win. Ours alone does not make theirs a danger.
#[test]
fn jace_wielder_mills_the_opponent_unless_their_jace_is_live() {
    const JACE: &[Permanent] = &[
        Permanent::new(names::JACE_WIELDER_OF_MYSTERIES),
        Permanent::new(ISLAND),
    ];
    const RIVAL: &[&str] = &[
        PLAINS,
        ISLAND,
        names::JACE_WIELDER_OF_MYSTERIES,
        PLAINS,
        ISLAND,
    ];
    const fn scenario(their_library: &'static [&'static str]) -> Scenario {
        Scenario::new("jace mills")
            .at(5, P0, TurnStep::PrecombatMain)
            .battlefield(P0, JACE)
            .library(P0, LIBRARY)
            .library(P1, their_library)
    }
    const LIVE: Scenario = scenario(RIVAL);
    const NONE: Scenario = scenario(LIBRARY);
    const BURIED: Scenario = scenario(LIBRARY).graveyard(P1, &[names::JACE_WIELDER_OF_MYSTERIES]);
    let milled = |scenario: &Scenario| {
        let (t, mut s) = arrange(scenario);
        let (d, c) = expert_at(&t, &mut s, casts(""), |s, d| {
            s.stack()
                .last()
                .is_some_and(|e| t.def(e.card).name == names::JACE_WIELDER_OF_MYSTERIES)
                && matches!(&d.candidates, Candidates::Options(o)
                    if o.iter().any(|o| matches!(o, Opt::Player(_))))
        });
        chosen(&d, c)
    };
    assert_eq!(milled(&LIVE), Opt::Player(P0));
    assert_eq!(milled(&NONE), Opt::Player(P1));
    assert_eq!(milled(&BURIED), Opt::Player(P1));
}

/// Teferi, Hero of Dominaria's +1 draws a card: taken on a deep library, not on one whose
/// draws could deck us ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)).
#[test]
fn teferi_hero_does_not_draw_from_a_thin_library() {
    const TEFERI: &[Permanent] = &[
        Permanent::new(names::TEFERI_HERO_OF_DOMINARIA),
        Permanent::new(ISLAND),
    ];
    const DEEP: &[&str] = &[
        PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS,
        ISLAND,
    ];
    const fn scenario(library: &'static [&'static str]) -> Scenario {
        Scenario::new("teferi draws")
            .at(5, P0, TurnStep::PrecombatMain)
            .battlefield(P0, TEFERI)
            .library(P0, library)
            .library(P1, LIBRARY)
    }
    let plus_one = |scenario: &Scenario| {
        let (t, mut s) = arrange(scenario);
        let (d, c) = expert_at(&t, &mut s, casts(""), |_, d| {
            d.kind == DecisionKind::Priority
        });
        matches!(
            chosen(&d, c),
            Opt::Activate { slot, ability: AbilityIx::Own(i) }
                if t.def(s.card(slot)).front().abilities[usize::from(i)].cost.loyalty == Some(1)
        )
    };
    assert!(plus_one(&scenario(DEEP)));
    assert!(!plus_one(&scenario(&[PLAINS, ISLAND, PLAINS])));
}

/// Expert against random, seats alternated. The CLI's 400-game tournament measures the gate
/// (at least 97% in the cb8d5d5f mirror); this pins a smaller seeded sample, with room for its
/// luck: 40 of 40, at 98.8% over 400, once retuned for the mirror's cards.
#[test]
fn expert_beats_random() {
    let t = mirror();
    let games = 40;
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
    assert!(wins * 20 >= games * 19, "expert won {wins} of {games}");
}
