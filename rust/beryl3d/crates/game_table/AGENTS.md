# Working in `game_table`

This crate places the zones and selects card faces and badges. It converts game
objects into the toolkit solver's geometry and keys. Playback owns how objects
move from one placement to the next.

Start at `profiles::Profile::for_window` and `table::place::place`. The profile
supplies regions and slots. Zone code prepares solver inputs, and `Placement`
carries the result. `table::rows::grouped` supplies the pile ordering used by
both placement and the recording signature. Preserve object identity when
moving, hiding or gathering copies.

`tests/all/` covers profiles, clearance, faces and cards tucked under a host.
Use a table story for crowding or overflow. A row or pile change can affect hit
reach, the first visible face and corpus grouping.
