# Project Beryl

A research project to build a Magic: The Gathering AI for cube draft that drafts
and plays at the level of a strong human player. It is a Tauri 2 desktop app for
Apple silicon Macs: a Rust workspace behind a React and TypeScript UI. Free and
open source, under the Apache License 2.0.

- [AGENTS.md](AGENTS.md): how every change is made, tested and delivered.
- [docs/engine-architecture.md](docs/engine-architecture.md): the rules engine's
  design and the boundaries search, the UI and drafting build on.

## Setup

On an Apple silicon Mac:

| Tool                     | Install                                                                        |
| ------------------------ | ------------------------------------------------------------------------------ |
| Xcode Command Line Tools | `xcode-select --install`                                                       |
| Rust                     | [rustup](https://rustup.rs); `rust/rust-toolchain.toml` pins it                |
| Node.js 24               | `brew install node`                                                            |
| uv, just                 | `brew install uv just`                                                         |
| Enact, Beads, Dolt       | [Enact](https://github.com/thurn/enact)'s setup, for delivering changes        |
| Maven, JDK 21            | `just oracle-setup`, with Homebrew; it also builds the XMage and Forge oracles |

Then `just` lists the commands. `just dev` opens the app with hot reload;
`just smoke` builds it and checks that it starts, headless; `just ci` runs every
check.
