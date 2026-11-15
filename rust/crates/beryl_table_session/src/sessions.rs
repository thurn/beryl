//! Runs the manual table and publishes each accepted action's view on the game feed.

use std::hash::{BuildHasher, RandomState};
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc::Receiver};

use beryl_link::guest::Guest;
use beryl_link::host::Sender;
use beryl_link::{Link, LinkError};
use beryl_session::protocol::{LinkState, TableAction};
use beryl_session::{GameUpdate, SessionId, SessionIds};
use beryl_table::deck::Deck;
use beryl_table::save::SaveError;
use beryl_table::{Action, Chance, Log, Refusal, Seat, Table};
use serde_json::Value;

use crate::guest::GuestSession;
use crate::host::HostSession;
use crate::publication::PublishedTable;
use crate::saves::{self, HostSettings, Save, SavedTable};

const SOLO: Seat = Seat(0);

pub(super) type Emit = Arc<dyn Fn(GameUpdate) + Send + Sync>;
pub(super) type EmitEvent = Arc<dyn Fn(TableEvent) + Send + Sync>;

/// Events outside the game feed for a linked table.
#[derive(Debug, PartialEq)]
pub enum TableEvent {
    Link(LinkState),
    Error(String),
    Lobby(Value),
}

pub struct TableSessions {
    ids: SessionIds,
    // Serialize replacements with solo actions and stop the old table before publishing the next.
    current: Mutex<Option<Current>>,
    emit: Emit,
    emit_event: EmitEvent,
    directory: Option<PathBuf>,
}

enum Current {
    Solo(Box<Solo>),
    Hosted(HostSession),
    Joined(GuestSession),
}

enum Departure {
    Host(Sender),
    Guest(Guest),
}

impl Current {
    fn stop(&self) -> Option<Departure> {
        match self {
            Self::Solo(_) => None,
            Self::Hosted(host) => host.stop().map(Departure::Host),
            Self::Joined(guest) => guest.stop().map(Departure::Guest),
        }
    }
}

struct Solo {
    table: PublishedTable,
    session: SessionId,
}

impl Solo {
    fn update(&self) -> GameUpdate {
        GameUpdate {
            session: self.session,
            view: self
                .table
                .publication(SOLO)
                .expect("solo seat")
                .view()
                .clone(),
        }
    }
}

impl TableSessions {
    /// Use the engine's `SessionIds` if both sessions send to the same stage.
    /// The emitter sends updates in order and must not call back into this session.
    pub fn new(ids: SessionIds, emit: impl Fn(GameUpdate) + Send + Sync + 'static) -> Self {
        Self {
            ids,
            current: Mutex::default(),
            emit: Arc::new(emit),
            emit_event: Arc::new(|_| {}),
            directory: None,
        }
    }

    /// Use a caller-supplied data directory for hosted saves.
    pub fn with_data_dir(
        ids: SessionIds,
        data_dir: impl Into<PathBuf>,
        emit: impl Fn(GameUpdate) + Send + Sync + 'static,
    ) -> Self {
        Self {
            directory: Some(data_dir.into().join("tables")),
            ..Self::new(ids, emit)
        }
    }

    /// Set the emitter for link and lobby notifications. Like the game
    /// emitter, it must not call back into the session.
    #[must_use]
    pub fn with_events(mut self, emit: impl Fn(TableEvent) + Send + Sync + 'static) -> Self {
        self.emit_event = Arc::new(emit);
        self
    }

    /// Start a table with one seat, replacing any table after its new view is ready.
    pub fn solo(&self, deck: Deck) -> Result<SessionId, Refusal> {
        self.solo_seeded(deck, RandomState::new().hash_one(0_u8))
    }

    /// Start a solo table with a repeatable shuffle for a recorded run.
    pub fn solo_seeded(&self, deck: Deck, seed: u64) -> Result<SessionId, Refusal> {
        let mut current = self.lock();
        let table = Table::new(1);
        let log = Log::new(&table);
        let mut table = PublishedTable::new(table, log, Chance::fresh(seed))?;
        table.apply_host(
            SOLO,
            &Action::LoadDeck {
                seat: SOLO,
                main: deck.main,
                side: deck.side,
            },
        )?;
        table.apply_host(SOLO, &Action::Start { first: SOLO })?;
        let session = self.ids.allocate();
        let solo = Solo { table, session };
        let update = solo.update();
        let previous = replace(&mut current, Current::Solo(Box::new(solo)));
        (self.emit)(update);
        drop(current);
        leave_link(previous);
        Ok(session)
    }

