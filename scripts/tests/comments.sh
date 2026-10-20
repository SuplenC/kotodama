#!/usr/bin/env bash
# Tests for scripts/comments, through its command line only.
set -euo pipefail

comments=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/comments
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
failed=0

if [ ! -x "$comments" ]; then
    echo "FAIL $comments is missing" >&2
    exit 1
fi

# expect <exit code> <description> <Rust source> [text the output must contain]
expect() {
    local want=$1 what=$2 got=0
    printf '%s\n' "$3" >"$dir/lib.rs"
    "$comments" "$dir" >"$dir/out" 2>&1 || got=$?
    if [ "$got" = "$want" ] && { [ -z "${4:-}" ] || grep -qF -- "$4" "$dir/out"; }; then
        echo "ok   $what"
    else
        echo "FAIL $what: exit $got, want $want"
        sed 's/^/     /' "$dir/out"
        failed=1
    fi
}

expect 0 "a comment above the code passes" '// The watcher re-arms on the new target.
fn watch() {}'
expect 0 "doc comments pass" '//! The crate.

/// Reads the config.
fn read() {}'
expect 0 "a TODO with an issue passes" '// TODO(#77): drop the copy; it waits on the upstream fix.'
expect 1 "a TODO without an issue fails" '// TODO: drop the copy.' "lib.rs:1: a TODO is written TODO(#n):"
expect 1 "a TODO with an issue but no colon fails" '// TODO(#77) drop the copy.' "TODO(#n):"
expect 0 "the word TODOs passes" '// The PR that closes an issue removes its TODOs.'
expect 1 "FIXME fails" '/// FIXME: wrong for empty input.' "FIXME and XXX"
expect 1 "XXX fails" '// XXX' "FIXME and XXX"
expect 1 "a trailing comment fails" 'let x = 1; // the count' "lib.rs:1: a comment goes above the code"
expect 1 "a label on a closing brace fails" 'fn f() {
} // fn f' "lib.rs:2: no labels on closing braces"
expect 1 "a block comment fails" '/* The index. */' "never /* */"
expect 1 "a block doc comment fails" '/** The index. */' "never /* */"
expect 1 "narration fails" '// Now uses the cache.' "narrating a change"
expect 1 "previously fails" '/// Previously this was a Vec.' "narrating a change"
expect 1 "no longer fails" '// The lock is no longer held here.' "narrating a change"
expect 0 "a word containing now passes" '// The known Items, nowhere else.'
# shellcheck disable=SC2016 # Backticks in Rust source, not a command substitution.
expect 0 "now in backticks passes" '// The clock reads `Instant::now` once.'
expect 0 "a URL passes" '// https://example.com/now/xxx'
expect 0 "// in a string is not a comment" 'let url = "https://example.com"; let x = 1;'
expect 0 "// in a raw string is not a comment" 'let s = r#"a "quoted" // b"#; let x = 1;'
expect 0 "// in a string over two lines is not a comment" 'let s = "first
second // third";'
expect 0 "a block comment marker in a string passes" 'let glob = "src/*.rs";'
expect 0 "a quote char literal does not open a string" "let q = '\"'; let x = 1;"
expect 0 "an escaped quote char literal does not open a string" "let q = '\\''; let s = \"a // b\";"
expect 0 "a lifetime is not a char literal" "fn f<'a>(s: &'a str) -> &'a str { s }
// A comment."
expect 1 "a comment after a string fails" 'let s = "a"; // b' "goes above the code"
expect 0 "a TODO in a string is not a comment" 'let s = "TODO: FIXME";'

if [ "$failed" -ne 0 ]; then
    exit 1
fi
