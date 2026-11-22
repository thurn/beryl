//! Story sources supplied by a host. The packaged catalog works without a checkout.

use std::borrow::Cow;
use std::sync::Arc;

pub use game_probe::story::Source;

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
