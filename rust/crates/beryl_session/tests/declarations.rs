//! The declare shape's prompts answered by clicks alone (plan K26): an attack and a block.
//! Each test holds the prompt's state after each click of one worked example to a text
//! written by hand from what the screen shows (plan K24), and checks that the clicks end
//! where the picks a UI holding the declaration itself would send end.

#[path = "common/play.rs"]
mod play;

use beryl_cards::names::{
    ELITE_SPELLBINDER, ISLAND, OVERLORD_OF_THE_MISTMOORS, SNAPCASTER_MAGE, TEFERI_TIME_RAVELER,
    THIEVING_SKYDIVER, THUNDERTRAP_TRAINER, TRINKET_MAGE,
};
use beryl_engine::card::Purpose;
use beryl_engine::layout::Queue as _;
use beryl_engine::{Candidates, Choice, Decision, Layout, Observation, Opt, TurnStep};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_search::{Agent, AgentDecision};
use beryl_session::prompt::{ButtonId, Click};
use beryl_session::protocol::{Candidate, Kind, Side, ZoneName};
use beryl_session::replay::Entry;
use beryl_session::save::Kind as SavedKind;
use beryl_session::script::Subject;
use play::{Play, cards};

const OPPONENT: Subject = Subject::Player {
    player: Side::Opponent,
};

/// The choices of the log from entry `from` on: the kind of decision each answered and
/// whether it continues the one before.
fn choices(play: &Play, from: usize) -> Vec<(SavedKind, bool)> {
    (play.game.replay().log[from..].iter())
        .filter_map(|entry| match entry {
            Entry::Choice(chosen) => Some((chosen.what.as_ref()?.kind, chosen.continuation)),
            _ => None,
        })
        .collect()
}

/// An opponent that attacks with every creature it is offered, and otherwise takes each
/// decision's first candidate.
struct AttackAll(u32);

impl<L: Layout> Agent<L> for AttackAll {
    fn choose(&mut self, _: Observation<'_, L>, d: &Decision<L>) -> Choice {
        self.0 += 1;
        let attack = match &d.candidates {
            Candidates::Options(options) => {
                (options.iter()).position(|o| matches!(o, Opt::Attack(_)))
            }
            Candidates::Objects(_) => None,
        };
        Choice(attack.unwrap_or(0) as u16)
    }

    fn next_decision(&self) -> Option<AgentDecision> {
        Some(AgentDecision(self.0))
    }

    fn rewind(&mut self, to: AgentDecision) {
        self.0 = to.0;
    }
}

/// Passes the human's priority until their decision is of `kind`.
fn pass_to(play: &mut Play, kind: Kind) {
    while play.decision().kind != kind {
        play.choose(|c| *c == Candidate::Pass);
    }
}

/// The human's turn with two Snapcaster Mages and an Elite Spellbinder to attack with, and
/// nothing in their way.
const ATTACK: Scenario = Scenario::new("attack")
    .at(3, P0, TurnStep::PrecombatMain)
    .battlefield(
        P0,
        &[
            Permanent::new(SNAPCASTER_MAGE),
            Permanent::new(SNAPCASTER_MAGE),
            Permanent::new(ELITE_SPELLBINDER),
        ],
    )
    .library(P0, &[ISLAND; 6])
    .library(P1, &[ISLAND; 6]);

