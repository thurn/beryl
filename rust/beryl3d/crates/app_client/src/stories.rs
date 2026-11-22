//! Story sources supplied by a host. The packaged catalog works without a checkout.

use std::borrow::Cow;
use std::sync::Arc;

/// A named story supplied to the client. The source uses the same notation as `just b3d story`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub name: Cow<'static, str>,
    pub text: Cow<'static, str>,
    /// Whether the compiled table has a timeline to play.
    pub(crate) plays: bool,
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

/// The stories embedded beside the app, in the same order the inspector lists them.
#[must_use]
pub fn embedded() -> Arc<[Source]> {
    let entries: &[(&str, &str, bool, Option<&str>)] =
        include!(concat!(env!("OUT_DIR"), "/stories.rs"));
    entries
        .iter()
        .map(|&(name, text, plays, feature)| Source {
            plays,
            feature: feature.map(Cow::Borrowed),
            ..Source::new(name, text)
        })
        .collect()
}
