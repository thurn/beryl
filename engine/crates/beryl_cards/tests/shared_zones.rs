//! Cards in a game of the shared-deck format ([`Format::SHARED_DECK`]), whose players share one
//! library and one graveyard. The format's rule: "Anything referring to 'your' graveyard or
//! library is referencing the shared zone". So a card in the shared graveyard is in each
//! player's graveyard: either player may cast it from there or have its ability trigger for
//! them, and an effect on each player's library or graveyard acts on the shared one once.
//!
//! A card in the shared library or graveyard has no owner: the player who takes it owns it from
//! then on, and "its owner" of a card still there is the player who performs the instruction.
//!
//! Each game is built with the kernel's `Board` from a deck of the cards named, all of them in
//! the shared library at first.

use beryl_cards::{CARDS, names};
use beryl_engine::event::{Event, Sink};
use beryl_engine::state::StackEntry;
use beryl_engine::two_player::TwoPlayerSeat;
use beryl_engine::{
    Candidates, Choice, Decision, DecisionKind, Format, GameTable, Opt, Profile, Replay, Scratch,
    Seat, Slot, State, Step, TurnStep, Zone, ZoneKey, apply, next,
};
use beryl_scenario::Live;
use beryl_scenario::prelude::{ME, Resolve, THEM};

struct Game {
    t: GameTable,
    s: State,
    x: Scratch,
}

