//! A player game that outgrows the `Small` layout goes on in `Large` (architecture §4.1):
//! it never ends stalled for a capacity, and plays on to its end. One game per kind of
//! capacity: a queue of the state, the object slots, a queue nested in a value, a decision's
//! options, and the mana solver's sources and classes of sources.

use std::collections::VecDeque;

use beryl_card::effect::build::{create_tokens, permit_cast, sequence, you};
use beryl_card::{CastTerms, Duration, Effect};
use beryl_cards::CARDS;
use beryl_cards::names::{
    COUNTERSPELL, ISLAND, OVERLORD_OF_THE_MISTMOORS, PLAINS, SNAPCASTER_MAGE, STRIP_MINE,
    WINDS_OF_ABANDON,
};
use beryl_cards::tokens::TREASURE;
use beryl_engine::card::{
    Ability, CardDef, Castings, Colors, Face, FaceIx, ManaCost, Objects, Players, Trigger, Types,
};
use beryl_engine::filter::Filter;
use beryl_engine::state::{EFFECTS, ObjectFlags, Pending};
use beryl_engine::{
    CardTable, Choice, Decision, DecisionKind, GameTable, Large, Layout, Observation, Opt, State,
    TurnStep, Zone,
};
use beryl_scenario::{Action, P0, P1, Permanent, Scenario};
use beryl_search::{Agent, AgentDecision, Budget, GameAgent};
use beryl_session::protocol::{OutcomeView, PlayerView, Status};
use beryl_session::{Game, Prepared, Seat, SessionId, StopsSetting};

/// Casts each card its script names at its first priority to cast it. Otherwise it takes the
/// first candidate (passing priority, the first target), or, if `eager`, attacks with every
/// creature it can and takes every card a search offers.
struct Script {
    table: GameTable,
    casts: VecDeque<Action>,
    eager: bool,
}

impl Script {
    fn new(table: &GameTable, casts: &[Action], eager: bool) -> Box<Script> {
        Box::new(Script {
            table: table.clone(),
            casts: casts.iter().copied().collect(),
            eager,
        })
    }
}

impl<L: Layout> Agent<L> for Script {
    fn choose(&mut self, obs: Observation<'_, L>, d: &Decision<L>, _: AgentDecision) -> Choice {
        if d.kind == DecisionKind::Priority
            && let Some(action) = self.casts.front()
            && let Some(c) = beryl_scenario::answer(&self.table, obs.unmasked_root(), d, action)
        {
            self.casts.pop_front();
            return c;
        }
        let more = |o: Opt<L>| matches!(o, Opt::Attack(_) | Opt::Object(_));
        if self.eager
            && let Some(c) = (0..d.len() as u16)
                .map(Choice)
                .find(|&c| d.candidates.option(c).is_some_and(more))
        {
            return c;
        }
        Choice(0)
    }
}

/// The game of `scenario` between `human` and `opponent`.
fn game(
    cards: &'static CardTable,
    scenario: &Scenario,
    human: impl Fn(&GameTable) -> Box<Script>,
    opponent: impl Fn(&GameTable) -> Box<dyn GameAgent + Send>,
    stops: StopsSetting,
) -> Game {
    let (table, state) = beryl_scenario::position(cards, scenario).unwrap();
    Game::prepared(
        Prepared {
            opponent: opponent(&table),
            stand_in: human(&table),
            scripted: false,
            hand_over: None,
            state,
            human: Seat::P0,
            stops,
            table,
        },
        SessionId(1),
    )
}

/// The random agent, as the opponent.
fn random(table: &GameTable) -> Box<dyn GameAgent + Send> {
    let random = beryl_search::agent("random").unwrap();
    (random.new)(table, 1, Budget::DEFAULT)
}

/// Plays `game` to its end, the stand-in answering for the human, and calls `watch` with each
/// view and the game as it then stands in `Large`, once widened.
fn play(game: &mut Game, mut watch: impl FnMut(&PlayerView, Option<&State<Large>>)) -> OutcomeView {
    loop {
        let Some(view) = game.next_view() else {
            game.answer_by_stand_in();
            continue;
        };
        watch(&view, game.large());
        match view.status {
            Status::Over { outcome } => return outcome,
            Status::Deciding { .. } => {
                game.answer_by_stand_in();
            }
            _ => {}
        }
    }
}

