# Working in `game_art`

This crate holds the game-level card face catalog (`faces`) and HUD coverage
artwork (`art::orb` and `art::turn`) shared by `game_table`, `game_play`, and
`game_hud`. Those crates re-export its modules so callers keep stable paths.

Start at `faces::FaceList` for printing, token, and ability face lookups, and
`art::atlas_artwork` / `art::atlas_cells` for life-orb and turn-banner coverage
layers. Keep this crate free of table layout, playback state, and prompt logic
so it stays at the base of the `game` level in `data/client/levels.json`.
