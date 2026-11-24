//! Deck library and card-store operations shared by the Tauri and HTTP commands.

use std::io::ErrorKind;
use std::path::Path;

use beryl_card_store::{STORE, facts};
use beryl_protocol::ipc::decks::{
    CardSearchHit, CardSearchRequest, CardTokenLink, CardTokensRequest, DeckDeleteRequest,
    DeckImportReply, DeckImportRequest, DeckOpenReply, SavedDeck,
};
use beryl_protocol::{SessionError, card_records};
pub use beryl_table::deck::Decks;
use beryl_table::deck::{self, Deck, DeckError};

fn imported(deck: Deck) -> DeckImportReply {
    let unknown = deck::unknown(&deck, &STORE);
    DeckImportReply {
        name: deck.name,
        main: deck.main,
        side: deck.side,
        unknown,
        skipped: deck.skipped,
    }
}

fn error(error: DeckError) -> SessionError {
    match error {
        DeckError::Io(error) => SessionError::Io {
            reason: error.to_string(),
        },
        DeckError::Json(error) => SessionError::Io {
            reason: error.to_string(),
        },
        error @ (DeckError::InvalidCod | DeckError::EmptyName) => SessionError::Deck {
            reason: error.to_string(),
        },
    }
}

/// Parses one pasted or opened deck and saves it under the supplied name.
pub fn import(decks: &Decks, request: DeckImportRequest) -> Result<DeckImportReply, SessionError> {
    let mut deck = match (request.text, request.cod) {
        (Some(text), None) => deck::parse_text(&text),
        (None, Some(cod)) => deck::parse_cod(&cod).map_err(error)?,
        _ => {
            return Err(SessionError::Deck {
                reason: "Provide either deck text or a Cockatrice deck.".to_owned(),
            });
        }
    };
    deck.name = request.name;
    decks.save(&deck).map_err(error)?;
    Ok(imported(deck))
}

/// Lists the local decks and their counts, with lists available to start a table.
pub fn list(decks: &Decks) -> Result<Vec<SavedDeck>, SessionError> {
    let mut saved = Vec::new();
    for name in decks.list().map_err(error)? {
        let deck = match decks.load(&name) {
            Ok(deck) => deck,
            // Another command can delete a deck between listing and loading its file.
            Err(DeckError::Io(error)) if error.kind() == ErrorKind::NotFound => continue,
            Err(failure) => return Err(error(failure)),
        };
        let (main_count, side_count) = deck::counts(&deck);
        saved.push(SavedDeck {
            deck: imported(deck),
            main_count,
            side_count,
        });
    }
    Ok(saved)
}

pub fn delete(decks: &Decks, request: &DeckDeleteRequest) -> Result<(), SessionError> {
    match decks.delete(&request.name) {
        Err(DeckError::Io(error)) if error.kind() == ErrorKind::NotFound => Ok(()),
        result => result.map_err(error),
    }
}

/// Reads a file selected in the desktop picker.
pub fn read_file(path: &Path) -> Result<DeckOpenReply, SessionError> {
    let extension = path.extension().and_then(std::ffi::OsStr::to_str);
    match extension.map(str::to_ascii_lowercase).as_deref() {
        Some("txt" | "dec") => Ok(DeckOpenReply::Text {
            text: file_text(path)?,
        }),
        Some("cod") => Ok(DeckOpenReply::Cod {
            text: file_text(path)?,
        }),
        _ => Err(SessionError::Deck {
            reason: "Choose a .txt, .dec or .cod deck file.".to_owned(),
        }),
    }
}

fn file_text(path: &Path) -> Result<String, SessionError> {
    std::fs::read_to_string(path).map_err(|error| SessionError::Io {
        reason: format!("{}: {error}", path.display()),
    })
}

/// Finds cards or token printings by name, using their first printed face for the row.
pub fn search(request: &CardSearchRequest) -> Vec<CardSearchHit> {
    let records = if request.tokens {
        STORE.search_tokens(&request.text, request.limit)
    } else {
        STORE.search(&request.text, request.limit)
    };
    records
        .into_iter()
        .filter_map(|record| {
            let face = record.faces.into_iter().next()?;
            Some(CardSearchHit {
                name: record.name,
                image: card_records::image("normal", face.side.as_str(), &record.scryfall_id),
                scryfall_id: record.scryfall_id,
                type_line: face.type_line,
                mana_cost: face.mana_cost,
                power: face.power,
                toughness: face.toughness,
            })
        })
        .collect()
}

/// The name and exact printing of each token the named card makes.
pub fn tokens(request: &CardTokensRequest) -> Vec<CardTokenLink> {
    STORE
        .card_ignoring_case(&request.name)
        .map_or_else(Vec::new, |record| {
            facts::tokens_made(&record)
                .iter()
                .map(|token| CardTokenLink {
                    name: token.name.clone(),
                    scryfall_id: token.scryfall_id.clone(),
                })
                .collect()
        })
}
