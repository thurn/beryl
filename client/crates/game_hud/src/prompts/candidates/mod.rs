//! A prompt answered by the decision's candidates, with no shape state.

mod view;

prompt_feature!(Candidates, view::view, super::no_steps);
