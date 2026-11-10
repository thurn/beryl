//! Replay files (architecture §14): a game's seed, decklists and every choice, which
//! reproduce it. Since version 2, a choice is recorded by what it does ([`crate::save`]), so a
//! later build replays it though its candidates moved. Since version 3, what chance produced
//! is recorded too, each hidden card as a player first learns it and each coin flip
//! ([`beryl_engine::chance`]), so a replay does not depend on the seed. Since version 4, the
//! board as each turn begins is recorded too ([`crate::checkpoint`]), which a replay checks
//! itself against and rebuilds the game from where it can no longer go on. Loading a file
//! upgrades it to the format this build writes first ([`crate::migrate`]).

use std::path::Path;

use beryl_cards::CARDS;
use beryl_engine::chance::Random;
use beryl_engine::{CardTable, GameTable, Seat, State, TableError};
use serde::{Deserialize, Serialize};

use crate::checkpoint::Checkpoint;
use crate::game::with_stops;
use crate::protocol::StopsSetting;
use crate::save::Chosen;
use crate::{SessionError, StartGame};
use crate::{migrate, perturb};

/// A recorded game: how it was set up, and every entry of its choice log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayFile {
    /// The file's format: 1 recorded each choice as its index alone, 2 by what it does, 3
    /// also what chance produced, and 4 also the board as each turn begins. A file without one
    /// is version 1.
    #[serde(default = "version_1")]
    pub format_version: u8,
    /// The older engine silently answered a required waterbend choice with one answer.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub legacy_waterbend: bool,
    pub seed: u64,
    /// Each seat's decklist: (count, card name).
    pub decks: [Vec<(u8, String)>; 2],
    pub human: u8,
    /// The agent in the other seat.
    pub opponent: String,
    /// The priority stops the game began with.
    pub stops: StopsSetting,
    /// Who won the coin flip and chose to play or draw
    /// ([CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)), since version 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chooser: Option<u8>,
    /// The board the game was rebuilt from, as its turn began, where the record before it
    /// could no longer replay ([`crate::Game::from_replay`]): the log goes on from there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Box<Checkpoint>>,
    pub log: Vec<Entry>,
    /// The commit of the build that last wrote the file, if it said (older files do not).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

/// The format this build writes. Raising it adds a migration ([`crate::migrate`]).
pub const FORMAT_VERSION: u8 = 8;

fn version_1() -> u8 {
    1
}

/// One entry of the choice log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Entry {
    /// The answer to the open decision, whichever seat made it.
    Choice(Chosen),
    /// The human changed their stops while their decision was open.
    Stops(StopsSetting),
    /// The human went on from a halt, which the session, not the engine, asked.
    Halt,
    /// The answer the session gave to the human's decision for them (a priority passed, a
    /// trigger ordered under the smart stops), which it finds again as a replay plays on.
    Auto(Chosen),
    /// The human conceded, at the open decision, theirs or the agent's.
    Concede,
    /// The human took back their latest answer from their open decision
    /// ([`crate::Game::undo`]), including any grouped follow-ups.
    Undo,
    /// The human passed on to their next stop from their open priority
    /// ([`crate::Game::pass_on`]): the pass follows, and the session passes for them after it.
    PassOn,
    /// A card no player knew became known, as the entry before it played out: `card` is the
    /// name of what player `player`'s card was. A replay deals it there ([`Random::Reveal`]).
    Reveal { player: u8, card: String },
    /// Player `player` flipped a coin as the entry before it played out, and won or lost.
    Flip { player: u8, won: bool },
    /// The board as a turn began, after the entry before it.
    Checkpoint(Box<Checkpoint>),
}

impl Entry {
    /// The entry recording `outcome`.
    pub(crate) fn of(t: &GameTable, outcome: Random) -> Entry {
        match outcome {
            Random::Reveal { owner, card } => Entry::Reveal {
                player: owner.get(),
                card: t.def(card).name.to_owned(),
            },
            Random::Flip { player, won } => Entry::Flip {
                player: player.get(),
                won,
            },
        }
    }

    /// The outcome of chance this entry records in the game of `t`, if it records one.
    pub(crate) fn random(&self, t: &GameTable) -> Result<Option<Random>, SessionError> {
        let seat = |player: u8| {
            Seat::try_new(player).ok_or_else(|| SessionError::BadReplay {
                reason: format!("no seat {player}"),
            })
        };
        Ok(Some(match self {
            Entry::Reveal { player, card } => {
                let owner = seat(*player)?;
                let card = t
                    .deck(owner)
                    .iter()
                    .copied()
                    .find(|&c| t.def(c).name == card)
                    .ok_or_else(|| SessionError::BadReplay {
                        reason: format!("player {player}'s deck has no {card} to reveal"),
                    })?;
                Random::Reveal { owner, card }
            }
            &Entry::Flip { player, won } => Random::Flip {
                player: seat(player)?,
                won,
            },
            _ => return Ok(None),
        }))
    }
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
        let state = perturb::new_game(&table, self.seed, profile);
        Ok((table, state))
    }

    /// Reads the file at `path`, upgraded to the format this build writes ([`crate::migrate`]).
    pub fn load(path: &Path) -> Result<ReplayFile, SessionError> {
        let text = std::fs::read_to_string(path).map_err(SessionError::io(path))?;
        let file = serde_json::from_str(&text).map_err(|e| SessionError::BadReplay {
            reason: e.to_string(),
        })?;
        migrate::to_current(file, &CARDS)
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
