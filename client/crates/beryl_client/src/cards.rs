//! The card records the session thread gives the client (`Input::Cards`), from which the client
//! lists the faces it can draw. A card whose name has no listed face is drawn as the card back.
//!
//! A game against an agent gives the records of both decklists and sideboards when it starts or
//! resumes. A manual table has no decklist the session knows ahead of time: a guest never sees
//! the host's deck, and a player can search the whole card store for a card or a token to put
//! onto the table. So a manual table gives the records of every deck the player loads, before
//! the table's first view, and adds the names each later view shows that were not yet given.
//!
//! Each `Input::Cards` replaces the client's face list, and the host then prepares every face of
//! the new list again, so the feed sends one only when a name is new.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use app_client::Input;
use beryl_host::Inbox;
use beryl_protocol::protocol::PlayerView;
use beryl_session::card_records::{CardRecords, CardRecordsRequest, card_records};

/// The records last given to the client, shared by the session thread and the manual table's
/// view callback, which runs on the table's own threads.
#[derive(Clone)]
pub struct CardFeed {
    inbox: Inbox,
    given: Arc<Mutex<Given>>,
}

/// What the client's face list was last made from.
#[derive(Default, PartialEq)]
enum Given {
    #[default]
    Nothing,
    /// A game's decklists, main decks then sideboards.
    Decks(Vec<Vec<(u8, String)>>),
    /// A manual table's card and token names.
    Names(BTreeSet<String>),
}

impl CardFeed {
    #[must_use]
    pub fn new(inbox: Inbox) -> Self {
        Self {
            inbox,
            given: Arc::default(),
        }
    }

    /// Gives the client `records` for a game of the decklists `lists`, unless the client was
    /// last given them. `records` is called only when they are sent.
    pub fn decks(&self, lists: Vec<Vec<(u8, String)>>, records: impl FnOnce() -> CardRecords) {
        let mut given = self.given.lock().unwrap_or_else(PoisonError::into_inner);
        let lists = Given::Decks(lists);
        if *given == lists {
            return;
        }
        self.inbox.send(Input::Cards(Box::new(records())));
        *given = lists;
    }

    /// Gives the client the records of every name the manual table has shown so far and of
    /// `names`, when one of `names` is new. After a game against an agent, the table's names
    /// replace the game's decklists.
    pub fn names<'a>(&self, names: impl IntoIterator<Item = &'a str>) {
        let mut given = self.given.lock().unwrap_or_else(PoisonError::into_inner);
        if !matches!(*given, Given::Names(_)) {
            *given = Given::Names(BTreeSet::new());
        }
        let Given::Names(known) = &mut *given else {
            unreachable!("the feed holds names")
        };
        let mut added = false;
        for name in names {
            if !name.is_empty() && !known.contains(name) {
                known.insert(name.to_owned());
                added = true;
            }
        }
        if added {
            let request = CardRecordsRequest {
                names: known.iter().cloned().collect(),
            };
            let records = card_records(&request, &beryl_cards::CARDS);
            self.inbox.send(Input::Cards(Box::new(records)));
        }
    }

    /// Gives the client the records of every card `view` names that it was not yet given,
    /// before the view itself is sent. A token is found by its printing's key when it has one
    /// (`ObjectView::token_printing`), as the table draws it, and by its name otherwise.
    pub fn view(&self, view: &PlayerView) {
        self.names(view.objects.iter().flat_map(|object| {
            (object.name.as_deref().into_iter()).chain(object.token_printing.as_deref())
        }));
    }
}
