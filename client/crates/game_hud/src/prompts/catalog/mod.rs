//! The catalog prompt and the steps it makes on the table.

mod view;

use super::{Shown, Steps};

/// The marks and arrows the client draws itself for the prompt `shown`, which was `was`: none,
/// since a catalog is about no object.
pub(super) fn own(_shown: Option<Shown>, _was: Option<Shown>, _steps: &mut Steps) {}

prompt_feature!(Catalog, view::view, own);
