//! A scenario's starting position (architecture §17.1): every card starts in its owner's
//! library, in slots numbered as [`Slots`] says, and moves to its zone.

use beryl_engine::rng::SplitMix64;
use beryl_engine::state::{CounterKind, Memo, Tracker};
use beryl_engine::{
    CardId, CardTable, Format, GameTable, Large, LargeCardIx, Layout, Profile, Seat, State, Zone,
};

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

    /// Renumbers the deck slots of a game that has yet to begin.
    fn apply<L: Layout>(self, s: &mut State<L>) {
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
    let t = table(cards, scenario)?;
    let s = State::new(&t, scenario.setup.seed, Profile::EXACT)
        .map_err(|error| setup_failure(scenario, error.to_string()))?;
    let s = arrange(&t, s, scenario, slots)?;
    Ok((t, s))
}

/// Starts in Small if the deal fits and the cards cannot start a subgame. Scenarios can
/// inspect decisions in a subgame by starting in Large.
pub(crate) fn with_layout<T>(
    cards: &'static CardTable,
    scenario: &Scenario,
    slots: Slots,
    small: impl FnOnce(GameTable, State) -> Result<T, Failure>,
    large: impl FnOnce(GameTable, State<Large>) -> Result<T, Failure>,
) -> Result<T, Failure> {
    let t = table(cards, scenario)?;
    let seed = scenario.setup.seed;
    if !has_subgame_card(&t)
        && let Ok(s) = State::new(&t, seed, Profile::EXACT)
    {
        let s = arrange(&t, s, scenario, slots)?;
        small(t, s)
    } else {
        let s = State::<Large>::deal(&t, seed, Profile::EXACT)
            .map_err(|error| setup_failure(scenario, error.to_string()))?;
        let s = arrange(&t, s, scenario, slots)?;
        large(t, s)
    }
}

fn has_subgame_card(table: &GameTable) -> bool {
    table.indices::<LargeCardIx>().any(|card| {
        table.def(card).faces.iter().any(|face| {
            face.abilities
                .iter()
                .any(beryl_engine::card::Ability::starts_subgame)
        })
    })
}

fn setup_failure(scenario: &Scenario, message: String) -> Failure {
    Failure {
        scenario: scenario.name,
        action: None,
        message,
    }
}

fn table(cards: &'static CardTable, scenario: &Scenario) -> Result<GameTable, Failure> {
    let decks = decks(scenario);
    let sideboards = scenario.setup.players.map(|p| {
        p.sideboard
            .iter()
            .map(|&name| (1, name))
            .collect::<Vec<_>>()
    });
    GameTable::with_sideboards(
        Format::CONSTRUCTED,
        cards,
        decks.each_ref().map(Vec::as_slice),
        sideboards.each_ref().map(Vec::as_slice),
    )
    .map_err(|error| setup_failure(scenario, format!("{error:?}")))
}

/// Every card the setup names, by owner: the decks the game is built from.
fn decks(scenario: &Scenario) -> [Vec<(u8, &'static str)>; 2] {
    scenario.setup.players.map(|p| {
        p.hand
            .iter()
            .chain(p.battlefield)
            .chain(p.permanents.iter().filter(|b| !b.token).map(|b| &b.card))
            .chain(p.permanents.iter().flat_map(|b| b.exiled_with))
            .chain(p.graveyard)
            .chain(p.exile)
            .chain(p.library)
            .map(|&name| (1, name))
            .collect()
    })
}

/// The setup's position: every card starts in its owner's library and moves to its zone.
fn arrange<L: Layout>(
    t: &GameTable,
    mut s: State<L>,
    scenario: &Scenario,
    slots: Slots,
) -> Result<State<L>, Failure> {
    let setup = &scenario.setup;
    slots.apply(&mut s);
    s.board().begin_at(setup.turn, setup.active, setup.step);
    if t.can(beryl_engine::table::Capability::TurnStructure) {
        s.plan_from_current_step(t.format())
            .map_err(|_| setup_failure(scenario, "the initial turn plan is full".to_string()))?;
    }
    for (p, player) in Seat::BOTH.into_iter().zip(&setup.players) {
        s.board().set_life(p, player.life);
        let full = |_| setup_failure(scenario, "a zone is full".to_string());
        for &name in player.hand {
            let slot = library_slot(&s, t, p, name);
            s.board().place(slot, Zone::Hand).map_err(full)?;
        }
        for b in player.all_permanents() {
            let slot = if b.token {
                s.board()
                    .place_token(
                        p,
                        token_index(t, b.card)
                            .map_err(|message| setup_failure(scenario, message))?,
                        b.face,
                        &[],
                    )
                    .map_err(full)?
            } else {
                let slot = library_slot(&s, t, p, b.card);
                s.board().place(slot, Zone::Battlefield).map_err(full)?;
                s.board().set_face(slot, b.face);
                slot
            };
            s.board().set_tapped(slot, b.tapped);
            s.board().set_sick(slot, b.sick);
            // A scenario's permanents entered the battlefield the turn it begins in, as its
            // setup has always had them: a permanent whose abilities are faster that turn
            // may use them so, and the scripts recorded from scenarios stop where it may.
            s.track(t, Tracker::EnteredThisTurn, slot);
            // CR 306.5b: a planeswalker "enters with a number of loyalty counters on it equal
            // to its printed loyalty number."
            let printed = s.copiable(slot).turned(t, b.face).face(t).loyalty;
            let counters = b
                .counters
                .map(|c| (c.kind, c.count))
                .or(printed.map(|n| (CounterKind::Loyalty, n)));
            if let Some((kind, n)) = counters {
                s.board().set_counters(slot, kind, n).map_err(full)?;
            }
            for &name in b.exiled_with {
                let card = library_slot(&s, t, p, name);
                s.board().place(card, Zone::Exile).map_err(full)?;
                s.board()
                    .remember(card, Memo::ExiledWith(slot))
                    .map_err(full)?;
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
fn token_index<C: CardId>(t: &GameTable, name: &str) -> Result<C, String> {
    t.index_named(name)
        .ok_or_else(|| format!("no card of the setup creates a {name}"))
}

/// A slot in `player`'s library holding the card `name`. The decks are built from the setup,
/// so there is always one.
fn library_slot<L: Layout>(s: &State<L>, t: &GameTable, player: Seat, name: &str) -> L::Slot {
    s.library(player)
        .iter()
        .copied()
        .find(|&slot| s.name(t, slot) == name)
        .expect("the deck holds every card the setup names")
}
