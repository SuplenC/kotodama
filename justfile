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

# Check formatting and lints
check:
    @{{ dev }} just _check

# Run the tests; arguments go to `cargo nextest run`
test *args:
    @{{ dev }} just _test "$@"

# Run the benchmarks; arguments go to `cargo bench`
bench *args:
    @{{ dev }} just _bench "$@"

# Run the e2e tests against headless sway, or only the named ones; `--update` rewrites the reference images
e2e *args:
    @{{ dev }} just _e2e "$@"

# Rebuild the dev container image locally (machines without Nix only)
image:
    docker build -t "$(scripts/dev-image)" docker

[private]
_check:
    nixfmt --check flake.nix
    git ls-files scripts e2e/run '*.sh' | xargs shellcheck
    just --fmt --check --unstable
    actionlint
    if scripts/has-crates; then cargo fmt --all --check; fi
    if scripts/has-crates; then cargo clippy --workspace --all-targets -- -D warnings; fi

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
