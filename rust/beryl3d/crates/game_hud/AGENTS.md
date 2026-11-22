# Working in `game_hud`

This crate builds game-screen features such as prompts, life totals, the log,
pile viewers, the inspector, opening hands and the result. The app registers
them and routes their actions. Table placement lives in `game_table`.

`hud::HudIn` is the shared game view input. Start in `prompts` for `PromptView`
and its shapes, in `inspect` for card details, and in `screens` for opening and
game over. Derive game state from the current view. Keep each feature's view key
in sync with what it reads.

Prompt ordering, the inspector and orb art have local tests. Full HUD, prompt
and compact-layout tests live in `app_client/tests/all/`. A change to a prompt
can affect button visibility, keyboard actions and the space available for
cards.
