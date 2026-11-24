# Working in `game_manual`

This crate owns the manual table, lobby and manual overlays. The app mounts them
and forwards their typed commands. The session decides which actions are legal.

`features` contains views and message handlers. Start in `manual` for action and
target routing, and in `screens` for the lobby and table screen. Use the
protocol view as the source of truth for game state. Keep pending choices and
view-only state local to the relevant feature.

Opening and turn controls have local unit tests. The user flows are in
`app_client/tests/all/manual*.rs` and `lobby.rs`. A routing change can affect
hover or click targets, the viewer and the assisted and unassisted paths.
