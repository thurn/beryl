//! The pick shape's prompts answered by clicks alone (plan K26): a priority, a choice of
//! targets, and a choice among permanents on the table. Each test holds the prompt's state
//! after each click of one worked example to a text written by hand from what the screen
//! shows (plan K24), and checks that the clicks end where the picks a UI holding the prompt's
//! state itself would send end.

use std::sync::mpsc::channel;

#[path = "common/play.rs"]
mod play;

use beryl_cards::CARDS;
use beryl_cards::names::{
    COUNTERSPELL, ELITE_SPELLBINDER, ISLAND, MANA_CRYPT, MANA_TITHE, OUST, PLAINS, REPRIEVE,
    SNAPCASTER_MAGE, STAFF_OF_THE_STORYTELLER, STOCK_UP, TEFERI_HERO_OF_DOMINARIA,
    TEFERI_TIME_RAVELER, THUNDERTRAP_TRAINER, TIME_SPIRAL, TUNDRA,
};
use beryl_engine::card::Purpose;
use beryl_engine::layout::Queue as _;
use beryl_engine::{Candidates, Choice, Decision, Layout, Observation, Opt, Seat, TurnStep};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_search::{Agent, AgentDecision, Budget};
use beryl_session::prompt::{ButtonId, Click, ClickReply, ClickRequest, ShapeView};
use beryl_session::protocol::{Candidate, DecisionView, Kind, Side, ZoneName};
use beryl_session::replay::Entry;
use beryl_session::save::Kind as SavedKind;
use beryl_session::{
    CandidateIndex, Game, Prepared, SessionError, Sessions, StartGame, Status, StopsSetting,
};
use play::{Play, SESSION, cards};

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

/// The human's main phase with two Islands, a Mana Crypt and a Counterspell in hand, and an
/// Island and a Tundra to pay with.
const MAIN_PHASE: Scenario = Scenario::new("priority")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[ISLAND, ISLAND, MANA_CRYPT, COUNTERSPELL])
    .battlefield(P0, &[Permanent::new(ISLAND), Permanent::new(TUNDRA)])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// A priority marks what can be played or cast, not what only makes mana or cannot be cast,
/// and its buttons pass. A card with one way to use it is used by a click; one with several
/// opens the choice among them.
#[test]
fn a_priority_is_answered_by_a_click_on_a_card_or_its_pass_button() {
    let mut play = Play::scenario(&MAIN_PHASE);
    let islands = play.named(ISLAND, ZoneName::Hand, Side::You);
    let crypt = play.named(MANA_CRYPT, ZoneName::Hand, Side::You)[0];
    let counterspell = play.named(COUNTERSPELL, ZoneName::Hand, Side::You)[0];
    let tundra = play.named(TUNDRA, ZoneName::Battlefield, Side::You)[0];
    // Either Island plays, and Mana Crypt costs nothing; Counterspell has no spell to target. The
    // turn button says where passing leads: no creature attacks, so to the second main phase.
    let open = format!(
        "pick priority: \n\
         picked 0 of 0..1\n\
         eligible {}\n\
         decline[Next | To Main 2]*\n",
        cards(&[islands[0], islands[1], crypt]),
    );
    assert_eq!(play.prompt(), open);
    // A card with nothing to do changes nothing.
    assert_eq!(play.on(counterspell).as_deref(), Some(open.as_str()));
    // The Tundra makes white or blue: two ways, so the choice among them opens, under its
    // name. The session does not hold that prompt's state yet.
    assert_eq!(play.on(tundra).as_deref(), Some("mode act: Choose One\n"));
    assert_eq!(play.prompt(), open, "the priority is as it was");

    // The second Island in hand stands for the first, which the engine offers for both
    // (architecture §7.2): the click plays a land, as picking the candidate does.
    let mut picked = Play::scenario(&MAIN_PHASE);
    picked.choose(|c| matches!(c, Candidate::PlayLand { .. }));
    assert_eq!(play.on(islands[1]), None, "the click answers");
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(
        play.named(ISLAND, ZoneName::Battlefield, Side::You).len(),
        2
    );

    // The land is played: the Mana Crypt is all that is left to mark. Passing by the button ends
    // the main phase, as picking the pass does.
    assert_eq!(
        play.prompt(),
        format!(
            "pick priority: \n\
             picked 0 of 0..1\n\
             eligible #{crypt}\n\
             decline[Next | To Main 2]*\n"
        )
    );
    picked.choose(|c| *c == Candidate::Pass);
    assert_eq!(play.press(ButtonId::Decline), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.view.turn.step, picked.view.turn.step);
}

