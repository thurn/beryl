//! New root ticket IDs. A root ID is a letter, `T` for a ticket, `E` for an epic, `M` for a
//! meta-ticket or `A` for a ticket Ignite filed on its own, and then a four-character root
//! (`TQ44S`, `EQ44S`, `MAB12`, `AR2D2`). Children add dotted numbers to their parent's ID, so a
//! whole epic's tree shares its `E`.
//!
//! The roots of all the letters are one space: a root used under one letter is never minted
//! under another, so a bare root (`q44s`) names one ticket and lookups try every letter.

use rusqlite::{Connection, OptionalExtension};

use ignite_core::protocol::tickets::{TicketFields, TicketType};

use crate::{Result, StoreError};

/// The characters of a root. The first is never a digit, so a root never looks like a number,
/// and `I`, `L`, `O` and `U` are left out as easy to misread.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The letters a root ID starts with: `T` for a ticket, `E` for an epic, `M` for a meta-ticket
/// and `A` for a ticket Ignite filed on its own.
pub(crate) const PREFIXES: [char; 4] = ['T', 'E', 'M', 'A'];

/// Who a batch's new root tickets are filed by, which decides their letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Filer {
    /// A person or a session: the letter of the ticket's type ([`prefix_for`]).
    Person,
    /// Ignite on its own, such as upkeep filing a flaky check or a regression: `A` whatever
    /// the ticket's type, so the tickets nobody asked for are told apart at a glance.
    Ignite,
}

impl Filer {
    /// The letter of a new root ticket with `fields` filed by `self`.
    pub(super) fn letter(self, fields: &TicketFields) -> char {
        match self {
            Filer::Person => prefix_for(fields),
            Filer::Ignite => 'A',
        }
    }
}

/// The letter a new root ID starts with: `E` for a ticket created as an epic, `M` for a
/// meta-ticket, `T` for any other. The `E` records the type at creation: changing a ticket's
/// type to or from epic later keeps its ID. A child takes its parent's ID whatever its own type.
pub(super) fn prefix_for(fields: &TicketFields) -> char {
    match fields.ticket_type {
        Some(TicketType::Epic) => 'E',
        Some(TicketType::Meta) => 'M',
        _ => 'T',
    }
}

/// A new root ID: `prefix`, a capital letter and three capitals or digits (`TQ44S`), whose root
/// is not [`taken`].
pub(super) fn allocate(conn: &Connection, prefix: char) -> Result<String> {
    loop {
        let mut bytes = [0_u8; 4];
        getrandom::getrandom(&mut bytes)
            .map_err(|e| StoreError::Invalid(format!("no random numbers: {e}")))?;
        // Rejection sampling keeps all 22 letters equally likely as the first character.
        if bytes[0] >= 242 {
            continue;
        }
        let mut root = String::with_capacity(4);
        root.push(char::from(ALPHABET[10 + usize::from(bytes[0]) % 22]));
        for byte in &bytes[1..] {
            root.push(char::from(ALPHABET[usize::from(*byte) % 32]));
        }
        if !taken(conn, &root)? {
            return Ok(format!("{prefix}{root}"));
        }
    }
}

/// Whether `root` is used by a ticket or an alias under any letter. Aliases are stored in
/// lowercase, so they are compared in lowercase, alone or after a project prefix (`by-tq44s`).
fn taken(conn: &Connection, root: &str) -> Result<bool> {
    let ids = PREFIXES.map(|letter| format!("{letter}{root}"));
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM tickets WHERE id IN (?1, ?2, ?3, ?4) UNION ALL \
             SELECT 1 FROM aliases WHERE alias IN (lower(?1), lower(?2), lower(?3), lower(?4)) \
             OR alias LIKE '%-' || lower(?1) OR alias LIKE '%-' || lower(?2) \
             OR alias LIKE '%-' || lower(?3) OR alias LIKE '%-' || lower(?4) \
             LIMIT 1",
            ids,
            |row| row.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_used_under_either_letter_is_taken() {
        let conn = crate::hub::open_in_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO tickets (id, title, created_at, created_by, updated_at, version)
               VALUES ('TQ44S', 'Task', 'x', 'x', 'x', 1), ('EM4K2', 'Epic', 'x', 'x', 'x', 1),
                      ('MAB12', 'Meta', 'x', 'x', 'x', 1), ('AXY34', 'Auto', 'x', 'x', 'x', 1);
             INSERT INTO aliases VALUES ('by-er2d2', 'TQ44S');",
        )
        .unwrap();
        for root in ["Q44S", "M4K2", "AB12", "XY34", "R2D2"] {
            assert!(taken(&conn, root).unwrap(), "{root}");
        }
        assert!(!taken(&conn, "ZZZZ").unwrap());
    }

    #[test]
    fn epics_get_an_e_meta_tickets_an_m_automatic_ones_an_a_and_others_a_t() {
        let epic = TicketFields {
            ticket_type: Some(TicketType::Epic),
            ..TicketFields::default()
        };
        assert_eq!(prefix_for(&epic), 'E');
        let meta = TicketFields {
            ticket_type: Some(TicketType::Meta),
            ..TicketFields::default()
        };
        assert_eq!(prefix_for(&meta), 'M');
        assert_eq!(prefix_for(&TicketFields::default()), 'T');
        assert_eq!(Filer::Person.letter(&epic), 'E');
        assert_eq!(Filer::Ignite.letter(&epic), 'A');
        assert_eq!(Filer::Ignite.letter(&TicketFields::default()), 'A');
        let conn = crate::hub::open_in_memory().unwrap();
        let id = allocate(&conn, 'E').unwrap();
        assert!(id.starts_with('E') && id.len() == 5, "{id}");
    }
}
