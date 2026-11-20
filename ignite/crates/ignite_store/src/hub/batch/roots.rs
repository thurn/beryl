//! New root ticket IDs. A root ID is a letter, `T` for a ticket, `E` for an epic, `M` for a
//! meta-ticket, `A` for a ticket Ignite filed on its own or `B` for a player's bug report, and
//! then a root of at least four characters (`TQ44S`, `EQ44S`, `MAB12`, `AR2D2`, `BX9K2`).
//! Children add dotted numbers to their parent's ID, so a whole epic's tree shares its `E`.
//!
//! Four characters give 720,896 roots. As they fill, a random four-character root is more
//! often taken, and after [`TRIES_PER_LENGTH`] taken roots in a row [`allocate`] tries one
//! character longer, so IDs stay short for as long as the space allows and never run out.
//!
//! The roots of all the letters are one space: a root used under one letter is never minted
//! under another, so a bare root (`q44s`) names one ticket and lookups try every letter.

use rusqlite::{Connection, OptionalExtension};

use ignite_core::protocol::tickets::{TicketFields, TicketType};

use crate::{Result, StoreError};

/// The characters of a root. The first is never a digit, so a root never looks like a number,
/// and `I`, `L`, `O` and `U` are left out as easy to misread.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// How many taken roots of one length [`allocate`] draws in a row before it tries a root one
/// character longer. With a fraction `f` of the four-character roots taken, a new ID is longer
/// with probability `f^8`: under 1% while half are taken, about 10% at three quarters.
const TRIES_PER_LENGTH: usize = 8;

/// The shortest root.
const MIN_ROOT_LENGTH: usize = 4;

/// The longest root [`allocate`] tries before it gives up. Eight characters are a trillion
/// roots, so reaching it means `taken` is wrong, not that the space is full.
const MAX_ROOT_LENGTH: usize = 8;

/// The letters a root ID starts with: `T` for a ticket, `E` for an epic, `M` for a meta-ticket
/// `A` for a ticket Ignite filed on its own and `B` for a player's bug report.
pub(crate) const PREFIXES: [char; 5] = ['T', 'E', 'M', 'A', 'B'];

/// The label that marks a ticket filed from a player's bug report (`by bugs file`).
pub(crate) const BUG_REPORT_LABEL: &str = "bug-report";

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
/// meta-ticket, `B` for a ticket labelled [`BUG_REPORT_LABEL`], `T` for any other. The `E` records the type at creation: changing a ticket's
/// type to or from epic later keeps its ID. A child takes its parent's ID whatever its own type.
pub(super) fn prefix_for(fields: &TicketFields) -> char {
    match fields.ticket_type {
        Some(TicketType::Epic) => 'E',
        Some(TicketType::Meta) => 'M',
        _ if has_bug_report_label(fields) => 'B',
        _ => 'T',
    }
}

fn has_bug_report_label(fields: &TicketFields) -> bool {
    fields
        .labels
        .as_ref()
        .is_some_and(|labels| labels.iter().any(|label| label == BUG_REPORT_LABEL))
}

/// A new root ID: `prefix`, a capital letter and at least three more capitals or digits
/// (`TQ44S`), whose root is not [`taken`]. Roots are four characters long until
/// [`TRIES_PER_LENGTH`] random four-character roots in a row are taken, then five, and so on.
pub(crate) fn allocate(conn: &Connection, prefix: char) -> Result<String> {
    for length in MIN_ROOT_LENGTH..=MAX_ROOT_LENGTH {
        for _ in 0..TRIES_PER_LENGTH {
            let root = random_root(length)?;
            if !taken(conn, &root)? {
                return Ok(format!("{prefix}{root}"));
            }
        }
    }
    Err(StoreError::Invalid(format!(
        "no free ticket root of up to {MAX_ROOT_LENGTH} characters"
    )))
}

