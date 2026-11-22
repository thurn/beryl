# Working in `kit_fx`

This toolkit crate evaluates effects and emits paint such as glows, particles,
lines and outlines. The caller supplies the subject's geometry and timing. Keep
game script interpretation and Magic terms in the game layer.

`effects::EffectRun` carries a timed effect and `effects::write` writes the
drawing records for a moment. Evaluate from the supplied time. Keep the
animation frame free of allocation.

Use an effect story to exercise a drawing layer on its own. The recording checks
in `game_probe/tests/all/effects*.rs` cover effects on game subjects. A layer
change can affect compositing and fragment budgets. Inspect the effect at its
strongest point and as it fades.