/// P0 casts Oust with a creature on each side to aim it at.
const OUST_EITHER: Scenario = Scenario::new("target")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[OUST, PLAINS])
    .battlefield(
        P0,
        &[Permanent::new(PLAINS), Permanent::new(THUNDERTRAP_TRAINER)],
    )
    .battlefield(P1, &[Permanent::new(ELITE_SPELLBINDER)])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// A single target is picked by a click. A removal aimed at the viewer's own permanent is
/// asked about first (UI Conventions, exception 7): the click picks it, marked as theirs,
/// and the prompt's buttons confirm it or take it back.
#[test]
fn a_target_is_picked_by_a_click_and_an_own_permanent_is_asked_about() {
    let mut play = Play::scenario(&OUST_EITHER);
    let oust = play.named(OUST, ZoneName::Hand, Side::You)[0];
    let mine = play.named(THUNDERTRAP_TRAINER, ZoneName::Battlefield, Side::You)[0];
    let theirs = play.named(ELITE_SPELLBINDER, ZoneName::Battlefield, Side::Opponent)[0];
    assert_eq!(play.on(oust), None, "casting Oust is a priority's click");
    assert!(play.decision().prompt.is(Purpose::Target));
    let open = format!(
        "pick target: Choose a target.\n\
         picked 0 of 1\n\
         warned #{mine}\n\
         eligible {}\n",
        cards(&[mine, theirs]),
    );
    assert_eq!(play.prompt(), open);
    let asked = format!(
        "pick target: Do you want to target Thundertrap Trainer?\n\
         picked 1 of 1: #{mine}\n\
         warned #{mine}\n\
         eligible #{theirs}\n\
         friendly-target #{mine}\n\
         confirm[Yes] decline[No]*\n"
    );
    assert_eq!(play.on(mine).as_deref(), Some(asked.as_str()));
    // Cancel takes the pick back, and so does a second click on it.
    assert_eq!(
        play.press(ButtonId::Decline).as_deref(),
        Some(open.as_str())
    );
    assert_eq!(play.on(mine).as_deref(), Some(asked.as_str()));
    assert_eq!(play.on(mine).as_deref(), Some(open.as_str()));
    // A click on the opponent's creature answers at once, as picking that target does.
    let mut picked = Play::scenario(&OUST_EITHER);
    picked.choose(|c| matches!(c, Candidate::Cast { .. }));
    picked.choose(|c| *c == Candidate::Object { slot: theirs });
    assert_eq!(play.on(theirs), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    let spellbinders = |play: &Play| {
        play.named(ELITE_SPELLBINDER, ZoneName::Battlefield, Side::Opponent)
            .len()
    };
    assert_eq!(spellbinders(&play), 0, "it left the battlefield");

    // Confirming the warning aims Oust at the viewer's own creature.
    let mut own = Play::scenario(&OUST_EITHER);
    own.on(oust);
    own.on(mine);
    assert_eq!(own.press(ButtonId::Confirm), None);
    assert!(
        own.named(THUNDERTRAP_TRAINER, ZoneName::Battlefield, Side::You)
            .is_empty()
    );
    assert_eq!(spellbinders(&own), 1);
}

/// An opponent that casts whatever it can, and otherwise takes each decision's first
/// candidate.
struct Casts(u32);

impl<L: Layout> Agent<L> for Casts {
    fn choose(&mut self, _: Observation<'_, L>, d: &Decision<L>) -> Choice {
        self.0 += 1;
        let cast = match &d.candidates {
            Candidates::Options(options) => {
                (options.iter()).position(|o| matches!(o, Opt::Cast { .. }))
            }
            Candidates::Objects(_) => None,
        };
        Choice(cast.unwrap_or(0) as u16)
    }

    fn next_decision(&self) -> Option<AgentDecision> {
        Some(AgentDecision(self.0))
    }

    fn rewind(&mut self, to: AgentDecision) {
        self.0 = to.0;
    }
}

/// P0 casts a Mana Crypt, P1 answers it with Mana Tithe, and P0 holds a Counterspell.
const TITHED: Scenario = Scenario::new("stack targets")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[MANA_CRYPT, COUNTERSPELL])
    .battlefield(P0, &[Permanent::new(ISLAND), Permanent::new(ISLAND)])
    .hand(P1, &[MANA_TITHE])
    .battlefield(P1, &[Permanent::new(PLAINS)])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// Among spells on the stack alone, the one opponent's spell opens picked, and the pick waits