    /// Start a two-seat table. The transport supplies each accepted link,
    /// including reconnects, through the receiver. Use a bounded channel to
    /// avoid queuing an unbounded number of incoming links.
    pub fn host(
        &self,
        links: Receiver<impl Link + 'static>,
        settings: HostSettings,
    ) -> Result<SessionId, SaveError> {
        let directory = self.directory()?;
        let mut table = Table::new(2);
        table.game_match.starting_life = settings.starting_life;
        table.game_match.starting_hand_size = settings.starting_hand_size;
        for player in &mut table.players {
            player.life = settings.starting_life;
        }
        let seed = RandomState::new().hash_one(0_u8);
        let log = Log::new(&table);
        let save = Save::create(directory, settings, &table, seed)?;
        self.open_host(self.lock(), links, save, table, log, Chance::fresh(seed))
    }

    /// Start a guest session. Its first view arrives after the handshake.
    /// The connector is reused to redial after an interrupted connection.
    pub fn join<L: Link + 'static>(
        &self,
        connect: impl FnMut() -> Result<L, LinkError> + Send + 'static,
        name: impl Into<String>,
        secret: impl Into<String>,
    ) -> SessionId {
        let mut current = self.lock();
        let session = self.ids.allocate();
        let (guest, events) = GuestSession::new(
            session,
            connect,
            name.into(),
            secret.into(),
            Arc::clone(&self.emit),
            Arc::clone(&self.emit_event),
        );
        let previous = replace(&mut current, Current::Joined(guest));
        if let Some(Current::Joined(guest)) = current.as_ref() {
            guest.listen(events);
        }
        drop(current);
        leave_link(previous);
        session
    }

    /// Replay a save and listen for the original guest on the supplied links.
    pub fn resume(
        &self,
        id: &str,
        links: Receiver<impl Link + 'static>,
    ) -> Result<SessionId, SaveError> {
        let current = self.lock();
        if matches!(current.as_ref(), Some(Current::Hosted(host)) if host.is_table(id)) {
            return Err(
                io::Error::new(io::ErrorKind::AlreadyExists, "The table is already open.").into(),
            );
        }
        let (save, table, log, chance) = Save::open(self.directory()?, id)?;
        self.open_host(current, links, save, table, log, chance)
    }

    pub fn list_saved(&self) -> Result<Vec<SavedTable>, SaveError> {
        saves::list(self.directory()?)
    }

    /// Read the transport invite so the caller can rebuild the listener before resuming.
    pub fn saved_invite(&self, id: &str) -> Result<String, SaveError> {
        saves::invite(self.directory()?, id)
    }

    fn directory(&self) -> Result<&std::path::Path, SaveError> {
        self.directory
            .as_deref()
            .ok_or_else(|| io::Error::other("No table data directory was supplied.").into())
    }

    fn open_host(
        &self,
        mut current: MutexGuard<'_, Option<Current>>,
        links: Receiver<impl Link + 'static>,
        save: Save,
        table: Table,
        log: Log,
        chance: Chance,
    ) -> Result<SessionId, SaveError> {
        let invalid = |_: Refusal| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Could not prepare the hosted table.",
            )
        };
        if table.players.len() != 2 {
            return Err(invalid(Refusal::INVALID).into());
        }
        let table = PublishedTable::new(table, log, chance).map_err(invalid)?;
        let session = self.ids.allocate();
        let host = HostSession::new(
            session,
            table,
            save,
            Arc::clone(&self.emit),
            Arc::clone(&self.emit_event),
        )
        .map_err(invalid)?;
        let update = host.current();
        let previous = replace(&mut current, Current::Hosted(host));
        (self.emit)(update);
        if let Some(Current::Hosted(host)) = current.as_ref() {
            host.listen(links);
        }
        drop(current);
        leave_link(previous);
        Ok(session)
    }

    pub fn act(&self, action: TableAction) -> Result<(), Refusal> {
        let mut current = self.lock();
        match current.as_mut().ok_or(Refusal::Stale)? {
            Current::Solo(solo) => {
                solo.table.apply_client(SOLO, action)?;
                (self.emit)(solo.update());
                Ok(())
            }
            Current::Hosted(host) => {
                let apply = host.local_action();
                drop(current);
                apply(action)
            }
            Current::Joined(guest) => {
                let apply = guest.local_action();
                drop(current);
                apply(action)
            }
        }
    }

    pub fn current(&self) -> Option<GameUpdate> {
        self.lock().as_ref().and_then(|current| match current {
            Current::Solo(solo) => Some(solo.update()),
            Current::Hosted(host) => Some(host.current()),
            Current::Joined(guest) => guest.current(),
        })
    }

    pub fn leave(&self) {
        let sender = {
            let mut current = self.lock();
            current.take().and_then(|old| old.stop())
        };
        leave_link(sender);
    }

    fn lock(&self) -> MutexGuard<'_, Option<Current>> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn replace(current: &mut Option<Current>, next: Current) -> Option<Departure> {
    current.replace(next).and_then(|old| old.stop())
}

fn leave_link(departure: Option<Departure>) {
    match departure {
        Some(Departure::Host(sender)) => {
            let _ = sender.leave();
        }
        Some(Departure::Guest(guest)) => drop(guest),
        None => {}
    }
}
