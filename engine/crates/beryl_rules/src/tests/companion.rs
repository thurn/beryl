//! Companions (the reveal before the game and the {3} special action), and a permanent's
//! permission to cast a spell from its controller's graveyard once each turn. The test
//! companion has Lurrus of the Dream-Den's abilities with a cost of plain white mana, since
//! the engine has no hybrid mana yet.

use super::{LAND, decide, fixture_slots};
use crate::card::{Ability, CardDef, Face, Keywords, ManaCost, Purpose, Types, mana};
use crate::ids::Seat;
use crate::state::{TurnStep, Zone, ZoneKey};
use crate::{
    CardTable, Choice, Decision, DecisionKind, Decklist, GameTable, Opt, Profile, Replay, Scratch,
    State, apply,
};
use beryl_card::{DeckCondition, Filter};
use beryl_kernel::format::Format;
use beryl_kernel::two_player::TwoPlayerSeat as _;

/// "A permanent spell with mana value 2 or less" and "each permanent card in your starting
/// deck has mana value 2 or less".
const PERMANENT: Filter = Filter::Types(Types::PERMANENT);
const SMALL_PERMANENT: Filter = Filter::All(&[PERMANENT, Filter::ManaValueAtMost(2)]);

static COMPANION: CardDef = CardDef {
    name: "Test Companion",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        cost: mana("{1}{W}{W}"),
        types: Types::CREATURE,
        power: Some(3),
        toughness: Some(2),
        keywords: Keywords::LIFELINK,
        abilities: &[
            Ability::companion(&DeckCondition {
                each: PERMANENT,
                has: Filter::ManaValueAtMost(2),
            }),
            Ability::cast_from_graveyard_each_turn(&SMALL_PERMANENT),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A creature card with this name and faces.
const fn creature(name: &'static str, faces: &'static [Face]) -> CardDef {
    CardDef {
        name,
        shape: beryl_card::definition::Shape::Single,
        faces,
        tokens: &[],
    }
}

/// A vanilla 2/2 creature face with this cost.
const fn vanilla(cost: ManaCost) -> Face {
    Face {
        cost,
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }
}

static BEAR: CardDef = creature("Test Bear", &[vanilla(mana("{1}{W}"))]);
static GIANT: CardDef = creature("Test Giant", &[vanilla(mana("{2}{W}"))]);
static HYDRA: CardDef = creature("Test Hydra", &[vanilla(mana("{X}{W}"))]);
static FLASH: CardDef = CardDef {
    name: "Test Flash",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        cost: mana("{W}"),
        types: Types::INSTANT,
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A two-mana creature with flash: a permanent spell the permission accepts that its
/// controller could also cast during an opponent's turn.
static FLASH_BEAR: CardDef = creature(
    "Test Flash Bear",
    &[Face {
        keywords: Keywords::FLASH,
        ..vanilla(mana("{1}{W}"))
    }],
);

static CARDS: CardTable = CardTable::new(&[
    &LAND,
    &COMPANION,
    &BEAR,
    &GIANT,
    &HYDRA,
    &FLASH,
    &FLASH_BEAR,
]);

const SMALL_DECK: &Decklist = &[(20, "Test Land"), (16, "Test Bear"), (4, "Test Flash")];
const BIG_DECK: &Decklist = &[(20, "Test Land"), (19, "Test Bear"), (1, "Test Giant")];
const LANDS: &Decklist = &[(40, "Test Land")];
const COMPANION_LIST: &Decklist = &[(1, "Test Companion")];

fn table(decks: [&Decklist; 2], sideboards: [&Decklist; 2]) -> GameTable {
    GameTable::with_sideboards(Format::CONSTRUCTED, &CARDS, decks, sideboards).unwrap()
}

fn choose(t: &GameTable, s: &mut State, c: Choice) {
    apply(s, t, &mut Scratch::new(), &mut Replay, c);
}

/// The choice of `d` that offers `o`.
fn offered(d: &Decision, o: impl Fn(Opt) -> bool) -> Option<Choice> {
    (0..d.len() as u16)
        .map(Choice)
        .find(|&c| d.candidates.option(c).is_some_and(&o))
}

/// Plays first and keeps, answering a companion question with `reveal`, and returns the
/// purposes asked and the first decision after them.
fn start(t: &GameTable, s: &mut State, reveal: bool) -> (Vec<(Purpose, Seat)>, Decision) {
    let mut asked = Vec::new();
    loop {
        let d = decide(t, s);
        let c = match d.prompt.purpose {
            Purpose::PlayOrDraw | Purpose::Mulligan => Choice(0),
            Purpose::Companion if reveal => Choice(0),
            Purpose::Companion => offered(&d, |o| o == Opt::Done).unwrap(),
            _ => return (asked, d),
        };
        asked.push((d.prompt.purpose, d.player));
        choose(t, s, c);
    }
}

/// The card named `name` in `player`'s library, moved to `zone`.
fn put(t: &GameTable, s: &mut State, player: Seat, name: &str, zone: Zone) -> crate::ids::Slot {
    let slot = s
        .library(player)
        .iter()
        .copied()
        .find(|&slot| s.definition_name(t, slot) == name)
        .unwrap();
    s.board().place(slot, zone).unwrap();
    slot
}

/// A game at `player`'s precombat main phase of turn 3, nothing else set up.
fn main_phase(t: &GameTable, player: Seat) -> State {
    let mut s = State::new(t, 5, Profile::EXACT).unwrap();
    s.board().begin_at(3, player, TurnStep::PrecombatMain);
    s
}

/// Answers every decision but priority with its first candidate, and returns the next
/// priority decision.
fn to_priority(t: &GameTable, s: &mut State) -> Decision {
    loop {
        let d = decide(t, s);
        if d.kind == DecisionKind::Priority {
            return d;
        }
        choose(t, s, Choice(0));
    }
}

#[test]
fn a_revealed_companion_is_put_into_hand_for_three_at_sorcery_speed() {
    let t = table([SMALL_DECK, LANDS], [COMPANION_LIST, &[]]);
    let mut s = State::new(&t, 7, Profile::EXACT).unwrap();
    let (asked, first) = start(&t, &mut s, true);
    // The question comes before the opening hands, and only the player with a companion is
    // asked.
    assert_eq!(asked[0].0, Purpose::PlayOrDraw);
    assert_eq!(asked[1], (Purpose::Companion, Seat::P0));
    assert!(
        asked[2..]
            .iter()
            .all(|&(purpose, _)| purpose == Purpose::Mulligan)
    );
    assert_eq!(first.kind, DecisionKind::Priority);
    assert_eq!(s.companion(&t, Seat::P0).map(|(index, _)| index), Some(0));
    assert_eq!(s.companion(&t, Seat::P1), None);

    // Set up P0's main phase with three lands, keeping the revealed companion.
    let mut s = {
        let mut fresh = main_phase(&t, Seat::P0);
        fresh.reveal_companion(Seat::P0, 0);
        fresh
    };
    for _ in 0..3 {
        put(&t, &mut s, Seat::P0, LAND.name, Zone::Battlefield);
    }
    let d = decide(&t, &mut s);
    let action = offered(&d, |o| o == Opt::Companion).expect("the companion action");
    choose(&t, &mut s, action);
    // The action used the stack for nothing.
    assert!(s.stack().is_empty());
    let hand: Vec<_> = fixture_slots(s.zone(ZoneKey::Of(Seat::P0, Zone::Hand)))
        .filter(|&slot| s.definition_name(&t, slot) == COMPANION.name)
        .collect();
    assert_eq!(hand.len(), 1);
    assert!(s.knows(Seat::P1, hand[0]), "the companion is revealed");
    assert_eq!(s.companion(&t, Seat::P0), None, "once a game");
    assert!(
        s.zone(ZoneKey::Of(Seat::P0, Zone::Battlefield))
            .all(|slot| s.object(slot).is_tapped())
    );
    assert_eq!(s.check_invariants(&t), Ok(()));

    // Three more lands: the companion is cast like any card in hand.
    for _ in 0..3 {
        put(&t, &mut s, Seat::P0, LAND.name, Zone::Battlefield);
    }
    s.board().set_priority(Seat::P0);
    let d = decide(&t, &mut s);
    assert!(offered(&d, |o| o == Opt::Companion).is_none());
    assert!(
        offered(
            &d,
            |o| matches!(o, Opt::Cast { slot, .. } if slot == hand[0])
        )
        .is_some()
    );
}

#[test]
fn the_companion_action_waits_for_an_empty_stack_in_its_players_main_phase() {
    let t = table([SMALL_DECK, LANDS], [COMPANION_LIST, &[]]);
    for active in Seat::BOTH {
        let mut s = main_phase(&t, active);
        s.reveal_companion(Seat::P0, 0);
        for _ in 0..4 {
            put(&t, &mut s, Seat::P0, LAND.name, Zone::Battlefield);
        }
        // Two instants, so that P0 has something to do with one on the stack.
        put(&t, &mut s, Seat::P0, FLASH.name, Zone::Hand);
        put(&t, &mut s, Seat::P0, FLASH.name, Zone::Hand);
        s.board().set_priority(Seat::P0);
        let d = decide(&t, &mut s);
        assert_eq!(d.player, Seat::P0);
        assert_eq!(
            offered(&d, |o| o == Opt::Companion).is_some(),
            active == Seat::P0,
            "offered only during its player's own turn"
        );
        // With an instant of their own on the stack, the player has priority but may not.
        let cast = offered(&d, |o| matches!(o, Opt::Cast { .. })).unwrap();
        choose(&t, &mut s, cast);
        let d = to_priority(&t, &mut s);
        assert_eq!((d.player, s.stack().len()), (Seat::P0, 1));
        assert!(offered(&d, |o| matches!(o, Opt::Cast { .. })).is_some());
        assert!(offered(&d, |o| o == Opt::Companion).is_none());
    }
    // Nor with too little mana.
    let mut s = main_phase(&t, Seat::P0);
    s.reveal_companion(Seat::P0, 0);
    for _ in 0..2 {
        put(&t, &mut s, Seat::P0, LAND.name, Zone::Battlefield);
    }
    let d = decide(&t, &mut s);
    assert!(offered(&d, |o| o == Opt::Companion).is_none());
}

#[test]
fn a_deck_with_a_three_mana_permanent_is_not_offered_the_companion() {
    let t = table([BIG_DECK, LANDS], [COMPANION_LIST, &[]]);
    let mut s = State::new(&t, 7, Profile::EXACT).unwrap();
    let (asked, _) = start(&t, &mut s, true);
    assert!(
        !asked
            .iter()
            .any(|&(purpose, _)| purpose == Purpose::Companion)
    );
    assert_eq!(s.companion(&t, Seat::P0), None);
}

#[test]
fn a_declined_companion_stays_outside_the_game_and_offers_no_action() {
    let t = table([SMALL_DECK, SMALL_DECK], [COMPANION_LIST, COMPANION_LIST]);
    let mut s = State::new(&t, 7, Profile::EXACT).unwrap();
    let (asked, _) = start(&t, &mut s, false);
    // Both players are asked, the starting player first.
    let starting = s.turn().starting_player();
    let companions: Vec<_> = asked
        .iter()
        .filter(|&&(purpose, _)| purpose == Purpose::Companion)
        .map(|&(_, player)| player)
        .collect();
    assert_eq!(companions, [starting, starting.opponent()]);
    assert_eq!(s.companion(&t, Seat::P0), None);
    assert_eq!(s.sideboard(&t, Seat::P0).count(), 1);
}

#[test]
fn a_companion_in_the_main_deck_is_an_ordinary_card() {
    let deck: &Decklist = &[(20, "Test Land"), (19, "Test Bear"), (1, "Test Companion")];
    let t = table([deck, LANDS], [&[], &[]]);
    let mut s = State::new(&t, 7, Profile::EXACT).unwrap();
    let (asked, _) = start(&t, &mut s, true);
    assert!(
        !asked
            .iter()
            .any(|&(purpose, _)| purpose == Purpose::Companion)
    );
    let mut s = main_phase(&t, Seat::P0);
    let companion = put(&t, &mut s, Seat::P0, COMPANION.name, Zone::Hand);
    for _ in 0..3 {
        put(&t, &mut s, Seat::P0, LAND.name, Zone::Battlefield);
    }
    let d = decide(&t, &mut s);
    assert!(offered(&d, |o| o == Opt::Companion).is_none());
    assert!(
        offered(
            &d,
            |o| matches!(o, Opt::Cast { slot, .. } if slot == companion)
        )
        .is_some()
    );
}

/// The cards of `d`'s casts from `player`'s graveyard, by name.
fn graveyard_casts(t: &GameTable, s: &State, d: &Decision, player: Seat) -> Vec<&'static str> {
    let graveyard = s.zone(ZoneKey::Of(player, Zone::Graveyard));
    let mut names: Vec<_> = (0..d.len() as u16)
        .filter_map(|c| match d.candidates.option(Choice(c)) {
            Some(Opt::Cast { slot, .. }) if graveyard.contains(slot) => {
                Some(s.definition_name(t, slot))
            }
            _ => None,
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Passes priority and answers every other decision with its first candidate until `player`
/// has priority in their precombat main phase of turn `turn` with an empty stack.
fn to_main_phase(t: &GameTable, s: &mut State, player: Seat, turn: u16) -> Decision {
    loop {
        let d = decide(t, s);
        if d.kind == DecisionKind::Priority
            && s.turn().number() == turn
            && d.player == player
            && s.turn().active() == player
            && s.turn().step() == TurnStep::PrecombatMain
            && s.stack().is_empty()
        {
            return d;
        }
        let c = offered(&d, |o| o == Opt::Pass)
            .or_else(|| offered(&d, |o| o == Opt::Done))
            .unwrap_or(Choice(0));
        choose(t, s, c);
    }
}

#[test]
fn a_permanent_lets_its_controller_cast_one_small_permanent_spell_from_their_graveyard_a_turn() {
    let deck: &Decklist = &[
        (20, "Test Land"),
        (4, "Test Bear"),
        (2, "Test Giant"),
        (3, "Test Flash"),
        (2, "Test Companion"),
    ];
    let t = table([deck, LANDS], [&[], &[]]);
    let mut s = main_phase(&t, Seat::P0);
    put(&t, &mut s, Seat::P0, COMPANION.name, Zone::Battlefield);
    for _ in 0..6 {
        put(&t, &mut s, Seat::P0, LAND.name, Zone::Battlefield);
    }
    let bear = put(&t, &mut s, Seat::P0, BEAR.name, Zone::Graveyard);
    put(&t, &mut s, Seat::P0, BEAR.name, Zone::Graveyard);
    put(&t, &mut s, Seat::P0, GIANT.name, Zone::Graveyard);
    put(&t, &mut s, Seat::P0, FLASH.name, Zone::Graveyard);
    // The opponent's graveyard is theirs alone.
    put(&t, &mut s, Seat::P1, LAND.name, Zone::Graveyard);
    // An instant in hand, so that P0 always has something to do but pass.
    put(&t, &mut s, Seat::P0, FLASH.name, Zone::Hand);

    let d = decide(&t, &mut s);
    // A permanent spell with mana value 2 or less: not the three-mana creature, not the
    // instant.
    assert_eq!(graveyard_casts(&t, &s, &d, Seat::P0), [BEAR.name]);
    let cast = offered(&d, |o| matches!(o, Opt::Cast { slot, .. } if slot == bear))
        .or_else(|| {
            offered(&d, |o| {
                matches!(o, Opt::Cast { slot, .. }
                    if s.definition_name(&t, slot) == BEAR.name
                        && s.object(slot).zone() == Zone::Graveyard)
            })
        })
        .unwrap();
    choose(&t, &mut s, cast);
    let d = to_priority(&t, &mut s);
    assert_eq!((d.player, s.stack().len()), (Seat::P0, 1));
    // The permission is used as the spell is cast: the second bear waits for the next turn.
    let pass = offered(&d, |o| o == Opt::Pass).unwrap();
    choose(&t, &mut s, pass);
    let d = to_priority(&t, &mut s);
    assert_eq!((d.player, s.stack().len()), (Seat::P0, 0));
    assert_eq!(s.turn().step(), TurnStep::PrecombatMain);
    assert_eq!(
        s.object(bear).zone(),
        Zone::Battlefield,
        "the bear resolved"
    );
    assert_eq!(graveyard_casts(&t, &s, &d, Seat::P0), Vec::<&str>::new());

    // A second permanent with the ability gives a second cast this turn.
    let mut s2 = s;
    put(&t, &mut s2, Seat::P0, COMPANION.name, Zone::Battlefield);
    let d = decide(&t, &mut s2);
    assert_eq!(graveyard_casts(&t, &s2, &d, Seat::P0), [BEAR.name]);

    // The next turn of its controller, the permission is unused again.
    let d = to_main_phase(&t, &mut s, Seat::P0, 5);
    assert_eq!(s.turn().number(), 5);
    assert_eq!(graveyard_casts(&t, &s, &d, Seat::P0), [BEAR.name]);
    assert_eq!(s.check_invariants(&t), Ok(()));
}

#[test]
fn the_graveyard_permission_works_only_during_its_controllers_turn() {
    let deck: &Decklist = &[
        (20, "Test Land"),
        (2, "Test Flash Bear"),
        (1, "Test Companion"),
    ];
    let t = table([deck, LANDS], [&[], &[]]);
    for active in Seat::BOTH {
        let mut s = main_phase(&t, active);
        put(&t, &mut s, Seat::P0, COMPANION.name, Zone::Battlefield);
        for _ in 0..4 {
            put(&t, &mut s, Seat::P0, LAND.name, Zone::Battlefield);
        }
        put(&t, &mut s, Seat::P0, FLASH_BEAR.name, Zone::Graveyard);
        // A flash creature in hand, so that P0 has something to do on either turn.
        put(&t, &mut s, Seat::P0, FLASH_BEAR.name, Zone::Hand);
        s.board().set_priority(Seat::P0);
        let d = decide(&t, &mut s);
        assert_eq!(d.player, Seat::P0);
        let expected: &[&str] = if active == Seat::P0 {
            &[FLASH_BEAR.name]
        } else {
            &[]
        };
        assert_eq!(
            graveyard_casts(&t, &s, &d, Seat::P0),
            expected,
            "\"During each of your turns\": offered only while P0 is the active player"
        );
    }
}

#[test]
fn a_graveyard_spell_with_x_is_cast_with_a_mana_value_the_permission_accepts() {
    let deck: &Decklist = &[(20, "Test Land"), (1, "Test Hydra"), (1, "Test Companion")];
    let t = table([deck, LANDS], [&[], &[]]);
    let mut s = main_phase(&t, Seat::P0);
    put(&t, &mut s, Seat::P0, COMPANION.name, Zone::Battlefield);
    for _ in 0..6 {
        put(&t, &mut s, Seat::P0, LAND.name, Zone::Battlefield);
    }
    put(&t, &mut s, Seat::P0, HYDRA.name, Zone::Graveyard);
    let d = decide(&t, &mut s);
    let cast = offered(&d, |o| matches!(o, Opt::Cast { .. })).unwrap();
    choose(&t, &mut s, cast);
    let d = decide(&t, &mut s);
    assert_eq!(d.kind, DecisionKind::X);
    let xs: Vec<_> = (0..d.len() as u16)
        .filter_map(|c| match d.candidates.option(Choice(c)) {
            Some(Opt::Number(x)) => Some(x),
            _ => None,
        })
        .collect();
    // {X}{W} has mana value 2 or less with X as 0 or 1, though six lands could pay for more.
    assert_eq!(xs, [0, 1]);
}