/// for the button: a click on another spell changes it, and none answers.
#[test]
fn a_target_among_spells_opens_on_the_opponents_and_waits_for_its_button() {
    let tithed = || Play::against(&TITHED, Some(Box::new(Casts(0))));
    let mut play = tithed();
    let crypt = play.named(MANA_CRYPT, ZoneName::Hand, Side::You)[0];
    let counterspell = play.named(COUNTERSPELL, ZoneName::Hand, Side::You)[0];
    assert_eq!(play.on(crypt), None, "the Mana Crypt is cast");
    let tithe = play.named(MANA_TITHE, ZoneName::Stack, Side::Opponent)[0];
    assert_eq!(play.on(counterspell), None, "Counterspell is cast");
    assert!(play.decision().prompt.is(Purpose::Target));
    let open = format!(
        "pick target: Target Mana Tithe\n\
         picked 1 of 1: #{tithe}\n\
         eligible #{crypt}\n\
         hostile-target #{tithe}\n\
         confirm[Submit 1]*\n"
    );
    assert_eq!(play.prompt(), open);
    // The viewer's own spell takes the pick's place, and a second click on it leaves none.
    assert_eq!(
        play.on(crypt).unwrap(),
        format!(
            "pick target: Target Mana Crypt\n\
             picked 1 of 1: #{crypt}\n\
             eligible #{tithe}\n\
             friendly-target #{crypt}\n\
             confirm[Submit 1]*\n"
        )
    );
    assert_eq!(
        play.on(crypt).unwrap(),
        format!(
            "pick target: Choose a target.\n\
             picked 0 of 1\n\
             eligible {}\n\
             confirm(Submit 0)\n",
            cards(&[crypt, tithe]),
        )
    );
    assert_eq!(play.on(tithe).as_deref(), Some(open.as_str()));
    // The button answers, as picking the Mana Tithe does.
    let mut picked = tithed();
    picked.choose(|c| matches!(c, Candidate::Cast { slot, .. } if *slot == crypt));
    picked.choose(|c| matches!(c, Candidate::Cast { slot, .. } if *slot == counterspell));
    picked.choose(|c| *c == Candidate::Object { slot: tithe });
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
}

/// P0 casts Snapcaster Mage with an instant and a sorcery in their graveyard.
const SNAPCASTER: Scenario = Scenario::new("graveyard targets")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[SNAPCASTER_MAGE])
    .battlefield(P0, &[Permanent::new(ISLAND), Permanent::new(ISLAND)])
    .graveyard(P0, &[COUNTERSPELL, STOCK_UP])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// Targets in a graveyard are laid out in a tray, in the order the decision offers them, and
