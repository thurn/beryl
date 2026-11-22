//! A story supplied by the host or stored in a capture.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// A named story in the notation the inspector reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub name: Cow<'static, str>,
    pub text: Cow<'static, str>,
    /// Whether the compiled table has a timeline to play.
    pub plays: bool,
    /// The feature directory this story is kept under, when present.
    pub feature: Option<Cow<'static, str>>,
}

impl Source {
    /// A story written in the notation the inspector reads.
    #[must_use]
    pub const fn new(name: &'static str, text: &'static str) -> Self {
        Self {
            name: Cow::Borrowed(name),
            text: Cow::Borrowed(text),
            plays: false,
            feature: None,
        }
    }

    /// A story the host read from its checkout.
    #[must_use]
    pub fn owned(name: String, text: String, feature: Option<String>) -> Self {
        Self {
            name: Cow::Owned(name),
            text: Cow::Owned(text),
            plays: false,
            feature: feature.map(Cow::Owned),
        }
    }
}
