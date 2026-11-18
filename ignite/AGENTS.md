# Working on Ignite

Ignite is the build, check and delivery system that replaces Enact.
`docs/ignite.md` says what it does; the code contract in the design field of the
epic `Thrh4` (`tk show Thrh4 --json`) says where each part lives and which
ticket owns which file. These are the contract's rules.

## The workspace

- `ignite/` is a Cargo workspace of its own. It shares no crate, lock file or
  target directory with `rust/`, and nothing in the game depends on it.
- Crates: `ignite_core` (pure logic: IDs, time, `ignite.toml`, check manifests,
  the protocol, the traits and their fakes), `ignite_os` (the operating system),
  `ignite_git` (every `git` call), `ignite_store` (SQLite), `ignite_hub` (the
  HTTP server and dashboard), `ignite_daemon` (one per machine) and `ignite_cli`
  (the `ignite` executable).
- Check it with `cd ignite && cargo test -p <crate>` for one crate, or the
  `ignite` lane (`just ci ignite`) for formatting, clippy and every test.

## Rules

- Ignite's code names no path, crate, tool, branch or check of this repository.
  Those come from `ignite.toml` and `ignite/checks/*.toml`. A test fails if a
  file under `ignite/crates/` contains this repository's name.
- A ticket owns whole files. A shared file (a `mod.rs`, a schema, the protocol)
  belongs to the ticket that created it; later tickets only append to it.
- A stub module says in its header which ticket owns it, and its bodies are
  `todo!()`. The owning ticket replaces the bodies, removes the
  `#![allow(unused_variables)]` and rewrites the header.
- Test against the fakes in `ignite_core::fakes`, never against another ticket's
  unfinished code. Every trait in `ignite_core::traits` has one.
- A verb nobody has built is listed in `NOT_BUILT` in
  `crates/ignite_cli/src/client.rs`; the ticket that builds it deletes its row
  and registers a `Verb` (`ignite_daemon::server`) with the daemon.
- A schema change appends a migration to `MIGRATIONS` in `ignite_store`'s `hub`
  or `local` module and updates the row structs, which a test compares with the
  tables. It never edits an applied migration.
- No async runtime: threads and blocking I/O.
- `unsafe` only in `ignite_os`, whose macOS and Windows halves of each module
  are separate files (`<module>/macos.rs`, `<module>/windows.rs`).
- A new dependency goes in `[workspace.dependencies]` with a comment saying what
  it replaces.
- Time comes from the `Clock` trait. Tests never sleep to wait for something and
  never compare against wall-clock thresholds.
- No source file is over 800 lines.
- Exit codes are the same for every verb: 0 success, 1 usage or configuration, 2
  a check failed, 3 a conflict or lost claim, 4 the hub or remote is
  unreachable, 5 still running with a handle.
