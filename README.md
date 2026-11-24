# Project Beryl

A research project to build a Magic: The Gathering AI that plays at the level of
a strong human player, with a native Rust 3D client for macOS, Windows, iOS and
Android. The project is free and open source under the Apache License 2.0.

- [AGENTS.md](AGENTS.md): how every change is made, tested and delivered.
- [docs/engine-architecture.md](docs/engine-architecture.md): the rules engine's
  design and the boundaries search and the UI build on.
- [docs/beryl-3d.md](docs/beryl-3d.md) describes the client, its hosts and its
  inspection tools.

## Setup

On an Apple silicon Mac:

| Tool                     | Install                                                                                  |
| ------------------------ | ---------------------------------------------------------------------------------------- |
| Xcode Command Line Tools | `xcode-select --install`                                                                 |
| Rust                     | [rustup](https://rustup.rs); `rust-toolchain.toml` pins it                               |
| uv                       | `brew install uv`                                                                        |
| just 1.59+               | `cargo install just --locked`                                                            |
| cargo-nextest            | `cargo install cargo-nextest --locked`; Ignite runs the Rust tests with it               |
| Ignite                   | `cargo install --locked --path ignite/crates/ignite_cli`, then `ignite install` here     |
| Maven, JDK 21            | `just oracle-setup`, with Homebrew; it also builds the XMage and Forge oracles           |
| Rosetta                  | `softwareupdate --install-rosetta --agree-to-license`; Forge's build runs an x86-64 tool |
| iOS and Android SDKs     | `just mobile-setup`                                                                      |
| Lima                     | `brew install lima`, then `just bench-count` once to create the count gate's VM          |

Beryl's tickets are on Ignite's hub, hosted on JARVIS. Connect to the tailnet,
then `ignite ticket` reads and writes them and `ignite prime` prints the
workflow. Browse tickets on the [Beryl dashboard][tickets].

Run the setup recipes one at a time: each installs with Homebrew, which takes a
lock.

On Windows 11 (x64), which runs the app, the tests and the agents, but not
`pyre` or the instruction-count gate (`just bench-count`, `just count-gate`):

| Tool                      | Install                                                      |
| ------------------------- | ------------------------------------------------------------ |
| Visual Studio Build Tools | The "Desktop development with C++" workload                  |
| Git for Windows           | `winget install Git.Git`; `just` runs recipes in its `sh`    |
| Rust                      | [rustup](https://rustup.rs); `rust-toolchain.toml` pins it   |
| uv                        | `winget install astral-sh.uv`                                |
| just 1.59+                | `cargo install just --locked`                                |
| cargo-nextest             | `cargo install cargo-nextest --locked`                       |
| JDK 21, Maven             | `just oracle-setup`, with winget and Apache's Maven download |

Set `PYTHONUTF8=1`, so Python reads and writes text as UTF-8. Keep the checkout
and `BERYL_CACHE` on one Dev Drive (ReFS): copies there are block clones, as
APFS clones are on macOS, and Defender's scans slow builds less.

Run `just dev` to build and open the native desktop app or `just bundle` to
package a release. On macOS the bundle is `Beryl.app`. On Windows keep
`beryl.exe` beside the packaged `audio/` directory. `just smoke` checks desktop
startup without a window, and `ignite check` runs the landing gate. `just` lists
the public commands and `just client` lists the client tools.

On macOS, install full Xcode for iOS builds and run `just mobile-setup` for the
phone toolchains. Use `just ios` or `just android` to launch the app in the
simulator or emulator. Run `just ios-smoke` or `just android-smoke` to replay a
recording headlessly. Both phone apps use `ca.thurn.beryl`. See
[Hosts](docs/beryl-3d.md#hosts) for platform requirements and
[Desktop data](docs/beryl-3d.md#desktop-data) for saved games, decks and
profiles.

## Credits

The step icons beside each life total are from
[Phosphor Icons](https://phosphoricons.com), Copyright (c) 2023 Phosphor Icons,
under the MIT License.

[tickets]: https://jarvis.tail4b0774.ts.net:8443/by/
