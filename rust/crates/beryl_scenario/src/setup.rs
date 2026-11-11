//! A scenario's starting position (architecture §17.1): every card starts in its owner's
//! library, in slots numbered as [`Slots`] says, and moves to its zone.

use beryl_engine::rng::SplitMix64;
use beryl_engine::state::CounterKind;
use beryl_engine::{CardIx, CardTable, GameTable, Profile, Seat, Slot, State, Zone};

use crate::{Failure, Scenario};

/// How a scenario's setup numbers each deck's slots. Slot numbers are public handles that
/// say nothing about a card (architecture §13), so a scenario must play out the same under
/// every numbering: a rule that reads one is an engine bug (docs/bugs.md has several).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Slots {
    /// As [`State::new`] deals them.
    #[default]
    Dealt,
    /// Last first.
    Reversed,
    /// Shuffled, by a fixed seed.
    Shuffled,
}

impl Slots {
    pub const ALL: [Slots; 3] = [Slots::Dealt, Slots::Reversed, Slots::Shuffled];

    /// Renumbers the slots of `s`, a game not yet begun ([`State::renumber_deck_slots`]).
    fn apply(self, s: &mut State) {
        match self {
            Slots::Dealt => {}
            Slots::Reversed => s.board().reverse_deck_slots(),
            Slots::Shuffled => {
                let mut rng = SplitMix64::new(0x510f_5eed);
                s.board().renumber_deck_slots(|slots| rng.shuffle(slots));
            }
        }
    }
}

/// A scenario's starting position and the table it is played on, for tests that drive the
/// engine themselves from a hand-built position, such as the agents'. Its actions are not
/// run.
pub fn position(
    cards: &'static CardTable,
    scenario: &Scenario,
) -> Result<(GameTable, State), Failure> {
    setup(cards, scenario, Slots::Dealt)
}

/// A scenario's starting position as [`position`] gives it, with its slots numbered as
/// `slots` says.
pub(crate) fn setup(
    cards: &'static CardTable,
    scenario: &Scenario,
    slots: Slots,
) -> Result<(GameTable, State), Failure> {
    let fail = |message: String| Failure {
        scenario: scenario.name,
        action: None,
        message,
    };
    let decks = decks(scenario);
    let t = GameTable::new(cards, [decks[0].as_slice(), decks[1].as_slice()])
        .map_err(|e| fail(format!("{e:?}")))?;
    let s = arrange(&t, scenario, slots).map_err(fail)?;
    Ok((t, s))
}

/// Every card the setup names, by owner: the decks the game is built from.
fn decks(scenario: &Scenario) -> [Vec<(u8, &'static str)>; 2] {
    scenario.setup.players.map(|p| {
        p.hand
            .iter()
            .chain(p.battlefield.iter().filter(|b| !b.token).map(|b| &b.card))
            .chain(p.battlefield.iter().flat_map(|b| b.exiled_with))
            .chain(p.graveyard)
            .chain(p.exile)
            .chain(p.library)
            .map(|&name| (1, name))
            .collect()
    })
}

/// The setup's position: every card starts in its owner's library and moves to its zone.
fn arrange(t: &GameTable, scenario: &Scenario, slots: Slots) -> Result<State, String> {
    let setup = &scenario.setup;
    let mut s = State::new(t, setup.seed, Profile::EXACT).map_err(|e| e.to_string())?;
    slots.apply(&mut s);
    s.board().begin_at(setup.turn, setup.active, setup.step);
    for (p, player) in Seat::BOTH.into_iter().zip(&setup.players) {
        s.board().set_life(p, player.life);
        let full = |_| "a zone is full".to_string();
        for &name in player.hand {
            let slot = library_slot(&s, t, p, name);
            s.board().place(slot, Zone::Hand).map_err(full)?;
        }
        for b in player.battlefield {
            let slot = if b.token {
                s.board()
                    .place_token(p, token_index(t, b.card)?, b.face, false)
                    .map_err(full)?
            } else {
                let slot = library_slot(&s, t, p, b.card);
                s.board().place(slot, Zone::Battlefield).map_err(full)?;
                s.board().set_face(slot, b.face);
                slot
            };
            s.board().set_tapped(slot, b.tapped);
            s.board().set_sick(slot, b.sick);
            // CR 306.5b: a planeswalker "enters with a number of loyalty counters on it equal
            // to its printed loyalty number."
            let printed = s.copiable(slot).turned(b.face).face(t).loyalty;
            let counters = b
                .counters
                .map(|c| (c.kind, c.count))
                .or(printed.map(|n| (CounterKind::Loyalty, n)));
            if let Some((kind, n)) = counters {
                s.board().set_counters(slot, kind, n);
            }
            for &name in b.exiled_with {
                let card = library_slot(&s, t, p, name);
                s.board().place(card, Zone::Exile).map_err(full)?;
                s.board().set_exiled_with(card, slot);
            }
        }
        // Top first: the last placed is on top.
        for &name in player.graveyard.iter().rev() {
            let slot = library_slot(&s, t, p, name);
            s.board().place(slot, Zone::Graveyard).map_err(full)?;
        }
        for &name in player.exile {
            let slot = library_slot(&s, t, p, name);
            s.board().place(slot, Zone::Exile).map_err(full)?;
        }
        let mut order = Vec::new();
        for &name in player.library {
            let slot = s
                .library(p)
                .iter()
                .copied()
                .find(|&slot| s.name(t, slot) == name && !order.contains(&slot))
                .expect("the library holds the cards it lists");
            order.push(slot);
        }
        s.board().order_library(p, &order);
    }
    Ok(s)
}

/// The game index of the token named `name`: one a card of the decks creates.
fn token_index(t: &GameTable, name: &str) -> Result<CardIx, String> {
    (0..t.len())
        .map(|i| CardIx::new(u8::try_from(i).expect("game indices fit u8")))
        .find(|&card| t.def(card).name == name)
        .ok_or_else(|| format!("no card of the setup creates a {name}"))
}

/// A slot in `player`'s library holding the card `name`. The decks are built from the setup,
/// so there is always one.
fn library_slot(s: &State, t: &GameTable, player: Seat, name: &str) -> Slot {
    s.library(player)
        .iter()
        .copied()
        .find(|&slot| s.name(t, slot) == name)
        .expect("the deck holds every card the setup names")
}
