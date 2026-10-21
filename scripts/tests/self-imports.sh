#!/usr/bin/env bash
# Tests for scripts/self-imports, through its command line only.
set -euo pipefail

script=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/self-imports
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
failed=0

if [ ! -x "$script" ]; then
    echo "FAIL $script is missing" >&2
    exit 1
fi

# rewrites <description> <Rust source> <the source rewritten>
rewrites() {
    local what=$1
    printf '%s\n' "$2" >"$dir/lib.rs"
    printf '%s\n' "$3" >"$dir/want.rs"
    "$script" "$dir/lib.rs"
    if cmp -s "$dir/lib.rs" "$dir/want.rs"; then
        echo "ok   $what"
    else
        echo "FAIL $what:"
        diff "$dir/want.rs" "$dir/lib.rs" | sed 's/^/     /'
        failed=1
    fi
}

# checks <exit code> <description> <Rust source> [text the output must contain]
checks() {
    local want=$1 what=$2 got=0
    printf '%s\n' "$3" >"$dir/lib.rs"
    "$script" --check "$dir" >"$dir/out" 2>&1 || got=$?
    if [ "$got" = "$want" ] && { [ -z "${4:-}" ] || grep -qF -- "$4" "$dir/out"; }; then
        echo "ok   $what"
    else
        echo "FAIL $what: exit $got, want $want"
        sed 's/^/     /' "$dir/out"
        failed=1
    fi
}

rewrites "{self} becomes the module" 'use std::io::Read;
use std::io::{self};' 'use std::io::Read;
use std::io;'
rewrites "{self as name} becomes a rename" 'use std::fmt::{self as f};' 'use std::fmt as f;'
rewrites "a pub use is rewritten" 'pub use crate::binds::{self};' 'pub use crate::binds;'
rewrites "a pub(crate) use is rewritten" '    pub(crate) use super::windows::{self};' '    pub(crate) use super::windows;'
rewrites "other imports are left alone" 'use std::io;
use std::io::Write;
use kotodama_protocol::*;
let s = "use a::{self};";' 'use std::io;
use std::io::Write;
use kotodama_protocol::*;
let s = "use a::{self};";'

checks 0 "a plain module import passes" 'use std::io;'
checks 1 "{self} fails" 'use std::io;
use std::io::{self};' "lib.rs:2: write \`use foo;\`"
checks 1 "{self as name} fails" 'use std::fmt::{self as f};' "lib.rs:1:"

exit "$failed"