/// A test spell that lets its controller cast each card in their graveyard this turn, one
/// more time over than a `Small` state has continuous effects ([`EFFECTS`]): each permission
/// is an effect of its own. No card of the pool starts that many at once.
static OPEN_VAULT: CardDef = CardDef {
    name: "Open Vault",
    faces: &[Face {
        cost: ManaCost::NONE,
        types: Types::SORCERY,
        colors: Colors::NONE,
        abilities: &[Ability::spell().effect(&sequence(&[PERMIT; EFFECTS + 1]))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "You may cast each card in your graveyard this turn", once.
const PERMIT: Effect = permit_cast(
    you(),
    Objects::All {
        zone: Zone::Graveyard,
        filter: Filter::Whose(&you()),
    },
    Duration::EndOfTurn,
    CastTerms::PLAIN,
);

/// The Counterspell in the human's graveyard is what the vault permits. Short libraries end
/// the game soon after.
const VAULT: Scenario = Scenario::new("vault")
    .at(1, P0, TurnStep::PrecombatMain)
    .permanents(P0, &[Permanent::new(ISLAND)])
    .hand(P0, &["Open Vault"])
    .graveyard(P0, &[COUNTERSPELL])
    .library(P0, &[ISLAND, ISLAND, ISLAND])
    .permanents(P1, &[Permanent::new(PLAINS)])
    .library(P1, &[PLAINS, PLAINS]);

#[test]
fn a_game_past_a_small_capacity_plays_on_in_large() {
    let cards: Vec<&'static CardDef> = vec![
        &OPEN_VAULT,
        CARDS.get(COUNTERSPELL).unwrap(),
        CARDS.get(ISLAND).unwrap(),
        CARDS.get(PLAINS).unwrap(),
    ];
    let cards: &'static CardTable = Box::leak(Box::new(CardTable::new(cards.leak())));
    let mut game = game(
        cards,
        &VAULT,
        |t| Script::new(t, &[Action::cast(P0, "Open Vault")], false),
        random,
        StopsSetting::default(),
    );
    let mut most_permissions = 0;
    let outcome = play(&mut game, |_, large| {
        if let Some(s) = large {
            most_permissions = most_permissions.max(s.cast_permissions().count());
        }
    });
    assert!(
        matches!(outcome, OutcomeView::Win { .. }),
        "the game ends in a win, not a stall: {outcome:?}"
    );
    assert_eq!(most_permissions, EFFECTS + 1, "every permission was kept");
}

/// Forty-five Overlords of the Mistmoors attack each turn, and each attack makes two Insect
/// tokens: by the second attack, more tokens than `Small` has slots for (135, after both
/// decks' 120).
const OVERLORD_ARMY: Scenario = Scenario::new("overlord-army")
    .at(1, P0, TurnStep::PrecombatMain)
    .permanents(P0, &[Permanent::new(OVERLORD_OF_THE_MISTMOORS); 45])
    .library(P0, &[PLAINS; 5])
    .life(P1, 1000)
    .permanents(P1, &[Permanent::new(PLAINS)])
    .library(P1, &[PLAINS; 3]);

#[test]
fn tokens_past_the_small_slots_play_on_in_large() {
    let mut game = game(
        &CARDS,
        &OVERLORD_ARMY,
        |t| Script::new(t, &[], true),
        random,
        StopsSetting::default(),
    );
    let mut most_tokens = 0;
    let outcome = play(&mut game, |_, large| {
        if let Some(s) = large {
            let tokens = s
                .battlefield()
                .iter()
                .filter(|&&slot| s.object(slot).flags().has(ObjectFlags::TOKEN))
                .count();
            most_tokens = most_tokens.max(tokens);
        }
    });
    assert!(
        matches!(outcome, OutcomeView::Win { winner, .. } if winner == beryl_session::protocol::Side::You),
        "the army wins, not a stall: {outcome:?}"
    );
    assert!(
        most_tokens > 135,
        "more tokens than Small's slots: {most_tokens}"
    );
}

/// The opponent's seventeen Overlords attack, making thirty-four Insects; the human's
/// overloaded Winds of Abandon exiles all fifty-one, and the opponent searches for a basic land
/// per creature. Each card they take is an answer the paused resolution records: more than a
/// `Small` choice record holds (32).
const ABANDONED_ARMY: Scenario = Scenario::new("abandoned-army")
    .at(1, P1, TurnStep::PrecombatMain)
    .permanents(P0, &[Permanent::new(PLAINS); 6])
    .hand(P0, &[WINDS_OF_ABANDON])
    .library(P0, &[PLAINS; 3])
    .life(P0, 1000)
    .permanents(P1, &[Permanent::new(OVERLORD_OF_THE_MISTMOORS); 17])
    .library(P1, &[PLAINS; 43]);

#[test]
fn a_resolution_past_a_small_choice_record_plays_on_in_large() {
    let overloaded = Action::Cast {
        player: P0,
        card: WINDS_OF_ABANDON,
        face: FaceIx::FRONT,
        costs: Castings::only(0),
    };
    let mut game = game(
        &CARDS,
        &ABANDONED_ARMY,
        |t| Script::new(t, &[overloaded], false),
        |t| Script::new(t, &[], true),
        StopsSetting::default(),
    );
    let mut longest_record = 0;
    let outcome = play(&mut game, |_, large| {
        if let Some(s) = large
            && let Pending::Resolving(r) = s.pending()
        {
            longest_record = longest_record.max(r.record.len());
        }
    });
    assert!(
        matches!(outcome, OutcomeView::Win { winner, .. } if winner == beryl_session::protocol::Side::You),
        "the opponent searched their library empty and loses drawing, not to a stall: \
         {outcome:?}"
    );
    assert!(
        longest_record > 32,
        "more answers than Small's record holds: {longest_record}"
    );
}

/// How many "whenever you cast a spell" abilities each chorus has. A face holds at most 64,
/// so two choruses make more triggers than a `Small` state holds waiting (64), and more
/// options to order them than a `Small` decision holds (64).
const VOICES: usize = 33;

/// A test card with [`VOICES`] distinct cast abilities, each doing nothing; [`ECHOES`] is
/// another. Their triggers are not interchangeable, so ordering them offers each one. No
/// implemented cards trigger that often at once; a larger card pool may.
static VOICES_CARD: CardDef = chorus("Chorus of Voices");

static ECHOES: CardDef = chorus("Chorus of Echoes");

/// A spell to cast for nothing.
static PEBBLE: CardDef = CardDef {
    name: "Pebble",
    faces: &[Face {
        cost: ManaCost::NONE,
        types: Types::ARTIFACT,
        colors: Colors::NONE,
        ..Face::EMPTY
    }],
    tokens: &[],
};

const fn chorus(name: &'static str) -> CardDef {
    CardDef {
        name,
        faces: &[Face {
            cost: ManaCost::NONE,
            types: Types::ARTIFACT,
            colors: Colors::NONE,
            abilities: &[VOICE; VOICES],
            ..Face::EMPTY
        }],
        tokens: &[],
    }
}

/// A chorus's ability: "Whenever you cast a spell, nothing happens."
const VOICE: Ability =
    Ability::triggered(&Trigger::casts(Players::You, Filter::Any)).effect(&Effect::NOTHING);

const CHORUSES: Scenario = Scenario::new("choruses")
    .at(1, P0, TurnStep::PrecombatMain)
    .permanents(
        P0,
        &[
            Permanent::new("Chorus of Voices"),
            Permanent::new("Chorus of Echoes"),
        ],
    )
    .hand(P0, &["Pebble"])
    .library(P0, &[ISLAND; 3])
    .permanents(P1, &[Permanent::new(PLAINS)])
    .library(P1, &[PLAINS; 2]);

#[test]
fn a_decision_past_small_options_plays_on_in_large() {
    let cards: Vec<&'static CardDef> = vec![
        &VOICES_CARD,
        &ECHOES,
        &PEBBLE,
        CARDS.get(ISLAND).unwrap(),
        CARDS.get(PLAINS).unwrap(),
    ];
    let cards: &'static CardTable = Box::leak(Box::new(CardTable::new(cards.leak())));
    // Under full control the human orders their own triggers.
    let stops = StopsSetting {
        full_control: true,
        ..StopsSetting::default()
    };
    let mut game = game(
        cards,
        &CHORUSES,
        |t| Script::new(t, &[Action::cast(P0, "Pebble")], false),
        random,
        stops,
    );
    let mut most_candidates = 0;
    let outcome = play(&mut game, |view, _| {
        if let Status::Deciding { decision } = &view.status {
            most_candidates = most_candidates.max(decision.candidates.len());
        }
    });
    assert!(
        matches!(outcome, OutcomeView::Win { .. }),
        "the game ends in a win, not a stall: {outcome:?}"
    );
    assert_eq!(
        most_candidates,
        2 * VOICES,
        "the first trigger order offers every trigger"
    );
    assert!(game.large().is_some(), "the game went on in Large");
}

/// The permanents `P0` controls in `s`.
fn controlled_by_p0(s: &State<Large>) -> usize {
    s.battlefield()
        .iter()
        .filter(|&&slot| s.controller(slot) == P0)
        .count()
}

const MINES_AND_ISLAND: [Permanent; 18] = {
    let mut lands = [Permanent::new(STRIP_MINE); 18];
    lands[17] = Permanent::new(ISLAND);
    lands
};

/// Seventeen Strip Mines: each has an ability besides its mana ability, so each is a class of
/// sources of its own, and with the Island more than a `Small` payment holds (16). Paying for
/// the Snapcaster Mage, or only checking that it can be paid, considers them all.
const STRIP_MINES: Scenario = Scenario::new("strip-mines")
    .at(1, P0, TurnStep::PrecombatMain)
    .permanents(P0, &MINES_AND_ISLAND)
    .hand(P0, &[SNAPCASTER_MAGE])
    .library(P0, &[ISLAND; 3])
    .permanents(P1, &[Permanent::new(PLAINS)])
    .library(P1, &[PLAINS; 2]);

#[test]
fn source_classes_past_small_play_on_in_large() {
    let mut game = game(
        &CARDS,
        &STRIP_MINES,
        |t| Script::new(t, &[Action::cast(P0, SNAPCASTER_MAGE)], false),
        random,
        StopsSetting::default(),
    );
    let mut most_permanents = 0;
    let outcome = play(&mut game, |_, large| {
        if let Some(s) = large {
            most_permanents = most_permanents.max(controlled_by_p0(s));
        }
    });
    assert!(
        matches!(outcome, OutcomeView::Win { .. }),
        "the game ends in a win, not a stall: {outcome:?}"
    );
    assert_eq!(
        most_permanents, 19,
        "the Snapcaster joined the Strip Mines and the Island"
    );
}

/// How many Treasures the hoard makes: with the human's Islands, more untapped sources than
/// any `Small` battlefield holds (128 permanents).
const TREASURES: u8 = 90;

/// A test card: "When this enters, create [`TREASURES`] Treasure tokens." No implemented card
/// makes so many at once; a larger card pool may.
static HOARD: CardDef = CardDef {
    name: "Hoard",
    faces: &[Face {
        cost: ManaCost::NONE,
        types: Types::ARTIFACT,
        colors: Colors::NONE,
        abilities: &[Ability::triggered(&Trigger::ENTERS).effect(&create_tokens(
            you(),
            &TREASURE,
            TREASURES,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[&TREASURE],
};

const HOARDER: Scenario = Scenario::new("hoarder")
    .at(1, P0, TurnStep::PrecombatMain)
    .permanents(P0, &[Permanent::new(ISLAND); 40])
    .hand(P0, &["Hoard", SNAPCASTER_MAGE])
    .library(P0, &[ISLAND; 3])
    .permanents(P1, &[Permanent::new(PLAINS)])
    .library(P1, &[PLAINS; 2]);

#[test]
fn sources_past_small_play_on_in_large() {
    let cards: Vec<&'static CardDef> = vec![
        &HOARD,
        CARDS.get(SNAPCASTER_MAGE).unwrap(),
        CARDS.get(ISLAND).unwrap(),
        CARDS.get(PLAINS).unwrap(),
    ];
    let cards: &'static CardTable = Box::leak(Box::new(CardTable::new(cards.leak())));
    let casts = [Action::cast(P0, "Hoard"), Action::cast(P0, SNAPCASTER_MAGE)];
    let mut game = game(
        cards,
        &HOARDER,
        |t| Script::new(t, &casts, false),
        random,
        StopsSetting::default(),
    );
    let mut most_permanents = 0;
    let outcome = play(&mut game, |_, large| {
        if let Some(s) = large {
            most_permanents = most_permanents.max(controlled_by_p0(s));
        }
    });
    assert!(
        matches!(outcome, OutcomeView::Win { .. }),
        "the game ends in a win, not a stall: {outcome:?}"
    );
    assert_eq!(
        most_permanents,
        40 + 1 + usize::from(TREASURES) + 1,
        "the Snapcaster joined the Islands, the hoard and its Treasures"
    );
}