/// A random root of `length` characters: a capital letter, then capitals or digits.
fn random_root(length: usize) -> Result<String> {
    let mut root = String::with_capacity(length);
    while root.len() < length {
        let mut byte = [0_u8; 1];
        getrandom::getrandom(&mut byte)
            .map_err(|e| StoreError::Invalid(format!("no random numbers: {e}")))?;
        let byte = usize::from(byte[0]);
        if root.is_empty() {
            // Rejection sampling keeps all 22 letters equally likely as the first character.
            if byte < 242 {
                root.push(char::from(ALPHABET[10 + byte % 22]));
            }
        } else {
            root.push(char::from(ALPHABET[byte % 32]));
        }
    }
    Ok(root)
}

/// Whether `root` is used by a ticket or an alias under any letter. Aliases are stored in
/// lowercase, so they are compared in lowercase, alone or after a project prefix (`by-tq44s`).
fn taken(conn: &Connection, root: &str) -> Result<bool> {
    let ids = PREFIXES.map(|letter| format!("{letter}{root}"));
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM tickets WHERE id IN (?1, ?2, ?3, ?4, ?5) UNION ALL \
             SELECT 1 FROM aliases WHERE alias IN \
             (lower(?1), lower(?2), lower(?3), lower(?4), lower(?5)) \
             OR alias LIKE '%-' || lower(?1) OR alias LIKE '%-' || lower(?2) \
             OR alias LIKE '%-' || lower(?3) OR alias LIKE '%-' || lower(?4) \
             OR alias LIKE '%-' || lower(?5) \
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
                      ('MAB12', 'Meta', 'x', 'x', 'x', 1), ('AXY34', 'Auto', 'x', 'x', 'x', 1),
                      ('BZ8Z8', 'Bug', 'x', 'x', 'x', 1);
             INSERT INTO aliases VALUES ('by-er2d2', 'TQ44S');",
        )
        .unwrap();
        for root in ["Q44S", "M4K2", "AB12", "XY34", "Z8Z8", "R2D2"] {
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
        let bug = TicketFields {
            labels: Some(vec!["tier:opus".to_owned(), BUG_REPORT_LABEL.to_owned()]),
            ..TicketFields::default()
        };
        assert_eq!(prefix_for(&bug), 'B');
        assert_eq!(Filer::Person.letter(&bug), 'B');
        assert_eq!(Filer::Ignite.letter(&bug), 'A');
        assert_eq!(Filer::Person.letter(&epic), 'E');
        assert_eq!(Filer::Ignite.letter(&epic), 'A');
        assert_eq!(Filer::Ignite.letter(&TicketFields::default()), 'A');
        let conn = crate::hub::open_in_memory().unwrap();
        let id = allocate(&conn, 'E').unwrap();
        assert!(id.starts_with('E') && id.len() == 5, "{id}");
    }

    /// Inserts a ticket for every four-character root, so `allocate` can only find a longer one.
    #[test]
    fn a_full_four_character_space_gives_five_character_roots() {
        let conn = crate::hub::open_in_memory().unwrap();
        conn.execute_batch(
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < 31),
                  a(c, i) AS (SELECT substr('0123456789ABCDEFGHJKMNPQRSTVWXYZ', i + 1, 1), i
                              FROM n)
             INSERT INTO tickets (id, title, created_at, created_by, updated_at, version)
             SELECT 'T' || w.c || x.c || y.c || z.c, 't', 'x', 'x', 'x', 1
             FROM a w, a x, a y, a z WHERE w.i >= 10;",
        )
        .unwrap();
        let id = allocate(&conn, 'T').unwrap();
        assert_eq!(id.len(), 6, "{id}");
        let root = &id[1..];
        assert!(root.as_bytes()[0].is_ascii_uppercase(), "{id}");
        assert!(root.bytes().all(|b| ALPHABET.contains(&b)), "{id}");
        assert!(!taken(&conn, root).unwrap());
    }
}
