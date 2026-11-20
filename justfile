# Beryl's commands; `just` lists them. Most wrap `by` (tools/), and extra arguments pass
# straight through: `just smoke --seconds 30`.

set positional-arguments

# Recipes are POSIX shell; on Windows, Git for Windows' (PowerShell has no `sh` on its PATH).

set windows-shell := ["C:/Program Files/Git/bin/sh.exe", "-cu"]

# Python reads and writes text as UTF-8, as it does off Windows.

export PYTHONUTF8 := "1"
by := "uv run --quiet --project tools by"

# The CPU the `search` profile builds for: a Cargo profile cannot set it (architecture §18).
# Off Apple silicon, this machine's own.

search_cpu := if arch() == "aarch64" { "-C target-cpu=apple-m4" } else { "-C target-cpu=native" }

# Low priority for long compute: utility QoS, under which macOS gives interactive work and
# checks the CPU first. `nice` does not: beside nice'd work a check ran as slowly as beside
# work at full priority. Windows has neither: `by low` starts the work at idle priority.

by_anywhere := "uv run --quiet --project " + quote(justfile_directory() / "tools") + " by"

low := if os() == "windows" { by_anywhere + " low --" } else { "env BERYL_LOW_PRIORITY=1 taskpolicy -c utility" }

# Low priority, kept awake, for a blunder hunt that runs for hours.

awake := if os() == "windows" { by_anywhere + " low --awake --" } else { "caffeinate -i nice -n 10" }

default:
    @just --list --unsorted

# The app with hot reload, in a window (agents use `smoke`)
[group('app')]
dev *args:
    {{ by }} dev "$@"

# Build once and open a host and guest at the same local table (humans)
[group('app')]
table-pair *args:
    {{ by }} table-pair "$@"

# Build the debug app and launch it headless until its UI loads (--seconds N, --no-build)
[group('app')]
smoke *args:
    {{ by }} smoke "$@"

# Beryl 3D's commands (docs/beryl-3d.md): `just b3d` lists them, `just b3d <command> --help` explains one
[group('app')]
b3d *args:
    @{{ by }} b3d "$@"

# Install the phone apps' toolchains: Rust targets, CocoaPods, the iOS Simulator runtime, the Android SDK (idempotent)
[group('app')]
mobile-setup:
    {{ by }} mobile-setup

# Build the debug iOS app, boot the Beryl iPhone simulator and launch it there (--no-build)
[group('app')]
ios *args:
    {{ by }} ios "$@"

# Build the debug iOS app and launch it in a headless simulator until its UI loads (--seconds N, --no-build)
[group('app')]
ios-smoke *args:
    {{ by }} ios-smoke "$@"

# Build and sign the release iOS app and install it on the paired iPhone (--no-install, --user-asked)
[group('app')]
ios-device *args:
    {{ by }} ios-device "$@"

# Build the debug Android app, start the Beryl emulator and launch it there (--no-build)
[group('app')]
android *args:
    {{ by }} android "$@"

# Build the debug Android app and launch it in a headless emulator until its UI loads (--seconds N, --no-build)
[group('app')]
android-smoke *args:
    {{ by }} android-smoke "$@"

# Pull bug reports from a booted simulator and running emulator (--device for paired iPhone; or ZIP path)
[group('app')]
mobile-bugs *args:
    {{ by }} mobile-bugs "$@"

# Screenshot UI states headless into the cache and print the paths (shot names; ui/shots.json)
[group('app')]
ui-shots *args:
    {{ by }} ui-shots "$@"

# Type-check the UI with tsc, first restoring ui/node_modules from the cache
[group('app')]
ui-typecheck:
    {{ by }} deps
    cd ui && npm run --silent typecheck

# Run the UI's tests that need no browser (ui/scripts/*.test.mjs), first restoring ui/node_modules
[group('app')]
ui-unit:
    {{ by }} deps
    cd ui && npm test --silent

# Play the game screen headless through a recorded fixture: drag, click, hover (title filters)
[group('app')]
ui-test *args:
    {{ by }} ui-test "$@"

# Play every recorded fixture to its end on the stage, both layouts, under the audits (fixture names)
[group('app')]
ui-play *args:
    {{ by }} ui-play "$@"

# Time the frames of fixtures played live on a production build (--verbs, --phone, --browser chromium --throttle N)
[group('app')]
ui-perf *args:
    {{ by }} ui-perf "$@"

# Count the UI's renders, DOM mutations and style and layout work playing fixtures (--phone, --components, --gate [--update])
[group('app')]
ui-counts *args:
    {{ by }} ui-counts "$@"

# Live games in a headless page an agent drives over HTTP, until interrupted (--release, --port N)
[group('app')]
ui-live *args:
    {{ by }} ui-live "$@"

# The cards' spec pages, each moment a live game
[group('app')]
specs *args:
    {{ by }} specs "$@"

