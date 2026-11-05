# Beryl's commands; `just` lists them. Most wrap `by` (tools/), and extra arguments pass
# straight through: `just smoke --seconds 30`.

set positional-arguments := true

by := "uv run --quiet --project tools by"

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

# The release Beryl.app, under rust/target/tauri/release/bundle/macos
[group('app')]
bundle:
    {{ by }} bundle

# Restore rust/target and ui/node_modules from the cache (every command does this too)
[group('app')]
warm *args:
    {{ by }} warm "$@"

# Time benchmarks, refusing a busy, unplugged or throttled machine (cargo bench flags; --force)
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

# Install Maven and JDK 21, fetch XMage and Forge at their pins and build them, in the cache
[group('oracles')]
oracle-setup:
    {{ by }} oracle-setup

# Run one XMage test class and one Forge sim game (xmage or forge for just one)
[group('oracles')]
oracle-smoke *args:
    {{ by }} oracle-smoke "$@"

# The Rust tests (cargo test flags, e.g. -p beryl_app)
[group('test')]
test-rust *args:
    cd rust && cargo test "$@"

# The tools' tests
[group('test')]
test-py *args:
    cd tools && uv run --quiet pytest "$@"

# The fast check `enact push` runs (lanes for what changed, budget 60 s)
[group('checks')]
check:
    {{ by }} check

# Slow CI, or some of its lanes: repo, python, rust, ui, app (budget 300 s)
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