/// a click on one answers as a click on a permanent does.
#[test]
fn a_target_in_a_graveyard_is_picked_from_a_tray() {
    let mut play = Play::scenario(&SNAPCASTER);
    let snapcaster = play.named(SNAPCASTER_MAGE, ZoneName::Hand, Side::You)[0];
    let counterspell = play.named(COUNTERSPELL, ZoneName::Graveyard, Side::You)[0];
    let stock_up = play.named(STOCK_UP, ZoneName::Graveyard, Side::You)[0];
    assert_eq!(play.on(snapcaster), None, "Snapcaster Mage is cast");
    while !play.decision().prompt.is(Purpose::Target) {
        play.choose(|_| true);
    }
    let offered: Vec<String> = (play.decision().candidates.iter())
        .map(|c| match c.option {
            Candidate::Object { slot } => format!("#{slot}"),
            ref other => panic!("{other:?} is no card"),
        })
        .collect();
    assert_eq!(
        play.prompt(),
        format!(
            "pick target: Choose a target.\n\
             tray {}\n\
             picked 0 of 1\n\
             eligible {}\n",
            offered.join(" "),
            cards(&[counterspell, stock_up]),
        )
    );
    let mut picked = Play::scenario(&SNAPCASTER);
    picked.choose(|c| matches!(c, Candidate::Cast { .. }));
    while !picked.decision().prompt.is(Purpose::Target) {
        picked.choose(|_| true);
    }
    picked.choose(|c| *c == Candidate::Object { slot: stock_up });
    assert_eq!(play.on(stock_up), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
}

/// P0's Teferi, Time Raveler, with an artifact and a creature of P1's to return.
const TEFERI_RETURNS: Scenario = Scenario::new("up to one target")
    .at(3, P0, TurnStep::PrecombatMain)
    .battlefield(P0, &[Permanent::new(TEFERI_TIME_RAVELER)])
    .battlefield(
        P1,
        &[
            Permanent::new(STAFF_OF_THE_STORYTELLER),
            Permanent::new(ELITE_SPELLBINDER),
        ],
    )
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// Teferi's −3, "Return up to one target artifact, creature, or enchantment to its owner's
/// hand": its third ability.
fn minus_three(c: &Candidate) -> bool {
    matches!(c, Candidate::Activate { ability: 2, .. })
}

/// "Up to one target" is one prompt: the count is taken with the target. A click on a target
/// announces one target and then it, as one answer to take back; the button announces none.
#[test]
fn up_to_one_target_is_one_prompt_whose_click_announces_the_count() {
    let mut play = Play::scenario(&TEFERI_RETURNS);
    let staff = play.named(
        STAFF_OF_THE_STORYTELLER,
        ZoneName::Battlefield,
        Side::Opponent,
    )[0];
    let spellbinder = play.named(ELITE_SPELLBINDER, ZoneName::Battlefield, Side::Opponent)[0];
    // Teferi has two loyalty abilities to choose between, a prompt the session does not hold
    // yet: the −3 is picked as its candidate.
    play.choose(minus_three);
    assert_eq!(play.decision().kind, Kind::TargetCount);
    let open = format!(
        "pick target: Choose a target.\n\
         picked 0 of 0..1\n\
         eligible {}\n\
         decline[Submit 0]\n",
        cards(&[staff, spellbinder]),
    );
    assert_eq!(play.prompt(), open);
    let before = play.game.replay().log.len();
    let mut picked = Play::scenario(&TEFERI_RETURNS);
    picked.choose(minus_three);
    picked.choose(|c| *c == Candidate::Number { n: 1 });
    picked.choose(|c| *c == Candidate::Object { slot: staff });
    assert_eq!(play.on(staff), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert!(
        play.named(
            STAFF_OF_THE_STORYTELLER,
            ZoneName::Battlefield,
            Side::Opponent
        )
        .is_empty(),
        "the Staff is returned"
    );
    // The count is the player's answer and the target its continuation.
    assert_eq!(
        choices(&play, before)[..2],
        [(SavedKind::TargetCount, false), (SavedKind::Target, true)]
    );
    // One undo takes back the whole prompt, which opens afresh.
    play.view = play.game.undo().unwrap();
    assert_eq!(play.prompt(), open);
    assert_eq!(play.decision().prompt.revision, 0);

    // The button announces no target: Teferi's −3 only draws.
    let mut none = Play::scenario(&TEFERI_RETURNS);
    none.choose(minus_three);
    picked = Play::scenario(&TEFERI_RETURNS);
    picked.choose(minus_three);
    picked.choose(|c| *c == Candidate::Number { n: 0 });
    assert_eq!(none.press(ButtonId::Decline), None);
    assert_eq!(none.game.state(), picked.game.state());
    assert_eq!(none.game.replay().log, picked.game.replay().log);
}

/// The human's turn, with Teferi, Hero of Dominaria to +1: a Plains, an Island and a Tundra,
/// all tapped, and a Counterspell in hand.
const TEFERI_UNTAPS: Scenario = Scenario::new("untap")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[COUNTERSPELL])
    .battlefield(
        P0,
        &[
            Permanent::new(TEFERI_HERO_OF_DOMINARIA),
            Permanent::new(PLAINS).tapped(),
            Permanent::new(ISLAND).tapped(),
            Permanent::new(TUNDRA).tapped(),
        ],
    )
    .battlefield(P1, &[Permanent::new(ISLAND).tapped()])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// Plays Teferi's +1 and passes to the end step, where its untap asks.
fn to_the_untap(play: &mut Play) {
    play.choose(|c| matches!(c, Candidate::Activate { ability: 0, .. }));
    while play.decision().kind == Kind::Priority {
        play.choose(|c| *c == Candidate::Pass);
    }
    assert!(play.decision().prompt.is(Purpose::Untap));
}

/// An untap of up to two lands opens with the viewer's own lands picked that their hand
/// wants most; clicks pick and unpick, and the button says how many and answers.
#[test]
fn an_untap_opens_with_its_lands_picked_and_confirms_by_its_button() {
    let mut play = Play::scenario(&TEFERI_UNTAPS);
    to_the_untap(&mut play);
    let plains = play.named(PLAINS, ZoneName::Battlefield, Side::You)[0];
    let island = play.named(ISLAND, ZoneName::Battlefield, Side::You)[0];
    let tundra = play.named(TUNDRA, ZoneName::Battlefield, Side::You)[0];
    let theirs = play.named(ISLAND, ZoneName::Battlefield, Side::Opponent)[0];
    // Counterspell costs {U}{U}: only the Island and the Tundra together pay for it, so they
    // open picked. Two are picked, so no other land can be until one is unpicked.
    assert_eq!(
        play.prompt(),
        format!(
            "pick untap: Untap up to two lands.\n\
             picked 2 of 0..2: {both}\n\
             selected {both}\n\
             confirm[Submit 2]* decline[Submit 0]\n",
            both = cards(&[island, tundra]),
        )
    );
    // Unpicking the Island leaves one picked and the rest to pick, the opponent's land too.
    assert_eq!(
        play.on(island).unwrap(),
        format!(
            "pick untap: Untap up to two lands.\n\
             picked 1 of 0..2: #{tundra}\n\
             eligible {}\n\
             selected #{tundra}\n\
             confirm[Submit 1]* decline[Submit 0]\n",
            cards(&[plains, island, theirs]),
        )
    );
    assert_eq!(
        play.on(plains).unwrap(),
        format!(
            "pick untap: Untap up to two lands.\n\
             picked 2 of 0..2: #{tundra} #{plains}\n\
             selected {}\n\
             confirm[Submit 2]* decline[Submit 0]\n",
            cards(&[tundra, plains]),
        )
    );
    // A third pick does nothing.
    let full = play.prompt();
    assert_eq!(play.on(island).unwrap(), full);
    assert_eq!(play.decision().prompt.revision, 2, "two clicks changed it");
    // The button untaps the two picked, as picking each as the engine offers it does: the
    // engine asks no more once two are chosen.
    let before = play.game.replay().log.len();
    let mut picked = Play::scenario(&TEFERI_UNTAPS);
    to_the_untap(&mut picked);
    picked.choose(|c| *c == Candidate::Object { slot: plains });
    picked.choose(|c| *c == Candidate::Object { slot: tundra });
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    // The first pick is the player's answer and the second its continuation.
    let untaps = choices(&play, before);
    assert_eq!(
        untaps[..2],
        [
            (SavedKind::ChooseUpTo, false),
            (SavedKind::ChooseUpTo, true)
        ]
    );

    // With none picked the confirm button has nothing to say, and the other untaps none.
    let mut none = Play::scenario(&TEFERI_UNTAPS);
    to_the_untap(&mut none);
    none.on(island);
    assert_eq!(
        none.on(tundra).unwrap(),
        format!(
            "pick untap: Untap up to two lands.\n\
             picked 0 of 0..2\n\
             eligible {}\n\
             confirm(Submit 0) decline[Submit 0]*\n",
            cards(&[plains, island, tundra, theirs]),
        )
    );
    picked = Play::scenario(&TEFERI_UNTAPS);
    to_the_untap(&mut picked);
    picked.choose(|c| *c == Candidate::Done);
    assert_eq!(none.press(ButtonId::Decline), None);
    assert_eq!(none.game.state(), picked.game.state());
}

/// As [`TEFERI_UNTAPS`], with a second Island tapped, a Plains untapped, and a Reprieve to
/// hold up.
const TEFERI_UNTAPS_FOR_REPRIEVE: Scenario = Scenario::new("untap for a white instant")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[REPRIEVE])
    .battlefield(
        P0,
        &[
            Permanent::new(TEFERI_HERO_OF_DOMINARIA),
            Permanent::new(PLAINS).tapped(),
            Permanent::new(ISLAND).tapped(),
            Permanent::new(ISLAND).tapped(),
            Permanent::new(TUNDRA).tapped(),
            Permanent::new(PLAINS),
        ],
    )
    .battlefield(P1, &[Permanent::new(ISLAND).tapped()])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// Where any land pays for what the hand holds, the untap opens on those that make the most
