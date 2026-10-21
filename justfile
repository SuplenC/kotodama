# Every public recipe runs in the dev environment (see scripts/dev-env), so the
# same command works on Linux with Nix, inside `just shell`, and on macOS through
# Docker or OrbStack. The private `_` recipes are what runs inside it.

set positional-arguments
set shell := ["bash", "-euo", "pipefail", "-c"]

dev := justfile_directory() / "scripts" / "dev-env"

# List the recipes
default:
    @just --list

# Open a shell in the dev environment
shell:
    @{{ dev }} bash

# Check formatting, lints and comments
check:
    @{{ dev }} just _check

# Format the restyled crates (scripts/restyled-crates) with the nightly rustfmt; the others are left alone
fmt:
    @{{ dev }} just _fmt

# Check the comment rules of docs/agents/code-quality.md in the restyled crates
comments:
    @{{ dev }} just _comments

# Run the tests; arguments go to `cargo nextest run`
test *args:
    @{{ dev }} just _test "$@"

# Run the benchmarks; arguments go to `cargo bench`
bench *args:
    @{{ dev }} just _bench "$@"

# Run the e2e tests against headless sway, or only the named ones; `--update` rewrites the reference images
e2e *args:
    @{{ dev }} just _e2e "$@"

# Run Kotodama on a headless sway desktop served over VNC on localhost:5900, to try it by hand; `--keep` keeps its state between runs
demo *args:
    @scripts/demo "$@"

# Rebuild the dev container image locally (machines without Nix only)
image:
    docker build -t "$(scripts/dev-image)" docker

[private]
_check:
    git ls-files '*.nix' | xargs nixfmt --check
    git ls-files scripts e2e/run '*.sh' | xargs shellcheck
    just --fmt --check --unstable
    actionlint
    # Stable rustfmt's defaults for most crates, the nightly rustfmt for the restyled ones.
    if scripts/has-crates; then scripts/fmt --check; fi
    just _comments
    # The restyled crates opt into [workspace.lints], so this lints them harder.
    # Two passes, as tests and benches may unwrap: clippy's `allow-unwrap-in-tests`
    # spares only `#[test]` functions and `#[cfg(test)]` modules, not an
    # integration test's helpers or a benchmark, and a target cannot carry lints
    # of its own. The second pass rechecks the libraries the tests build on, which
    # the first has already linted.
    if scripts/has-crates; then cargo clippy --workspace --lib --bins --examples -- -D warnings; fi
    if scripts/has-crates; then cargo clippy --workspace --tests --benches -- -D warnings -A clippy::unwrap_used; fi

[private]
_fmt:
    if scripts/has-crates; then scripts/fmt; fi

[private]
_comments:
    scripts/restyled-crates | sed 's|^|crates/|' | xargs -r scripts/comments

[private]
_test *args:
    for test in scripts/tests/*.sh; do "$test"; done
    # nextest runs each test in its own process, all in parallel; it skips doc tests.
    if scripts/has-crates; then cargo nextest run --workspace --no-tests=pass ${CI:+--profile ci} "$@"; fi
    if scripts/has-crates; then cargo test --workspace --doc; fi

[private]
_bench *args:
    if scripts/has-crates; then cargo bench --workspace "$@"; fi

[private]
_e2e *args:
    e2e/run "$@"
