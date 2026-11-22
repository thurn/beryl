# Working in `kit_space`

This toolkit crate owns cameras, pose tracks and arrangement. It places opaque
keys with geometry and grouping supplied by its caller. Zone rules and game
object identities belong to the game layer.

`arrange::arrange` solves a `Container` and its `Item`s into a `Solved` result.
Items stay in input order, including hidden items with rest poses. Each visible
item carries its layer and any press target. `camera::Camera` projects poses and
`motion` supplies time-based tracks.

`tests/all/arrange.rs` and `motion.rs` cover placement and tracks. Exercise
overflow and hit reachability with a container story when changing a policy.
Table rows, hands and trays share the solver.