impl Game {
    /// A shared-deck game of `deck`, in the precombat main phase of `active`'s turn 3, with
    /// every card in the shared library. The game's events are recorded.
    fn new(deck: &[(u8, &'static str)], active: Seat) -> Self {
        let t = GameTable::new(Format::SHARED_DECK, &CARDS, [deck, &[]]).unwrap();
        let mut s = State::new(&t, 0, Profile::EXACT).unwrap();
        assert!(s.shares_zones());
        s.board().begin_at(3, active, TurnStep::PrecombatMain);
        let mut x = Scratch::new();
        x.events = Sink::recording();
        Game { t, s, x }
    }

    /// Moves a card named `name` from the shared library to `zone`: for a hand or the
    /// battlefield, `player`'s.
    fn put(&mut self, name: &str, player: Seat, zone: Zone) -> Slot {
        let slot = (self.s.library(player).iter().copied())
            .find(|&slot| self.s.definition_name(&self.t, slot) == name)
            .unwrap_or_else(|| panic!("{name} in the shared library"));
        if !matches!(zone, Zone::Library | Zone::Graveyard) {
            self.s.claim(slot, player);
        }
        self.s.board().place(slot, zone).unwrap();
        slot
    }

    /// Answers each decision until `done` holds where one is asked: with the option `wanted`
    /// names where it is offered, else by passing priority, else with the first answer.
    fn play(
        &mut self,
        mut wanted: impl FnMut(&State, &Decision) -> Option<Opt>,
        mut done: impl FnMut(&State) -> bool,
    ) -> Decision {
        for _ in 0..500 {
            let Step::Decide(d) = next(&mut self.s, &self.t, &mut self.x, &mut Replay) else {
                panic!("the game ended");
            };
            if done(&self.s) {
                return d;
            }
            let options: &[Opt] = match &d.candidates {
                Candidates::Options(o) => o.as_slice(),
                Candidates::Objects(_) => &[],
            };
            let pick = wanted(&self.s, &d)
                .and_then(|opt| options.iter().position(|&o| o == opt))
                .or_else(|| {
                    (d.kind == DecisionKind::Priority)
                        .then(|| options.iter().position(|&o| o == Opt::Pass))
                        .flatten()
                })
                .unwrap_or(0);
            apply(
                &mut self.s,
                &self.t,
                &mut self.x,
                &mut Replay,
                Choice(u16::try_from(pick).unwrap()),
            );
        }
        panic!("the game never reached the state awaited");
    }

    /// The option to cast the card in `slot`, if the decision offers one.
    fn cast(d: &Decision, slot: Slot) -> Option<Opt> {
        match &d.candidates {
            Candidates::Options(o) => o
                .iter()
                .copied()
                .find(|o| matches!(o, Opt::Cast { slot: s, .. } if *s == slot)),
            Candidates::Objects(_) => None,
        }
    }

    /// The controller of the spell on the stack whose card is in `slot`, if there is one.
    fn spell_controller(s: &State, slot: Slot) -> Option<Seat> {
        (s.stack().iter())
            .find(|entry| entry.spell().is_some_and(|spell| spell.slot == slot))
            .map(StackEntry::controller)
    }
}

/// Either player may cast a card in the shared graveyard that an effect of theirs gave
/// flashback to: Snapcaster Mage's controller casts Lightning Bolt from there, and the spell is
/// theirs.
#[test]
fn either_player_casts_a_card_given_flashback_from_the_shared_graveyard() {
    use names::{ISLAND, LIGHTNING_BOLT, MOUNTAIN, SNAPCASTER_MAGE};
    let deck = [
        (1, SNAPCASTER_MAGE),
        (1, LIGHTNING_BOLT),
        (8, ISLAND),
        (8, MOUNTAIN),
    ];
    for player in Seat::BOTH {
        let mut g = Game::new(&deck, player);
        let snapcaster = g.put(SNAPCASTER_MAGE, player, Zone::Hand);
        let bolt = g.put(LIGHTNING_BOLT, player, Zone::Graveyard);
        for land in [ISLAND, ISLAND, MOUNTAIN] {
            g.put(land, player, Zone::Battlefield);
        }
        assert!(g.s.holds(player.opponent(), bolt));
        g.play(
            |_, d| Game::cast(d, snapcaster).or_else(|| Game::cast(d, bolt)),
            |s| Game::spell_controller(s, bolt).is_some(),
        );
        assert_eq!(Game::spell_controller(&g.s, bolt), Some(player));
        // The caster took the card out of the shared graveyard, and owns it on the stack.
        assert_eq!(g.s.object(bolt).owner(), player);
    }
}

/// Either player may cast a card with escape from the shared graveyard: Underworld Breach's
/// controller casts Lightning Bolt from there, exiling three other cards from it.
#[test]
fn either_player_casts_a_card_with_escape_from_the_shared_graveyard() {
    use names::{ISLAND, LIGHTNING_BOLT, MOUNTAIN, UNDERWORLD_BREACH};
    let deck = [
        (1, UNDERWORLD_BREACH),
        (1, LIGHTNING_BOLT),
        (8, ISLAND),
        (8, MOUNTAIN),
    ];
    for player in Seat::BOTH {
        let mut g = Game::new(&deck, player);
        g.put(UNDERWORLD_BREACH, player, Zone::Battlefield);
        g.put(MOUNTAIN, player, Zone::Battlefield);
        for _ in 0..3 {
            g.put(ISLAND, player, Zone::Graveyard);
        }
        let bolt = g.put(LIGHTNING_BOLT, player, Zone::Graveyard);
        let (mut caster, mut owner) = (None, None);
        g.play(
            |_, d| Game::cast(d, bolt),
            |s| {
                caster = caster.or(Game::spell_controller(s, bolt));
                owner = owner.or(caster.map(|_| s.object(bolt).owner()));
                caster.is_some() && s.stack().is_empty()
            },
        );
        assert_eq!(caster, Some(player));
        // The caster took the card out of the shared graveyard, and owned it on the stack.
        assert_eq!(owner, Some(player));
        let exiled =
            (g.s.in_zone(Zone::Exile)).filter(|slot| g.s.definition_name(&g.t, slot) == ISLAND);
        assert_eq!(exiled.count(), 3);
        // The cards exiled to pay the escape cost are the payer's.
        assert!(exiled.all(|slot| g.s.object(slot).owner() == player));
    }
}

/// Bloodghast in the shared graveyard says "Whenever a land you control enters, you may return
/// this card from your graveyard to the battlefield". It is in each player's graveyard, so a
/// land entering under either player's control triggers it, once, for that player and not for
/// the other.
#[test]
fn a_card_in_the_shared_graveyard_triggers_for_the_player_the_event_names() {
    use names::{BLOODGHAST, MOUNTAIN};
    let deck = [(1, BLOODGHAST), (8, MOUNTAIN)];
    for player in Seat::BOTH {
        let mut g = Game::new(&deck, player);
        let ghast = g.put(BLOODGHAST, player, Zone::Graveyard);
        let land = g.put(MOUNTAIN, player, Zone::Hand);
        let triggered = |s: &State| -> Vec<Seat> {
            let waiting = (s.triggers().iter())
                .filter(|p| p.of.source.is_some_and(|o| o.slot == ghast))
                .map(|p| p.of.controller.seat());
            let stacked = (s.stack().iter())
                .filter(|e| e.source().is_some_and(|o| o.slot == ghast))
                .map(StackEntry::controller);
            waiting.chain(stacked).collect()
        };
        g.play(
            |_, _| Some(Opt::PlayLand(land)),
            |s| !triggered(s).is_empty(),
        );
        assert_eq!(triggered(&g.s), [player]);
    }
}

/// Day's Undoing says "Each player shuffles their hand and graveyard into their library, then
/// draws seven cards." The players share one library, which is shuffled once, and they draw
/// from it one card at a time, starting with the active player: "Whenever multiple players are
/// drawing cards simultaneously, they are dealt one at a time, starting with the active
/// player".
#[test]
fn each_player_shuffles_the_shared_library_once_and_draws_alternately() {
    use names::{BLOODGHAST, DAYS_UNDOING, ISLAND, MOUNTAIN};
    let deck = [
        (1, DAYS_UNDOING),
        (1, BLOODGHAST),
        (20, ISLAND),
        (20, MOUNTAIN),
    ];
    for player in Seat::BOTH {
        let mut g = Game::new(&deck, player);
        let undoing = g.put(DAYS_UNDOING, player, Zone::Hand);
        g.put(BLOODGHAST, player, Zone::Graveyard);
        g.put(MOUNTAIN, player.opponent(), Zone::Hand);
        for _ in 0..3 {
            g.put(ISLAND, player, Zone::Battlefield);
        }
        let mut cast = false;
        g.play(
            |_, d| {
                let opt = Game::cast(d, undoing);
                cast |= opt.is_some();
                opt
            },
            |s| s.object(undoing).zone() != Zone::Hand && s.stack().is_empty(),
        );
        assert!(cast);
        let events = g.x.events.events();
        let shuffles = (events.iter())
            .filter(|e| matches!(e, Event::LibraryShuffled { .. }))
            .count();
        assert_eq!(shuffles, 1, "the shared library is shuffled once");
        let draws: Vec<(Seat, u8)> = (events.iter())
            .filter_map(|e| match *e {
                Event::Draw { player, cards } => Some((player, cards)),
                _ => None,
            })
            .take(14)
            .collect();
        let alternating: Vec<(Seat, u8)> = (0..14)
            .map(|i| {
                (
                    if i % 2 == 0 {
                        player
                    } else {
                        player.opponent()
                    },
                    1,
                )
            })
            .collect();
        assert_eq!(draws, alternating);
    }
}

/// Accumulated Knowledge says "Draw a card, then draw cards equal to the number of cards named
/// Accumulated Knowledge in all graveyards." The shared graveyard is every player's, and each
/// card in it is counted once.
#[test]
fn a_count_of_cards_in_all_graveyards_counts_the_shared_graveyard_once() {
    use names::{ACCUMULATED_KNOWLEDGE, ISLAND};
    let deck = [(3, ACCUMULATED_KNOWLEDGE), (20, ISLAND)];
    for player in Seat::BOTH {
        let mut g = Game::new(&deck, player);
        let knowledge = g.put(ACCUMULATED_KNOWLEDGE, player, Zone::Hand);
        for _ in 0..2 {
            g.put(ACCUMULATED_KNOWLEDGE, player, Zone::Graveyard);
        }
        for _ in 0..2 {
            g.put(ISLAND, player, Zone::Battlefield);
        }
        let hand = |s: &State| {
            s.zone(beryl_engine::ZoneKey::Of(player, Zone::Hand))
                .count()
        };
        g.play(
            |_, d| Game::cast(d, knowledge),
            |s| s.object(knowledge).zone() == Zone::Graveyard,
        );
        assert_eq!(hand(&g.s), 1 + 2, "one card, then one for each other copy");
    }
}

/// Every card the tests put somewhere, and Islands for the library to draw from.
const DECK: &[(u8, &str)] = &[
    (6, names::FOREST),
    (6, names::SWAMP),
    (20, names::ISLAND),
    (1, names::HAUNTED_FENGRAF),
    (1, names::SYLVAN_SCRYING),
    (1, names::CAVERN_OF_SOULS),
    (1, names::MISTER_IMMORTAL),
    (1, names::LURRUS_OF_THE_DREAM_DEN),
    (1, names::TORMODS_CRYPT),
    (1, names::UNSUBSTANTIATE),
    (1, names::REST_IN_PEACE),
    (1, names::MENTAL_NOTE),
    (1, names::MEGRIM),
    (1, names::CHART_A_COURSE),
    (1, names::DANDAN),
];

/// A shared game in `active`'s first main phase, and a way to put its cards where a test wants
/// them before it is played.
struct Position {
    t: GameTable,
    s: State,
}

impl Position {
    fn new(active: Seat) -> Position {
        let t = GameTable::new(Format::SHARED_DECK, &CARDS, [DECK, &[]]).unwrap();
        let mut s = State::deal(&t, 7, Profile::EXACT).unwrap();
        assert!(s.shares_zones());
        let turn = if active == Seat::P0 { 1 } else { 2 };
        s.board().begin_at(turn, active, TurnStep::PrecombatMain);
        Position { t, s }
    }

    /// A card named `name` still in the shared library.
    fn in_library(&self, name: &str) -> Slot {
        let s = &self.s;
        *s.library(Seat::P0)
            .iter()
            .find(|&&slot| s.name(&self.t, slot) == name)
            .unwrap_or_else(|| panic!("{name} in the shared library"))
    }

    /// Puts a card named `name` from the shared library into `zone`: for a zone with owners,
    /// owned by `owner`, and into the shared one for the shared graveyard.
    fn put(&mut self, owner: Seat, name: &str, zone: Zone) -> Slot {
        let slot = self.in_library(name);
        if !matches!(zone, Zone::Graveyard | Zone::Library) {
            self.s.claim(slot, owner);
        }
        self.s.board().place(slot, zone).unwrap();
        slot
    }

    fn lands(&mut self, owner: Seat, name: &str, n: usize) {
        for _ in 0..n {
            self.put(owner, name, Zone::Battlefield);
        }
    }

    fn live(self) -> Live {
        Live::from_position(self.t, self.s)
    }
}

/// The owner of the card in `slot`, which must have one.
fn owner(s: &State, slot: Slot) -> Seat {
    assert!(s.has_owner(slot), "the card has an owner");
    s.object(slot).owner()
}

#[test]
fn a_card_returned_from_the_shared_graveyard_to_a_hand_belongs_to_that_hand() {
    let mut p = Position::new(Seat::P1);
    p.lands(Seat::P1, names::FOREST, 3);
    let fengraf = p.put(Seat::P1, names::HAUNTED_FENGRAF, Zone::Battlefield);
    let dandan = p.put(Seat::P0, names::DANDAN, Zone::Graveyard);
    assert!(!p.s.has_owner(dandan));
    let mut live = p.live();
    live.act(THEM.activates(names::HAUNTED_FENGRAF, 1)).unwrap();
    live.act(Resolve).unwrap();
    let s = live.state();
    assert!(s.zone(ZoneKey::Of(Seat::P1, Zone::Hand)).contains(dandan));
    assert_eq!(owner(s, dandan), Seat::P1);
    // The land sacrificed for the cost went to the shared graveyard, and has no owner there.
    assert_eq!(s.object(fengraf).zone(), Zone::Graveyard);
    assert!(!s.has_owner(fengraf));
}

#[test]
fn a_land_found_in_the_shared_library_asks_its_new_owner_as_it_enters() {
    let mut p = Position::new(Seat::P1);
    p.lands(Seat::P1, names::FOREST, 2);
    p.put(Seat::P1, names::SYLVAN_SCRYING, Zone::Hand);
    let cavern = p.in_library(names::CAVERN_OF_SOULS);
    let mut live = p.live();
    live.act(THEM.casts(names::SYLVAN_SCRYING)).unwrap();
    live.act(Resolve).unwrap();
    live.act(THEM.picks_up_to(names::CAVERN_OF_SOULS)).unwrap();
    assert_eq!(owner(live.state(), cavern), Seat::P1);
    live.act(THEM.plays(names::CAVERN_OF_SOULS)).unwrap();
    // Cavern of Souls asks for a creature type as it enters: its owner and controller answer.
    live.act(THEM.chooses_named("Fish")).unwrap();
    let s = live.state();
    assert_eq!(owner(s, cavern), Seat::P1);
    assert_eq!(s.controller(cavern), Seat::P1);
}

#[test]
fn a_card_put_from_the_shared_graveyard_under_its_owners_control_is_the_performers() {
    let mut p = Position::new(Seat::P1);
    p.lands(Seat::P1, names::FOREST, 3);
    let immortal = p.put(Seat::P0, names::MISTER_IMMORTAL, Zone::Graveyard);
    let mut live = p.live();
    live.act(THEM.activates(names::MISTER_IMMORTAL, 0)).unwrap();
    live.act(Resolve).unwrap();
    live.act(THEM.controls(names::MISTER_IMMORTAL, 1)).unwrap();
    assert_eq!(owner(live.state(), immortal), Seat::P1);
}

#[test]
fn a_spell_cast_from_the_shared_graveyard_belongs_to_its_caster() {
    let mut p = Position::new(Seat::P1);
    p.lands(Seat::P1, names::FOREST, 2);
    p.put(Seat::P1, names::LURRUS_OF_THE_DREAM_DEN, Zone::Battlefield);
    let immortal = p.put(Seat::P0, names::MISTER_IMMORTAL, Zone::Graveyard);
    let mut live = p.live();
    live.act(THEM.casts(names::MISTER_IMMORTAL)).unwrap();
    assert_eq!(live.state().object(immortal).zone(), Zone::Stack);
    assert_eq!(owner(live.state(), immortal), Seat::P1);
    live.act(Resolve).unwrap();
    live.act(THEM.controls(names::MISTER_IMMORTAL, 1)).unwrap();
    assert_eq!(owner(live.state(), immortal), Seat::P1);
}

#[test]
fn a_card_exiled_from_the_shared_graveyard_returns_to_its_exilers_hand() {
    let mut p = Position::new(Seat::P1);
    p.lands(Seat::P1, names::FOREST, 3);
    p.put(Seat::P1, names::TORMODS_CRYPT, Zone::Battlefield);
    let immortal = p.put(Seat::P0, names::MISTER_IMMORTAL, Zone::Graveyard);
    p.lands(Seat::P0, names::ISLAND, 2);
    p.put(Seat::P0, names::UNSUBSTANTIATE, Zone::Hand);
    let mut live = p.live();
    // Tormod's Crypt exiles the shared graveyard for the player who sacrificed it.
    live.act(THEM.activates(names::TORMODS_CRYPT, 0)).unwrap();
    live.act(THEM.targets_player(THEM)).unwrap();
    live.act(Resolve).unwrap();
    assert_eq!(live.state().object(immortal).zone(), Zone::Exile);
    assert_eq!(owner(live.state(), immortal), Seat::P1);
    live.act(THEM.activates(names::MISTER_IMMORTAL, 0)).unwrap();
    live.act(Resolve).unwrap();
    // P0 answers the pass with Unsubstantiate, whose one target is Mister Immortal: no player
    // could do anything but pass, so it resolves, and returns Mister Immortal to its owner's
    // hand, the hand of the player who exiled it.
    live.act(THEM.passes()).unwrap();
    live.act(ME.casts(names::UNSUBSTANTIATE)).unwrap();
    live.act(THEM.has(Zone::Hand, names::MISTER_IMMORTAL, 1))
        .unwrap();
    let s = live.state();
    assert!(s.zone(ZoneKey::Of(Seat::P1, Zone::Hand)).contains(immortal));
}

#[test]
fn a_milled_card_that_rest_in_peace_exiles_belongs_to_the_player_who_milled() {
    let mut p = Position::new(Seat::P1);
    p.put(Seat::P0, names::REST_IN_PEACE, Zone::Battlefield);
    p.lands(Seat::P1, names::ISLAND, 2);
    p.put(Seat::P1, names::MENTAL_NOTE, Zone::Hand);
    let top: Vec<Slot> =
        p.s.library(Seat::P0)
            .iter()
            .rev()
            .take(2)
            .copied()
            .collect();
    let mut live = p.live();
    live.act(THEM.casts(names::MENTAL_NOTE)).unwrap();
    live.act(Resolve).unwrap();
    let s = live.state();
    for slot in top {
        assert_eq!(s.object(slot).zone(), Zone::Exile);
        assert_eq!(owner(s, slot), Seat::P1);
    }
}

#[test]
fn a_discard_by_seat_one_triggers_whenever_an_opponent_discards() {
    let mut p = Position::new(Seat::P1);
    p.put(Seat::P0, names::MEGRIM, Zone::Battlefield);
    p.lands(Seat::P1, names::ISLAND, 2);
    p.put(Seat::P1, names::CHART_A_COURSE, Zone::Hand);
    p.put(Seat::P1, names::DANDAN, Zone::Hand);
    let mut live = p.live();
    live.act(THEM.casts(names::CHART_A_COURSE)).unwrap();
    live.act(Resolve).unwrap();
    live.act(THEM.picks(names::DANDAN)).unwrap();
    live.act(Resolve).unwrap();
    live.act(THEM.life(18)).unwrap();
    live.act(ME.life(20)).unwrap();
}

#[test]
fn a_sacrificed_dandan_has_no_owner_in_the_shared_graveyard() {
    let mut p = Position::new(Seat::P0);
    let dandan = p.put(Seat::P0, names::DANDAN, Zone::Battlefield);
    let mut live = p.live();
    live.act(Resolve).unwrap();
    let s = live.state();
    assert_eq!(s.object(dandan).zone(), Zone::Graveyard);
    assert!(!s.has_owner(dandan));
}

#[test]
fn a_seeded_forgetful_fish_game_between_random_agents_reaches_an_outcome() {
    let t = GameTable::new(
        Format::SHARED_DECK,
        &CARDS,
        [beryl_cards::decks::FORGETFUL_FISH, &[]],
    )
    .unwrap();
    let mut s = State::new(&t, 0, Profile::EXACT).unwrap();
    let mut x = Scratch::new();
    let mut rng: u64 = 0x5EED_F154_D4AD_A401;
    let mut next_u64 = || {
        rng = rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut reached = false;
    for _ in 0..50_000 {
        match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Over(_) => {
                reached = true;
                break;
            }
            Step::Decide(d) => {
                let count = d.candidates.len();
                let pick = usize::try_from(next_u64() % u64::try_from(count).unwrap()).unwrap();
                apply(
                    &mut s,
                    &t,
                    &mut x,
                    &mut Replay,
                    Choice(u16::try_from(pick).unwrap()),
                );
            }
        }
    }
    assert!(
        reached,
        "seeded Forgetful Fish game did not reach an outcome"
    );
}

fn shared_controller_game(
    active: Seat,
) -> (Game, &'static str, &'static str, &'static str, &'static str) {
    use beryl_card::prelude::{
        Ability, CardDef, Face, Filter, Objects, Prompt, Purpose, TargetKind, TargetSpec, Types,
        Zone as CardZone, choose, controller_of, each_player, exile, for_each_player, gain_life,
        its_controller, lose_life, sequence, target, that_player, those,
    };

    const A_GRAVEYARD_CARD: TargetSpec = TargetSpec::one(TargetKind::GraveyardCard, Filter::Any);
    static FILLER: CardDef = CardDef {
        name: "Test Filler",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            types: Types::LAND,
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static READ_CONTROLLER: CardDef = CardDef {
        name: "Test Read Controller",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            types: Types::SORCERY,
            abilities: &[Ability::spell()
                .targeting(&[A_GRAVEYARD_CARD])
                .effect(&gain_life(its_controller(), 3))],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static MOVE_AND_READ: CardDef = CardDef {
        name: "Test Move And Read",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            types: Types::SORCERY,
            abilities: &[Ability::spell()
                .targeting(&[A_GRAVEYARD_CARD])
                .effect(&sequence(&[
                    exile(target()),
                    lose_life(controller_of(those()), 1),
                    gain_life(its_controller(), 4),
                ]))],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static EACH_EXILES: CardDef = CardDef {
        name: "Test Each Exiles",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            types: Types::SORCERY,
            abilities: &[Ability::spell().effect(&for_each_player(
                each_player(),
                &sequence(&[
                    choose(
                        that_player(),
                        Objects::All {
                            zone: CardZone::Graveyard,
                            filter: Filter::Any,
                        },
                        Prompt::new(Purpose::Choose),
                    ),
                    exile(those()),
                ]),
            ))],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static CATALOG: beryl_engine::CardTable =
        beryl_engine::CardTable::new(&[&FILLER, &READ_CONTROLLER, &MOVE_AND_READ, &EACH_EXILES]);
    let deck = [
        (1, READ_CONTROLLER.name),
        (1, MOVE_AND_READ.name),
        (1, EACH_EXILES.name),
        (12, FILLER.name),
    ];
    let t = GameTable::new(Format::SHARED_DECK, &CATALOG, [&deck, &[]]).unwrap();
    let mut s = State::new(&t, 0, Profile::EXACT).unwrap();
    s.board().begin_at(3, active, TurnStep::PrecombatMain);
    let g = Game {
        t,
        s,
        x: Scratch::new(),
    };
    (
        g,
        FILLER.name,
        READ_CONTROLLER.name,
        MOVE_AND_READ.name,
        EACH_EXILES.name,
    )
}

#[test]
fn controller_of_a_card_in_or_moved_from_a_shared_zone_is_the_performing_player() {
    // 1. Players::ControllerOf(Target(0)) on a card still in the shared graveyard resolves to
    //    Seat::P1 (the casting player), not the Seat::P0 placeholder.
    {
        let (mut g, filler, read_controller, _, _) = shared_controller_game(Seat::P1);
        let spell = g.put(read_controller, Seat::P1, Zone::Hand);
        let card = g.put(filler, Seat::P0, Zone::Graveyard);
        assert!(!g.s.has_owner(card));
        g.play(
            |_, d| Game::cast(d, spell),
            |s| s.object(spell).zone() == Zone::Graveyard && s.stack().is_empty(),
        );
        assert_eq!(g.s.player(Seat::P1).life(), 23);
        assert_eq!(g.s.player(Seat::P0).life(), 20);
    }

    // 2. After Seat::P1 exiles a card from the shared graveyard, both Players::ControllerOf
    //    on the moved target (Scene::known) and on Objects::Those (Run::remember) resolve to
    //    Seat::P1, not Seat::P0.
    {
        let (mut g, filler, _, move_and_read, _) = shared_controller_game(Seat::P1);
        let spell = g.put(move_and_read, Seat::P1, Zone::Hand);
        let card = g.put(filler, Seat::P0, Zone::Graveyard);
        assert!(!g.s.has_owner(card));
        g.play(
            |_, d| Game::cast(d, spell),
            |s| s.object(spell).zone() == Zone::Graveyard && s.stack().is_empty(),
        );
        assert_eq!(g.s.object(card).zone(), Zone::Exile);
        assert_eq!(owner(&g.s, card), Seat::P1);
        assert_eq!(g.s.player(Seat::P1).life(), 23);
        assert_eq!(g.s.player(Seat::P0).life(), 20);
    }

    // 3. Inside Effect::ForEachPlayer, each player who exiles a card from the shared graveyard
    //    becomes that card's owner, not the spell's controller.
    {
        let (mut g, filler, _, _, each_exiles) = shared_controller_game(Seat::P0);
        let spell = g.put(each_exiles, Seat::P0, Zone::Hand);
        let first = g.put(filler, Seat::P0, Zone::Graveyard);
        let second = g.put(filler, Seat::P0, Zone::Graveyard);
        g.play(
            |_, d| Game::cast(d, spell),
            |s| s.object(spell).zone() == Zone::Graveyard && s.stack().is_empty(),
        );
        let mut owners = [owner(&g.s, first), owner(&g.s, second)];
        owners.sort_by_key(|p| p.index());
        assert_eq!(owners, [Seat::P0, Seat::P1]);
    }
}

#[test]
fn defined_pt_in_a_shared_zone_does_not_read_seat_zero() {
    use names::{ADELINE_RESPLENDENT_CATHAR, DANDAN, ISLAND, TARMOGOYF};
    let deck = [
        (2, ADELINE_RESPLENDENT_CATHAR),
        (1, TARMOGOYF),
        (2, DANDAN),
        (8, ISLAND),
    ];
    let mut g = Game::new(&deck, Seat::P0);
    g.put(DANDAN, Seat::P0, Zone::Battlefield);
    g.put(DANDAN, Seat::P0, Zone::Battlefield);
    let adeline_gy = g.put(ADELINE_RESPLENDENT_CATHAR, Seat::P0, Zone::Graveyard);
    let adeline_lib =
        g.s.library(Seat::P0)
            .iter()
            .copied()
            .find(|&slot| g.s.definition_name(&g.t, slot) == ADELINE_RESPLENDENT_CATHAR)
            .unwrap();
    assert!(!g.s.has_owner(adeline_gy));
    assert!(!g.s.has_owner(adeline_lib));
    // Adeline's power is the number of creatures "you" control; with no owner in a shared zone,
    // she does not count Seat::P0's creatures.
    assert_eq!(g.s.power(&g.t, adeline_gy), 0);
    assert_eq!(g.s.power(&g.t, adeline_lib), 0);
    // Tarmogoyf's ability counts card types across graveyards, which does not depend on "you".
    let goyf = g.put(TARMOGOYF, Seat::P0, Zone::Graveyard);
    g.put(ISLAND, Seat::P0, Zone::Graveyard);
    assert_eq!(g.s.power(&g.t, goyf), 2);
    assert_eq!(g.s.toughness(&g.t, goyf), 3);
}
