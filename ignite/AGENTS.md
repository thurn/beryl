# Working on Ignite

Ignite is the build, check and delivery system. `docs/ignite.md` says what it
does; the code contract in the design field of the epic `THRH4`
(`ignite ticket show THRH4 --json`) says where each part lives and which ticket
owns which file. These are the contract's rules.

## The workspace

- `ignite/` is a Cargo workspace of its own. It shares no crate, lock file or
  target directory with `rust/`, and nothing in the game depends on it.
- Crates: `ignite_core` (pure logic: IDs, time, `ignite.toml`, check manifests,
  the protocol, the traits and their fakes), `ignite_os` (the operating system),
  `ignite_git` (every `git` call), `ignite_store` (SQLite), `ignite_hub` (the
  HTTP server and dashboard), `ignite_daemon` (one per machine) and `ignite_cli`
  (the `ignite` executable).
- Check it with `cd ignite && cargo nextest run -p <crate>` for one crate, or
  `ignite test ignite ignite-lint` for every test (the `ignite` family),
  formatting and clippy (the `ignite-lint` family).

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

## The dashboard

The dashboard is the HTML that `ignite_hub` serves at the hub's root: Rust page
builders in `crates/ignite_hub/src/dashboard/`, and the stylesheets and
`cirrus.js` in `crates/ignite_hub/static/`, which `dashboard/assets.rs` compiles
into the binary.

- Never test a dashboard change on the live hub, and never run scripts or inject
  styles in its pages. See it in a preview instead:
  `cargo run -p ignite_hub --example preview -- --repo ..` serves this tree's
  dashboard at `http://127.0.0.1:7399/` over a copy of the machine's hub
  database (`--from <hub.db>` names another; `--refresh` copies it again). Edits
  in the preview change only the copy. The assets are compiled in, so restart
  the preview after editing `static/`.
- Drive the preview with headless Playwright. The desktop app's browser pane
  draws no frames while it is hidden, so view transitions and other animations
  never start there.
- `tests/dashboard_*.rs` check the pages' HTML against seeded databases. Add to
  them for any change in what a page shows.