/// An attack is declared by clicking the creatures: each is aimed at the defending player at
/// once, there being nothing else to attack, and taken back by another click. Every creature
/// that can attack is marked, the second of two alike too, though the engine offers one for
/// both. The buttons declare every attacker, none, or what is picked.
#[test]
fn attackers_are_declared_by_clicks_and_confirmed_by_their_count() {
    let mut play = Play::scenario(&ATTACK);
    pass_to(&mut play, Kind::Attacker);
    let mages = play.named(SNAPCASTER_MAGE, ZoneName::Battlefield, Side::You);
    let binder = play.named(ELITE_SPELLBINDER, ZoneName::Battlefield, Side::You)[0];
    let open = format!(
        "declare attack: Choose attackers.\n\
         declare attack:\n\
         eligible {}\n\
         auto[All Attack]* decline[No Attacks]\n",
        cards(&[mages[0], mages[1], binder]),
    );
    assert_eq!(play.prompt(), open);
    // Nothing is declared: there is nothing to confirm.
    assert_eq!(
        play.press(ButtonId::Confirm).as_deref(),
        Some(open.as_str())
    );
    // The second Mage attacks the defending player, the one thing it can attack.
    let one = format!(
        "declare attack: Choose attackers.\n\
         declare attack: #{}>opponent\n\
         eligible {}\n\
         selected #{}\n\
         confirm[1 Attacker]* decline[Cancel Attacks]\n",
        mages[1],
        cards(&[mages[0], binder]),
        mages[1],
    );
    assert_eq!(play.on(mages[1]).as_deref(), Some(one.as_str()));
    let two = format!(
        "declare attack: Choose attackers.\n\
         declare attack: #{}>opponent #{binder}>opponent\n\
         eligible #{}\n\
         selected {}\n\
         confirm[2 Attackers]* decline[Cancel Attacks]\n",
        mages[1],
        mages[0],
        cards(&[mages[1], binder]),
    );
    assert_eq!(play.on(binder).as_deref(), Some(two.as_str()));
    // A click on an attacker takes it back; the button takes back them all.
    assert!(play.on(mages[1]).is_some());
    assert_eq!(play.on(mages[1]).map(|s| s.lines().count()), Some(5));
    assert_eq!(
        play.press(ButtonId::Decline).as_deref(),
        Some(open.as_str())
    );
    // Every creature attacks, and one is taken back.
    let all = format!(
        "declare attack: Choose attackers.\n\
         declare attack: #{}>opponent #{}>opponent #{binder}>opponent\n\
         selected {}\n\
         confirm[3 Attackers]* decline[Cancel Attacks]\n",
        mages[0],
        mages[1],
        cards(&[mages[0], mages[1], binder]),
    );
    assert_eq!(play.press(ButtonId::Auto).as_deref(), Some(all.as_str()));
    assert!(play.on(mages[0]).is_some());

    // The picks a UI holding the declaration sends: the Mage the engine offers for the two,
    // the Spellbinder, and the end.
    let mut picked = Play::scenario(&ATTACK);
    pass_to(&mut picked, Kind::Attacker);
    let from = picked.game.replay().log.len();
    let attacks = |slot: u16| move |c: &Candidate| *c == Candidate::Attack { slot };
    picked.choose(attacks(mages[0]));
    picked.choose(attacks(binder));
    picked.choose(|c| *c == Candidate::Done);
    assert_eq!(play.press(ButtonId::Confirm), None, "the button answers");
    assert_eq!(play.game.state(), picked.game.state());
    // One prompt, one answer and its continuations: an undo takes back the declaration.
    assert_eq!(
        choices(&play, from)[..3],
        [
            (SavedKind::Attacker, false),
            (SavedKind::Attacker, true),
            (SavedKind::Attacker, true)
        ]
    );
    // A Mage and the Spellbinder dealt their damage, two and three.
    assert_eq!(play.view.players.opponent.life, 15);
}

