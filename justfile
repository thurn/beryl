# Beryl's commands; `just` lists them. Most wrap `by` (tools/), and extra arguments pass
# straight through: `just smoke --seconds 30`.

set positional-arguments := true

by := "uv run --quiet --project tools by"

# The CPU the `search` profile builds for: a Cargo profile cannot set it (architecture §18)

search_cpu := "-C target-cpu=apple-m4"

default:
    @just --list --unsorted

# The app with hot reload, in a window (humans; agents use `smoke`)
[group('app')]
dev *args:
    {{ by }} dev "$@"

# Build the debug app and launch it headless until its UI loads (--seconds N, --no-build)
[group('app')]
smoke *args:
    {{ by }} smoke "$@"

# Screenshot UI states headless into the cache and print the paths (shot names; ui/shots.json)
[group('app')]
ui-shots *args:
    {{ by }} ui-shots "$@"

# Play the game screen headless through a recorded fixture: drag, click, hover
[group('app')]
ui-test:
    {{ by }} ui-test

# Live games in a headless page an agent drives over HTTP, until interrupted (--release, --port N)
[group('app')]
ui-live *args:
    {{ by }} ui-live "$@"

# The release Beryl.app, under rust/target/tauri/release/bundle/macos
[group('app')]
bundle:
    {{ by }} bundle

# Restore rust/target and ui/node_modules from the cache (every command does this too)
[group('app')]
warm *args:
    {{ by }} warm "$@"

# Time benchmarks under the search profile, refusing a busy, unplugged or throttled machine (cargo bench flags; --force)
[group('bench')]
bench *args:
    {{ by }} bench "$@"

# Gungraun instruction counts in the Lima VM (cargo bench flags; default --bench '*_counts')
[group('bench')]
bench-count *args:
    {{ by }} bench-count "$@"

# Look up card data on Scryfall into data/cards.json (--cube for the whole cube)
[group('data')]
cards *args:
    {{ by }} cards "$@"

# 17lands games-in-hand win rates into data/card-quality.csv
[group('data')]
card-quality *args:
    {{ by }} card-quality "$@"

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

# The Rust tests (cargo test flags, e.g. -p beryl_app)
[group('test')]
test-rust *args:
    cd rust && cargo test "$@"

# Random-game fuzzing at length: N games, optimized but with debug assertions on
[group('test')]
fuzz games="10000":
    cd rust && BERYL_FUZZ_GAMES="$1" CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true cargo test --release -p beryl_search --test games -- --nocapture

# A seeded WU Control mirror game between two agents at low priority, as a transcript (--seed N, --out FILE)
[group('agents')]
[no-cd]
play *args:
    RUSTFLAGS="{{ search_cpu }}" nice -n 10 cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli -- play "$@"

# A round robin between agents, games in parallel at low priority (--games N, --seed N, --json FILE, --check)
[group('agents')]
[no-cd]
tournament *args:
    RUSTFLAGS="{{ search_cpu }}" nice -n 10 cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli -- tournament "$@"

# The long-think arena: starts or resumes the latest run, low priority, kept awake (--status, --new, --run DIR, --upgrade, --add FAMILY, --games N)
[group('agents')]
[no-cd]
longthink *args:
    RUSTFLAGS="{{ search_cpu }}" caffeinate -i nice -n 10 cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli -- longthink "$@"

# The tools' tests
[group('test')]
test-py *args:
    cd tools && uv run --quiet pytest "$@"

# The fast check `enact push` runs (lanes for what changed, budget 60 s)
[group('checks')]
check:
    {{ by }} check

# Slow CI, or some of its lanes: repo, python, rust, ui, app, shots, oracle (budget 300 s)
[group('checks')]
ci *lanes:
    {{ by }} ci "$@"

# Format everything: black, rustfmt, prettier, mdformat (80 columns), the justfile
[group('checks')]
fmt:
    uv run --quiet --project tools black --quiet tools
    cd rust && cargo fmt --all
    {{ by }} warm
    cd ui && node_modules/.bin/prettier --write --log-level warn .
    just --fmt --unstable
    git ls-files -co --exclude-standard '*.md' | grep -v -e '^.beads/' -e '^.agents/' | xargs uv run --quiet --project tools mdformat

# Edit one file in $EDITOR in a fresh worktree, then commit and `enact push` it
[group('enact')]
[no-cd]
edit file:
    @uv run --quiet --project {{ quote(justfile_directory() / "tools") }} by edit "$1"
