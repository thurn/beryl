//! Asset preparation with the face list a match supplies.

mod faces;
mod frame;
mod hosts;

use beryl_card_records::{CardRecordsRequest, TokenMakers, card_records, record};
use game_table::faces::FaceList;

/// The faces of `decklists`, each a list of `(count, card name)`, and of every token their
/// cards make, listed as the client lists them from the same cards' records. A token name that
/// several printings share shows the printing the first decklist card that makes it links. A
/// name the card store does not know has no face.
#[must_use]
pub fn decklist_faces(decklists: &[&[(u8, &str)]]) -> FaceList {
    let mut names: Vec<String> = Vec::new();
    let mut made: Vec<(String, &str)> = Vec::new();
    for &(_, name) in decklists.iter().flat_map(|list| list.iter()) {
        if let Some(card) = record(name, false, TokenMakers::NONE) {
            for token in card.tokens {
                names.push(token.name.clone());
                made.push((token.name, name));
            }
        }
        names.push(name.to_owned());
    }
    let pairs: Vec<(&str, &str)> = made.iter().map(|(t, c)| (t.as_str(), *c)).collect();
    FaceList::from_records(&card_records(
        &CardRecordsRequest { names },
        TokenMakers::new(&pairs),
    ))
}
