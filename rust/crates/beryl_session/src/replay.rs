//! Replay files (architecture §14): a game's seed, decklists and every choice, which
//! reproduce it bit for bit.

use std::path::Path;

use beryl_engine::{CardTable, GameTable, Seat, State, TableError};
use serde::{Deserialize, Serialize};

use crate::game::with_stops;
use crate::protocol::StopsSetting;
use crate::{SessionError, StartGame};

/// A recorded game: how it was set up, and every entry of its choice log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayFile {
    pub seed: u64,
    /// Each seat's decklist: (count, card name).
    pub decks: [Vec<(u8, String)>; 2],
    pub human: u8,
    /// The agent in the other seat.
    pub opponent: String,
    /// The priority stops the game began with.
    pub stops: StopsSetting,
    pub log: Vec<Entry>,
}

/// One entry of the choice log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Entry {
    /// The answer to the open decision, whichever seat made it.
    Choice(u16),
    /// The human changed their stops while their decision was open.
    Stops(StopsSetting),
    /// The human went on from a halt, which the session, not the engine, asked.
    Halt,
    /// The answer the session gave to the human's decision for them (a priority passed, a
    /// trigger ordered under the smart stops), which it finds again as a replay plays on.
    Auto(u16),
    /// The human conceded, at the open decision, theirs or the agent's.
    Concede,
}

impl ReplayFile {
    /// How the recorded game was started.
    pub fn start(&self) -> Result<StartGame, SessionError> {
        let human = Seat::try_new(self.human).ok_or_else(|| SessionError::BadReplay {
            reason: format!("no seat {}", self.human),
        })?;
        Ok(StartGame {
            seed: self.seed,
            decks: self.decks.clone(),
            human,
            opponent: self.opponent.clone(),
            stops: self.stops,
        })
    }

    /// The game's table and its state before the first decision.
    pub(crate) fn setup(
        &self,
        cards: &'static CardTable,
    ) -> Result<(GameTable, State), SessionError> {
        let decks = self.decks.each_ref().map(|deck| {
            deck.iter()
                .map(|(count, name)| {
                    cards
                        .get(name)
                        .map(|def| (*count, def.name))
                        .ok_or_else(|| SessionError::UnknownCard { name: name.clone() })
                })
                .collect::<Result<Vec<_>, _>>()
        });
        let [a, b] = decks;
        let decks = [a?, b?];
        let table = GameTable::new(cards, [decks[0].as_slice(), decks[1].as_slice()]).map_err(
            |e| match e {
                TableError::UnknownCard(name) => SessionError::UnknownCard {
                    name: name.to_owned(),
                },
                e => SessionError::Deck {
                    reason: format!("{e:?}"),
                },
            },
        )?;
        let profile = with_stops(
            beryl_engine::Profile::SMART,
            self.stops,
            self.start()?.human,
        );
        let state = State::new(&table, self.seed, profile);
        Ok((table, state))
    }

    pub fn load(path: &Path) -> Result<ReplayFile, SessionError> {
        let text = std::fs::read_to_string(path).map_err(SessionError::io(path))?;
        serde_json::from_str(&text).map_err(|e| SessionError::BadReplay {
            reason: e.to_string(),
        })
    }

    /// Writes the file whole: to a sibling first, then renamed over `path`, so a reader never
    /// sees half of it.
    pub fn save(&self, path: &Path) -> Result<(), SessionError> {
        let json = serde_json::to_string(self).expect("a replay serializes");
        let partial = path.with_extension("partial");
        std::fs::write(&partial, json).map_err(SessionError::io(&partial))?;
        std::fs::rename(&partial, path).map_err(SessionError::io(path))
    }
}
