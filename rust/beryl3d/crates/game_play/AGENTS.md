# Working in `game_play`

This crate plays protocol scripts into tracks and effects, including beats,
notices, arrows, marks and sound cues. Table placement supplies rest poses.
Prompt and screen construction belong to the HUD and app features.

Start at `play::Show` for queued updates and frame output, `beat` for timing,
and `verbs` for the concrete motions. Preserve the ordering of script steps and
the latest versus displayed view distinction. Evaluate animation at the
timestamp supplied by the caller.

`light/lines.rs` tests line geometry locally. `game_probe/tests/all/` covers
verbs, effects, sound and playback through recordings. Timing changes can move
later steps as well as the edited verb. Inspect the affected span.