# Arena recordings indexed into ~/arena as a UI reference: list, index, find, film, cards or card
video *args:
    {{ by }} video "$@"

# Playable card review and the reproducible scenario viewer.
playground *args:
    {{ by }} playground "$@"

scenarios *args:
    {{ by }} scenarios "$@"

# The app's bug reports: list, import a ZIP, file tickets, or mark done
[group('app')]
bugs *args:
    {{ by }} bugs "$@"

# The release Beryl.app, under rust/target/tauri/release/bundle/macos (Windows: an installer under bundle/nsis)
[group('app')]
bundle:
    {{ by }} bundle

# Time benchmarks under the search profile, refusing a busy, unplugged or throttled machine (cargo bench flags; --force)
[group('bench')]
bench *args:
    {{ by }} bench "$@"

# The scale rehearsal: build cloned cards in the set crates and time the compiler (--assert)
[group('bench')]
scale *args:
    {{ by }} scale "$@"

# Where agent time, CPU and tokens go, per ticket (ingest [--rebuild], sql '<query>', coverage; --json, --no-ingest; serve runs at normal priority)
[group('bench')]
efficiency *args:
    @{{ if args =~ '^serve( |$)' { "" } else { low } }} uv run --quiet --no-dev --project efficiency beryl-efficiency "$@"

# Gungraun instruction counts in the Lima VM (cargo bench flags; default -p beryl_bench --bench '*_counts')
[group('bench')]
bench-count *args:
    {{ by }} bench-count "$@"

# Fail when the rollout benchmark's instruction count rises past its bar, or with --family <name> a family's present cost past its ratio (count-gate.toml)
[group('bench')]
count-gate *args:
    {{ by }} count-gate "$@"

# The tools a card is written with: brief <name> prints the packet it is written from, new <name> starts its
# file, check <name> [--no-oracles] builds it, plays its scenarios in Beryl, XMage and Forge and diffs its
# rendered text, render <name>...|--all diffs rendered text against Oracle text, path <name> its file,
# header <name>|--all rewrites its file's generated header from the card store, diverge <name> <scenario>
# <xmage|forge> --cr <rule> --reason <text> records an oracle's own fault when the other oracle agrees,
# rulings <name> [--add] shows rulings and their ledger entries and can add drafts
[group('data')]
card command *args:
    {{ by }} card "$@"

# List a set's first printings or plan its unwritten cards (plan <code> [--beads])
[group('data')]
set command code *args:
    {{ by }} set "$@"

# Build the card store from Scryfall's bulk data (--refresh)
[group('data')]
cards *args:
    {{ by }} cards "$@"

# Count the mechanics in Forge's scripts that the vocabulary lacks, by pool cards blocked (--top N)
[group('data')]
census *args:
    {{ by }} census "$@"

# Hold every Comprehensive Rules link and quotation in the tree to the rules page (network; --refresh)
[group('data')]
citations *args:
    {{ by }} citations "$@"

# Derive Arena's values and write docs/ui-conventions.md from data/ (--check)
[group('data')]
conventions *args:
    {{ by }} conventions "$@"

# Write ui/src/design/tokens.css from data/design-tokens.json (--check)
[group('data')]
design-tokens *args:
    {{ by }} design-tokens "$@"

# Transcode the UI's music and sounds from the staged CC0 packs into ui/public/audio (--check)
[group('data')]
audio *args:
    {{ by }} audio "$@"

# Install Maven and JDK 21, fetch XMage and Forge at their pins and build them, in the cache
[group('oracles')]
oracle-setup:
    {{ by }} oracle-setup

# Check every card scenario against the oracles, with a JSON summary (card names for just those)
[group('oracles')]
oracle *cards:
    {{ by }} oracle "$@"

# Run one XMage test class and one Forge sim game (xmage or forge for just one)
[group('oracles')]
oracle-smoke *args:
    {{ by }} oracle-smoke "$@"

# The Rust workspace's tests under cargo-nextest (-p beryl_app, a name filter, other nextest flags); `ignite test` runs them as the gate does
[group('test')]
test-rust *args:
    cd rust && cargo nextest run "$@"

# Write beryl_workspace_hack's manifest, so a build of any one Rust package gives its dependencies the whole workspace's features
[group('test')]
workspace-hack:
    {{ by }} workspace-hack

# Random-game fuzzing at length: N engine games (and N/4 per keystone family), then N session games with every script checked, optimized but with debug assertions on (with the fuzz tests no check runs, which are ignored)
[group('test')]
fuzz games="10000":
    cd rust && export BERYL_FUZZ_GAMES="$1" CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true && \
    cargo test --release -p beryl_search --test all -- games:: shrink:: families:: --nocapture --include-ignored && \
    cargo test --release -p beryl_session --lib fuzz:: -- --nocapture --include-ignored

# The stage's verbs held to Arena's numbers: captured in stepped stage time and measured as Arena's clips were (--values: durations, delays and staggers only)
[group('test')]
motion *args:
    {{ by }} motion "$@"

