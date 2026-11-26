//! Cards in a game of the shared-deck format ([`Format::SHARED_DECK`]), whose players share one
//! library and one graveyard. The format's rule: "Anything referring to 'your' graveyard or
//! library is referencing the shared zone". So a card in the shared graveyard is in each
//! player's graveyard: either player may cast it from there or have its ability trigger for
//! them, and an effect on each player's library or graveyard acts on the shared one once.
//!
//! Each game is built with the kernel's `Board` from a deck of the cards named, all of them in
//! the shared library at first.

use beryl_cards::{CARDS, names};
use beryl_engine::event::{Event, Sink};
use beryl_engine::state::StackEntry;
use beryl_engine::two_player::TwoPlayerSeat;
use beryl_engine::{
    Candidates, Choice, Decision, DecisionKind, Format, GameTable, Opt, Profile, Replay, Scratch,
    Seat, Slot, State, Step, TurnStep, Zone, apply, next,
};

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
        let mut caster = None;
        g.play(
            |_, d| Game::cast(d, bolt),
            |s| {
                caster = caster.or(Game::spell_controller(s, bolt));
                caster.is_some() && s.stack().is_empty()
            },
        );
        assert_eq!(caster, Some(player));
        let exiled = (g.s.in_zone(Zone::Exile))
            .filter(|slot| g.s.definition_name(&g.t, slot) == ISLAND)
            .count();
        assert_eq!(exiled, 3);
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