/// No attackers is the decline button's answer.
#[test]
fn no_attack_is_declared_by_its_button() {
    let mut play = Play::scenario(&ATTACK);
    pass_to(&mut play, Kind::Attacker);
    let mut picked = Play::scenario(&ATTACK);
    pass_to(&mut picked, Kind::Attacker);
    picked.choose(|c| *c == Candidate::Done);
    assert_eq!(play.press(ButtonId::Decline), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
}

/// The human's turn with a Snapcaster Mage and an Elite Spellbinder, and a Teferi, Time
/// Raveler across the table to attack.
const WALKER: Scenario = Scenario::new("attack a planeswalker")
    .at(3, P0, TurnStep::PrecombatMain)
    .battlefield(
        P0,
        &[
            Permanent::new(SNAPCASTER_MAGE),
            Permanent::new(ELITE_SPELLBINDER),
        ],
    )
    .battlefield(P1, &[Permanent::new(TEFERI_TIME_RAVELER)])
    .library(P0, &[ISLAND; 6])
    .library(P1, &[ISLAND; 6]);

/// While the defending player controls a planeswalker, a creature declared awaits its aim:
/// the next click on the player or the planeswalker gives it, and the declaration confirms
/// once every attacker is aimed.
#[test]
fn an_attacker_is_aimed_at_a_planeswalker_or_its_controller() {
    let mut play = Play::scenario(&WALKER);
    pass_to(&mut play, Kind::Attacker);
    let mage = play.named(SNAPCASTER_MAGE, ZoneName::Battlefield, Side::You)[0];
    let binder = play.named(ELITE_SPELLBINDER, ZoneName::Battlefield, Side::You)[0];
    let teferi = play.named(TEFERI_TIME_RAVELER, ZoneName::Battlefield, Side::Opponent)[0];
    assert_eq!(
        play.prompt(),
        format!(
            "declare attack: Choose attackers.\n\
             declare attack:\n\
             eligible {}\n\
             auto[All Attack]* decline[No Attacks]\n",
            cards(&[mage, binder]),
        )
    );
    // Nothing awaits an aim yet: a click on the planeswalker does nothing.
    assert!(play.on(teferi).is_some_and(|s| !s.contains("selected")));
    // The Mage awaits its aim: what it can attack is marked, and the count cannot confirm.
    assert_eq!(
        play.on(mage).as_deref(),
        Some(
            format!(
                "declare attack: Choose attackers.\n\
                 declare attack:\n\
                 aiming #{mage}\n\
                 eligible {} opponent\n\
                 selected #{mage}\n\
                 confirm(1 Attacker) decline[Cancel Attacks]\n",
                cards(&[binder, teferi]),
            )
            .as_str()
        )
    );
    assert_eq!(
        play.on(teferi).as_deref(),
        Some(
            format!(
                "declare attack: Choose attackers.\n\
                 declare attack: #{mage}>#{teferi}\n\
                 eligible #{binder}\n\
                 selected #{mage}\n\
                 confirm[1 Attacker]* decline[Cancel Attacks]\n"
            )
            .as_str()
        )
    );
    assert!(play.on(binder).is_some_and(|s| s.contains("aiming")));
    assert_eq!(
        play.click(Click::On { on: OPPONENT }).as_deref(),
        Some(
            format!(
                "declare attack: Choose attackers.\n\
                 declare attack: #{mage}>#{teferi} #{binder}>opponent\n\
                 selected {}\n\
                 confirm[2 Attackers]* decline[Cancel Attacks]\n",
                cards(&[mage, binder]),
            )
            .as_str()
        )
    );

    // The picks a UI holding the declaration sends: each attacker, then what it attacks.
    // With no creature left to declare, the engine asks for no end.
    let mut picked = Play::scenario(&WALKER);
    pass_to(&mut picked, Kind::Attacker);
    let aims = |planeswalker| move |c: &Candidate| *c == Candidate::Defender { planeswalker };
    picked.choose(|c| *c == Candidate::Attack { slot: mage });
    picked.choose(aims(Some(teferi)));
    picked.choose(|c| *c == Candidate::Attack { slot: binder });
    picked.choose(aims(None));
    assert_eq!(play.press(ButtonId::Confirm), None, "the button answers");
    assert_eq!(play.game.state(), picked.game.state());
}

/// The opponent's turn: a Thundertrap Trainer and a Thieving Skydiver, which flies, attack
/// into an Elite Spellbinder, which flies, and an Overlord of the Mistmoors, which does not.
const BLOCK: Scenario = Scenario::new("block")
    .at(2, P1, TurnStep::PrecombatMain)
    .battlefield(
        P1,
        &[
            Permanent::new(THUNDERTRAP_TRAINER),
            Permanent::new(THIEVING_SKYDIVER),
        ],
    )
    .battlefield(
        P0,
        &[
            Permanent::new(ELITE_SPELLBINDER),
            Permanent::new(OVERLORD_OF_THE_MISTMOORS),
        ],
    )
    .library(P0, &[ISLAND; 6])
    .library(P1, &[ISLAND; 6]);

/// The game at the human's declaration of blockers against every creature of `scenario`'s
/// opponent.
fn blocking(scenario: &Scenario) -> Play {
    let mut play = Play::against(scenario, Some(Box::new(AttackAll(0))));
    pass_to(&mut play, Kind::Blocker);
    assert!(play.decision().prompt.is(Purpose::Block));
    play
}

/// A block is declared by clicking a blocker and then the attacker it blocks; a blocker that
/// can block one attacker alone blocks it at once
/// ([CR 702.9b](https://yawgatog.com/resources/magic-rules/#R7029b): "A creature with flying
/// can't be blocked except by creatures with flying and/or reach").
#[test]
fn blockers_are_declared_by_clicks_and_aimed_at_their_attackers() {
    let mut play = blocking(&BLOCK);
    let binder = play.named(ELITE_SPELLBINDER, ZoneName::Battlefield, Side::You)[0];
    let overlord = play.named(OVERLORD_OF_THE_MISTMOORS, ZoneName::Battlefield, Side::You)[0];
    let trainer = play.named(THUNDERTRAP_TRAINER, ZoneName::Battlefield, Side::Opponent)[0];
    let skydiver = play.named(THIEVING_SKYDIVER, ZoneName::Battlefield, Side::Opponent)[0];
    let open = format!(
        "declare block: Choose blockers.\n\
         declare block:\n\
         eligible {}\n\
         decline[No Blocks]*\n",
        cards(&[binder, overlord]),
    );
    assert_eq!(play.prompt(), open);
    // An attacker with no blocker awaiting it is nothing to click.
    assert_eq!(play.on(trainer).as_deref(), Some(open.as_str()));
    // The Overlord can block the Trainer alone, and does.
    assert_eq!(
        play.on(overlord).as_deref(),
        Some(
            format!(
                "declare block: Choose blockers.\n\
                 declare block: #{overlord}>#{trainer}\n\
                 eligible #{binder}\n\
                 selected #{overlord}\n\
                 confirm[1 Blocker]* decline[Cancel Blocks]\n"
            )
            .as_str()
        )
    );
    // The Spellbinder can block either: it awaits its attacker, both marked.
    assert_eq!(
        play.on(binder).as_deref(),
        Some(
            format!(
                "declare block: Choose blockers.\n\
                 declare block: #{overlord}>#{trainer}\n\
                 aiming #{binder}\n\
                 eligible {}\n\
                 selected {}\n\
                 confirm(2 Blockers) decline[Cancel Blocks]\n",
                cards(&[trainer, skydiver]),
                cards(&[binder, overlord]),
            )
            .as_str()
        )
    );
    assert_eq!(
        play.on(skydiver).as_deref(),
        Some(
            format!(
                "declare block: Choose blockers.\n\
                 declare block: #{overlord}>#{trainer} #{binder}>#{skydiver}\n\
                 selected {}\n\
                 confirm[2 Blockers]* decline[Cancel Blocks]\n",
                cards(&[binder, overlord]),
            )
            .as_str()
        )
    );

    // The picks a UI holding the declaration sends: the Overlord, whose one attacker the
    // engine does not ask for, and the Spellbinder and its attacker. With no creature left to
    // declare, the engine asks for no end.
    let mut picked = blocking(&BLOCK);
    let from = picked.game.replay().log.len();
    picked.choose(|c| *c == Candidate::Block { slot: overlord });
    picked.choose(|c| *c == Candidate::Block { slot: binder });
    picked.choose(|c| *c == Candidate::Object { slot: skydiver });
    assert_eq!(play.press(ButtonId::Confirm), None, "the button answers");
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(
        choices(&play, from)[..3],
        [
            (SavedKind::Blocker, false),
            (SavedKind::Blocker, true),
            (SavedKind::BlockTarget, true),
        ]
    );
}

/// No blockers is the decline button's answer.
#[test]
fn no_block_is_declared_by_its_button() {
    let mut play = blocking(&BLOCK);
    let mut picked = blocking(&BLOCK);
    picked.choose(|c| *c == Candidate::Done);
    assert_eq!(play.press(ButtonId::Decline), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
}

/// The opponent's turn: two Snapcaster Mages attack into a Trinket Mage and an Elite
/// Spellbinder.
const TWINS: Scenario = Scenario::new("block one of two alike")
    .at(2, P1, TurnStep::PrecombatMain)
    .battlefield(
        P1,
        &[
            Permanent::new(SNAPCASTER_MAGE),
            Permanent::new(SNAPCASTER_MAGE),
        ],
    )
    .battlefield(
        P0,
        &[
            Permanent::new(TRINKET_MAGE),
            Permanent::new(ELITE_SPELLBINDER),
        ],
    )
    .library(P0, &[ISLAND; 6])
    .library(P1, &[ISLAND; 6]);

/// Of two attackers alike the engine offers one, so either can be clicked, and two blockers
/// aimed at one of them block the same one, whichever the engine takes for it.
#[test]
fn two_blockers_aimed_at_one_of_two_attackers_alike_block_the_same_one() {
    let mut play = blocking(&TWINS);
    let trinket = play.named(TRINKET_MAGE, ZoneName::Battlefield, Side::You)[0];
    let binder = play.named(ELITE_SPELLBINDER, ZoneName::Battlefield, Side::You)[0];
    let mages = play.named(SNAPCASTER_MAGE, ZoneName::Battlefield, Side::Opponent);
    // Each blocker awaits its attacker, though the engine has one answer for it: the two are
    // two to the player.
    assert_eq!(
        play.on(trinket).as_deref(),
        Some(
            format!(
                "declare block: Choose blockers.\n\
                 declare block:\n\
                 aiming #{trinket}\n\
                 eligible #{binder} {}\n\
                 selected #{trinket}\n\
                 confirm(1 Blocker) decline[Cancel Blocks]\n",
                cards(&mages),
            )
            .as_str()
        )
    );
    assert!(play.on(mages[1]).is_some());
    assert!(play.on(binder).is_some());
    assert_eq!(
        play.on(mages[1]).as_deref(),
        Some(
            format!(
                "declare block: Choose blockers.\n\
                 declare block: #{trinket}>#{} #{binder}>#{}\n\
                 selected {}\n\
                 confirm[2 Blockers]* decline[Cancel Blocks]\n",
                mages[1],
                mages[1],
                cards(&[trinket, binder]),
            )
            .as_str()
        )
    );
    // The picks a UI holding the declaration sends. The engine offers the first Mage for
    // the two and, as the one answer, takes it unasked; once blocked it is told apart from
    // the other, so the second blocker's attacker is asked, and is the one blocked.
    let mut picked = blocking(&TWINS);
    picked.choose(|c| *c == Candidate::Block { slot: trinket });
    picked.choose(|c| *c == Candidate::Block { slot: binder });
    picked.choose(|c| *c == Candidate::Object { slot: mages[0] });
    assert_eq!(play.press(ButtonId::Confirm), None, "the button answers");
    assert_eq!(play.game.state(), picked.game.state());
    // Both blocked one Mage, which died; the other dealt its damage.
    let left = play.named(SNAPCASTER_MAGE, ZoneName::Battlefield, Side::Opponent);
    assert_eq!(left.len(), 1);
    assert_eq!(play.view.players.you.life, 18);
}
