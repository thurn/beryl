//! Identifiers shared by every part of Ignite.
//!
//! Each is a newtype over a string or number, so that a ticket ID can never be passed where a
//! commit is expected. Parsing checks only the shape of an identifier, never that it exists.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Why a string is not a valid identifier of some kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdError {
    pub kind: &'static str,
    pub value: String,
    pub reason: &'static str,
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {} {:?}: {}", self.kind, self.value, self.reason)
    }
}

impl std::error::Error for IdError {}

/// Defines a string newtype with `as_str`, `Display`, transparent serde and a `FromStr` that
/// runs the given validation. A `const fn` cannot generate trait impls, so this is a macro.
macro_rules! string_id {
    ($(#[$doc:meta])* $name:ident, $kind:literal, $check:expr) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Parses and validates `value`.
            pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
                let value = value.into();
                let check: fn(&str) -> Result<(), &'static str> = $check;
                match check(&value) {
                    Ok(()) => Ok(Self(value)),
                    Err(reason) => Err(IdError { kind: $kind, value, reason }),
                }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = IdError;
            fn from_str(value: &str) -> Result<Self, IdError> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;
            fn try_from(value: String) -> Result<Self, IdError> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.0
            }
        }
    };
}

fn non_empty_token(value: &str) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("empty");
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("contains whitespace");
    }
    Ok(())
}

fn ticket_shape(value: &str) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("empty");
    }
    if value.split('.').any(str::is_empty) {
        return Err("has an empty dotted part");
    }
    // Root IDs from earlier trackers may hold dashes (`ci-00031ced38`); a part never starts with
    // one, so a dash cannot be mistaken for an option on a command line.
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err("may hold only letters, digits, dots and dashes");
    }
    if value.split('.').any(|part| part.starts_with('-')) {
        return Err("has a dotted part starting with a dash");
    }
    Ok(())
}

fn hex_object(value: &str) -> Result<(), &'static str> {
    if !(4..=64).contains(&value.len()) {
        return Err("must be 4 to 64 hex digits");
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err("must be lowercase hex");
    }
    Ok(())
}

fn sha256_hex(value: &str) -> Result<(), &'static str> {
    if value.len() != 64 || !value.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
        return Err("must be 64 lowercase hex digits");
    }
    Ok(())
}

fn check_shape(value: &str) -> Result<(), &'static str> {
    non_empty_token(value)?;
    match value.split_once(':') {
        Some((family, rest)) if !family.is_empty() && !rest.is_empty() => Ok(()),
        _ => Err("must be <family>:<name>"),
    }
}

string_id!(
    /// A ticket, such as `Tq44s.9.48`: an epic's ID with one dotted part per level below it.
    TicketId,
    "ticket ID",
    ticket_shape
);

impl TicketId {
    /// The ID with its last dotted part removed, which names the parent ticket at creation.
    pub fn parent(&self) -> Option<TicketId> {
        self.0
            .rsplit_once('.')
            .map(|(parent, _)| TicketId(parent.to_owned()))
    }
}

string_id!(
    /// A machine, as the ID its daemon created and stored (such as `m-7f3a`), not its hostname.
    MachineId,
    "machine ID",
    non_empty_token
);

string_id!(
    /// A desktop-app session, such as `local_c8470108`.
    SessionId,
    "session ID",
    non_empty_token
);

string_id!(
    /// A build tree, such as `tree-07`.
    TreeId,
    "tree ID",
    non_empty_token
);

string_id!(
    /// A git commit, as a full or abbreviated lowercase hex object name.
    Commit,
    "commit",
    hex_object
);

string_id!(
    /// A git object (a blob or a tree), as a lowercase hex object name.
    Oid,
    "object ID",
    hex_object
);

string_id!(
    /// The SHA-256 of a check's inputs, as 64 lowercase hex digits.
    InputHash,
    "input hash",
    sha256_hex
);

string_id!(
    /// A short name for an operation still running, such as `L-5521`, that `ignite wait` takes.
    Handle,
    "handle",
    non_empty_token
);

string_id!(
    /// A check, named `<family>:<name>`, such as `rust:game_rules::tests::entering`.
    CheckName,
    "check name",
    check_shape
);

impl CheckName {
    /// The family the check belongs to: the part before the first colon.
    pub fn family(&self) -> &str {
        self.0.split_once(':').map_or("", |(family, _)| family)
    }
}

/// A position in the hub's numbered change feed.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Seq(pub u64);

/// A record's row ID in the hub's `records` table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecordId(pub u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_parent_drops_the_last_part() {
        let id: TicketId = "Tq44s.9.48".parse().unwrap();
        assert_eq!(id.parent().unwrap().as_str(), "Tq44s.9");
        assert_eq!("Tq44s".parse::<TicketId>().unwrap().parent(), None);
    }

    #[test]
    fn malformed_ids_are_refused() {
        assert!("".parse::<TicketId>().is_err());
        assert!("a..b".parse::<TicketId>().is_err());
        assert!("a b".parse::<TicketId>().is_err());
        assert!("ci-00031ced38".parse::<TicketId>().is_ok());
        assert!("a.-1".parse::<TicketId>().is_err());
        assert!("ABC".parse::<Commit>().is_err());
        assert!("3a9e4c1".parse::<Commit>().is_ok());
        assert!("rust".parse::<CheckName>().is_err());
        assert!("ab".repeat(32).parse::<InputHash>().is_ok());
        assert!("ab".parse::<InputHash>().is_err());
    }

    #[test]
    fn check_names_know_their_family() {
        let check: CheckName = "shots:stage-mode-ways/phone".parse().unwrap();
        assert_eq!(check.family(), "shots");
    }

    #[test]
    fn ids_serialize_as_plain_strings_and_validate_on_the_way_in() {
        let id: TicketId = "hrh4.2".parse().unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"hrh4.2\"");
        assert!(serde_json::from_str::<TicketId>("\"a..b\"").is_err());
        assert_eq!(serde_json::to_string(&Seq(7)).unwrap(), "7");
    }
}
