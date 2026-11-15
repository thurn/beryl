//! Runs the manual table and publishes each accepted action's view on the game feed.

use std::hash::{BuildHasher, RandomState};
use std::sync::{Mutex, MutexGuard, PoisonError};

use beryl_session::protocol::TableAction;
use beryl_session::{GameUpdate, SessionId, SessionIds};
use beryl_table::deck::Deck;
use beryl_table::{Action, Chance, Log, Refusal, Seat, Table};

use crate::publication::PublishedTable;

const SOLO: Seat = Seat(0);

pub struct TableSessions {
    ids: SessionIds,
    // Serialize actions and their publication, so two callers can't emit views out of order.
    current: Mutex<Option<Solo>>,
    emit: Box<dyn Fn(GameUpdate) + Send + Sync>,
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
            emit: Box::new(emit),
        }
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
        *current = Some(solo);
        (self.emit)(update);
        Ok(session)
    }

    pub fn act(&self, action: TableAction) -> Result<(), Refusal> {
        let mut current = self.lock();
        let solo = current.as_mut().ok_or(Refusal::Stale)?;
        solo.table.apply_client(SOLO, action)?;
        (self.emit)(solo.update());
        Ok(())
    }

    pub fn current(&self) -> Option<GameUpdate> {
        self.lock().as_ref().map(Solo::update)
    }

    pub fn leave(&self) {
        *self.lock() = None;
    }

    fn lock(&self) -> MutexGuard<'_, Option<Solo>> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
