//! A choice among modes, including the ways to use a card.

mod view;

pub(super) use view::way_faces;

prompt_feature!(Mode, view::view, super::yes_no::own, view::details);