/// colors of mana: a dual land with the first of the rest.
#[test]
fn an_untap_prefers_the_lands_that_make_the_most_colors() {
    let mut play = Play::scenario(&TEFERI_UNTAPS_FOR_REPRIEVE);
    to_the_untap(&mut play);
    let tundra = play.named(TUNDRA, ZoneName::Battlefield, Side::You)[0];
    let Some(ShapeView::Pick { picked, .. }) = &play.decision().prompt.state else {
        panic!("an untap is a pick the session holds");
    };
    // Reprieve costs {1}{W} and a Plains is untapped: any one land pays for it.
    assert_eq!(picked.len(), 2);
    assert!(picked.contains(&play::card(tundra)), "{picked:?}");
}

/// Two Teferis under one player's control, one of them tapped down a loyalty: the legend
/// rule asks which to keep.
const TWO_TEFERIS: Scenario = Scenario::new("legend rule")
    .at(3, P0, TurnStep::PrecombatMain)
    .battlefield(
        P0,
        &[
            Permanent::new(TEFERI_HERO_OF_DOMINARIA),
            Permanent::new(TEFERI_HERO_OF_DOMINARIA).tapped(),
        ],
    )
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// One permanent to choose among those on the table is a click: the legend to keep.
#[test]
fn the_legend_to_keep_is_picked_by_a_click() {
    let mut play = Play::scenario(&TWO_TEFERIS);
    assert!(play.decision().prompt.is(Purpose::LegendRule));
    let teferis = play.named(TEFERI_HERO_OF_DOMINARIA, ZoneName::Battlefield, Side::You);
    assert_eq!(
        play.prompt(),
        format!(
            "pick legend-rule: Choose the legendary permanent that you wish to keep. All others \
             will go to their owner's graveyard.\n\
             picked 0 of 1\n\
             eligible {}\n",
            cards(&teferis),
        )
    );
    let mut picked = Play::scenario(&TWO_TEFERIS);
    picked.choose(|c| *c == Candidate::Object { slot: teferis[1] });
    assert_eq!(play.on(teferis[1]), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(play.object(teferis[0]).zone, ZoneName::Graveyard);
    assert_eq!(play.object(teferis[1]).zone, ZoneName::Battlefield);
}

/// A click names the decision and the state of its prompt it was made on: one on another
/// decision or an earlier state is refused, and so is one where the session holds no state.
#[test]
fn a_click_on_another_decision_or_an_earlier_state_is_refused() {
    let mut play = Play::scenario(&TEFERI_UNTAPS);
    to_the_untap(&mut play);
    let plains = play.named(PLAINS, ZoneName::Battlefield, Side::You)[0];
    let stale = ClickRequest {
        decision: play.decision().id,
        revision: 0,
        click: Click::On {
            on: play::card(plains),
        },
    };
    let island = play.named(ISLAND, ZoneName::Battlefield, Side::You)[0];
    // The Island opens picked: a click unpicks it.
    play.on(island).unwrap();
    assert_eq!(
        play.game.click(stale),
        Err(SessionError::StalePrompt {
            revision: 0,
            open: 1
        })
    );
    let mut other = stale;
    other.decision.seq += 1;
    assert!(matches!(
        play.game.click(other),
        Err(SessionError::StaleDecision { .. })
    ));
    // A view drawn again shows the prompt as the clicks left it.
    let view = play.game.set_stops(StopsSetting::SMART).unwrap();
    let Status::Deciding { decision } = &view.status else {
        panic!("the untap is open");
    };
    assert_eq!(decision.prompt.revision, 1);
    assert_eq!(*decision.prompt, *play.decision().prompt);
}

/// The click command through a session, as the app sends it: the reply is the prompt's new
/// state, the latest view keeps it for a UI that takes the game up again, and a click that
/// answers closes the decision and plays the game on.
#[test]
fn a_session_takes_clicks_and_its_latest_view_keeps_the_prompt() {
    let (table, state) = beryl_scenario::position(&CARDS, &TEFERI_UNTAPS).unwrap();
    let random = beryl_search::agent("random").unwrap();
    let (sent, views) = channel();
    let sessions = Sessions::new(None, move |update| {
        let _ = sent.send(update);
    });
    sessions
        .start_prepared(Prepared {
            opponent: (random.new)(&table, 1, Budget::DEFAULT),
            stand_in: (random.new)(&table, 2, Budget::DEFAULT),
            // The human answers their own decisions from the first.
            hand_over: Some(Box::new(|| true)),
            scripted: false,
            table,
            state,
            human: Seat::P0,
            stops: StopsSetting::SMART,
        })
        .unwrap();
    // The view of the human's next decision.
    let deciding = || loop {
        let view = views.recv().unwrap().view;
        if let Status::Deciding { decision } = view.status {
            break decision;
        }
    };
    let index = |decision: &DecisionView, wanted: &dyn Fn(&Candidate) -> bool| {
        let at = (decision.candidates.iter()).position(|c| wanted(&c.option));
        CandidateIndex(at.unwrap() as u16)
    };
    let mut decision = deciding();
    let plus = index(&decision, &|c| {
        matches!(c, Candidate::Activate { ability: 0, .. })
    });
    sessions.choose(decision.id, plus).unwrap();
    decision = deciding();
    while decision.kind == Kind::Priority {
        let pass = index(&decision, &|c| *c == Candidate::Pass);
        sessions.choose(decision.id, pass).unwrap();
        decision = deciding();
    }
    assert!(decision.prompt.is(Purpose::Untap));
    let Some(ShapeView::Pick { picked, .. }) = &decision.prompt.state else {
        panic!("an untap is a pick the session holds");
    };
    // Unpicking a land changes the prompt and nothing else.
    let click = |revision, click| ClickRequest {
        decision: decision.id,
        revision,
        click,
    };
    let reply = sessions.click(click(0, Click::On { on: picked[0] }));
    let Ok(ClickReply::Open { prompt }) = reply else {
        panic!("the click changes the prompt: {reply:?}");
    };
    assert_eq!(prompt.revision, 1);
    let Status::Deciding { decision: latest } = sessions.current().unwrap().view.status else {
        panic!("the untap is open");
    };
    assert_eq!(latest.prompt, prompt);
    assert_eq!(latest.id, decision.id);
    // The button answers: the decision is closed, and the game plays on to another.
    let confirm = Click::Button {
        button: ButtonId::Confirm,
    };
    assert_eq!(sessions.click(click(1, confirm)), Ok(ClickReply::Answered));
    assert_eq!(
        sessions.click(click(1, confirm)),
        Err(SessionError::NotDeciding)
    );
    assert_ne!(deciding().id, decision.id);
}

fn owned(deck: &[(u8, &str)]) -> Vec<(u8, String)> {
    deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

/// A game played by clicks wherever the session holds the prompt's state, to a Time Spiral
/// whose untap of six interchangeable Islands is one click's run of picks, saves a record
/// that replays to the same game: the record holds the picks, never the clicks.
#[test]
fn a_save_made_through_clicks_replays() {
    let deck = owned(&[(40, ISLAND), (20, TIME_SPIRAL)]);
    let start = StartGame {
        seed: 7,
        decks: [deck.clone(), deck],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::SMART,
    };
    let mut play = Play::seeded(&start);
    let mut untapped = 0;
    for _ in 0..600 {
        if untapped > 0 || matches!(play.view.status, Status::Over { .. }) {
            break;
        }
        let prompt = &play.decision().prompt;
        if prompt.is(Purpose::Untap) {
            let Some(ShapeView::Pick { picked, .. }) = &prompt.state else {
                panic!("an untap is a pick the session holds");
            };
            let picked = picked.len();
            let before = play.game.replay().log.len();
            assert_eq!(play.press(ButtonId::Confirm), None);
            untapped = choices(&play, before).len();
            assert!(
                picked >= 2 && untapped >= picked,
                "{untapped} picks for {picked}"
            );
            continue;
        }
        if prompt.state.is_none() {
            // A prompt whose state the session does not hold yet: its first candidate.
            play.choose(|_| true);
            continue;
        }
        // At a priority: cast a Time Spiral, or play an Island, or pass.
        let card = [TIME_SPIRAL, ISLAND]
            .into_iter()
            .filter_map(|name| play.named(name, ZoneName::Hand, Side::You).first().copied())
            .find(|&slot| prompt.marks.iter().any(|m| m.on == play::card(slot)));
        let answered = match card {
            Some(card) => play.on(card),
            None => play.press(ButtonId::Decline),
        };
        assert_eq!(answered, None);
    }
    assert!(untapped > 0, "no Time Spiral resolved");
    let (replayed, view) = Game::from_replay(&CARDS, play.game.replay(), SESSION).unwrap();
    assert_eq!(replayed.state(), play.game.state());
    assert_eq!(replayed.replay().log, play.game.replay().log);
    assert_eq!(view.status, play.view.status);
}
