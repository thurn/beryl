//! A prompt that steps through the session's available numbers.

mod view;

pub(super) use view::stepper;

prompt_feature!(Number, view::view, super::yes_no::own);
