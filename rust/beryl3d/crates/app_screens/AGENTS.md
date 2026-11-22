# Working in `app_screens`

This crate builds utility screens and controls, including start, settings, bug
report, gallery and testing. Game prompts and manual play belong to the game
crates. The app client owns registration, routing and host requests.

Start at `screens::screen` and `ScreenIn` for the selected screen, or
`screens::settings` for the settings sheet. Build views from the given inputs
and local state. An equal feature view key must keep the same controls and
actions.

Host-facing screen tests live in `app_client/tests/all/`, including
`screens.rs`, `screens_compact.rs`, `settings_sheet.rs` and `bug_report.rs`.
Check both window classes when changing navigation or a shared control.