# Every decision sequence from the scenarios' end positions, deeper than the fast check: one card's or all (--depth N, --nodes N per scenario)
[group('test')]
exhaust *args:
    card=""; depth=6; nodes=20000; \
    while [ $# -gt 0 ]; do case "$1" in \
        --depth) depth="$2"; shift 2 ;; \
        --nodes) nodes="$2"; shift 2 ;; \
        *) card="$1"; shift ;; \
    esac; done; \
    cd rust && BERYL_EXHAUST_CARD="$card" BERYL_EXHAUST_DEPTH="$depth" BERYL_EXHAUST_NODES="$nodes" CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true cargo test --release -p beryl_cards --test exhaust every_scenario -- --nocapture

# Adds saves to the frozen corpus, weekly or so: seeded games covering every card and decision, or one replay (--replay FILE, --games N, --seed N)
[group('test')]
[no-cd]
freeze-saves *args:
    cargo run --quiet --release --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli_main -- freeze-saves {{ quote(justfile_directory() / "rust" / "crates" / "beryl_session" / "tests" / "saves") }} "$@"

# A seeded WU Control mirror game between two agents at low priority, as a transcript (--seed N, --out FILE)
[group('agents')]
[no-cd]
play *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli_main -- play "$@"

# An app game's transcript from a dev build in seconds; `--agent recorded` (or NAME) adds its searches from a search build at low priority (REPLAY.json, --turn N, --out FILE)
[group('agents')]
[no-cd]
review *args:
    #!/usr/bin/env bash
    set -euo pipefail
    manifest={{ quote(justfile_directory() / "rust" / "Cargo.toml") }}
    case " $* " in
      *" --agent greedy "*) search=false ;;
      *" --agent "*) search=true ;;
      *) search=false ;;
    esac
    if $search; then
      RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path "$manifest" -p beryl_cli_main -- review "$@"
    else
      cargo run --quiet --manifest-path "$manifest" -p beryl_cli_main -- review "$@"
    fi

# A round robin between agents, games in parallel at low priority (--games N, --seed N, --json FILE, --check)
[group('agents')]
[no-cd]
tournament *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli_main -- tournament "$@"

# A paired match against the champion, at low priority (--vs NAME, --work 3M, --bar stronger|plausible|audit, --cap N, --seed N, --out DIR)
[group('agents')]
[no-cd]
ladder *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli_main -- ladder "$@"

# Fit rollout values to accepted paired overrides from ladder match directories
[group('agents')]
[no-cd]
fit *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli_main -- fit "$@"

# The ladder's start positions, cut from champion games at low priority: build [--games N] [--seed N] [--work 3M] [--out DIR], or sample [--count N] DIR
[group('agents')]
[no-cd]
positions *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli_main -- positions "$@"

# A challenger's differing moves over the champion's last match, re-searched, at low priority (<challenger> <match-dir> --vs NAME, --recheck N, --work 3M, --games N)
[group('agents')]
[no-cd]
screen *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli_main -- screen "$@"

# The blunder suite against an agent, the champion by default, at low priority ([agent] --work 3M,30M, --seeds 5, --only NAME,..., --json FILE)
[group('agents')]
[no-cd]
blunders *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli_main -- blunders "$@"

# Compare paired rollouts and search branches at a blunder-suite position ([--family FAMILY] [--tree --work 3M --seed 0])
[group('agents')]
[no-cd]
why *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli_main -- why "$@"

# Report both blunder suites and their totals without a verdict (<challenger> --claims NAME,...|none [--vs NAME])
[group('agents')]
[no-cd]
gate *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli_main -- gate "$@"

# Discover candidate blunders for later LLM review; six hours by default (--hours 6, --out DIR, --seed N, --inspect CASE.json)
[group('agents')]
[no-cd]
blunder-hunt *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} {{ awake }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli_main -- blunder-hunt "$@"

# The tools' tests
[group('test')]
test-py *args:
    cd tools && uv run --quiet pytest "$@"

# The source lines each browser check runs, at low priority, into the cache (--commit REF, default origin/staging; --here)
[group('checks')]
coverage *args:
    {{ low }} {{ by }} coverage "$@"

# The file-event daemon's memory footprint, and whether the watchdog that restarts it is installed
[group('checks')]
fseventsd:
    {{ by }} fseventsd

# Format everything: black, rustfmt, prettier, mdformat (80 columns), the justfile
[group('checks')]
fmt:
    uv run --quiet --project tools black --quiet tools efficiency
    cd rust && uv run --quiet --project ../tools python -m beryl_tools.card_files rustfmt
    {{ by }} deps
    cd ui && node_modules/.bin/prettier --write --log-level warn .
    just --fmt --unstable
    git ls-files -co --exclude-standard '*.md' | grep -v -e '^.agents/' | xargs uv run --quiet --project tools mdformat
