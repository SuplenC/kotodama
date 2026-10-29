#!/usr/bin/env bash
# Tests for scripts/bench-compare, through its command line only.
set -euo pipefail

compare=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/bench-compare
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
failed=0

if [ ! -x "$compare" ]; then
    echo "FAIL $compare is missing" >&2
    exit 1
fi

bench() { printf 'test %s ... bench:  %s ns/iter (+/- 3)\n' "$1" "$2"; }
memory() { printf 'test %s ... bench: %s bytes (+/- 0)\n' "$1" "$2"; }

# expect <exit code> <description> <baseline> <current> <threshold> [text the output must contain] [unit] [floor]
expect() {
    local want=$1 what=$2 got=0
    printf '%s\n' "$3" >"$dir/baseline"
    printf '%s\n' "$4" >"$dir/current"
    "$compare" "$dir/baseline" "$dir/current" "$5" ${7:+"$7"} ${8:+"$8"} >"$dir/out" 2>&1 || got=$?
    if [ "$got" = "$want" ] && { [ -z "${6:-}" ] || grep -qF -- "$6" "$dir/out"; }; then
        echo "ok   $what"
    else
        echo "FAIL $what: exit $got, want $want"
        sed 's/^/     /' "$dir/out"
        failed=1
    fi
}

expect 1 "a benchmark twice as slow fails at 50%" "$(bench parse 1,000)" "$(bench parse 2,000)" 50 "parse"
expect 0 "a benchmark 20% slower passes at 50%" "$(bench parse 1,000)" "$(bench parse 1,200)" 50
expect 0 "a benchmark missing from the baseline passes" "$(bench parse 1,000)" "$(bench parse 1,000; bench rank 9,000)" 50
expect 1 "memory 11% higher fails at 10% when comparing bytes" "$(memory rss 1,000,000)" "$(memory rss 1,110,000)" 10 "rss" bytes
expect 0 "memory 9% higher passes at 10%" "$(memory rss 1,000,000)" "$(memory rss 1,090,000)" 10 "" bytes
expect 0 "timings are left out when comparing bytes" "$(bench parse 1,000; memory rss 100)" "$(bench parse 9,000; memory rss 100)" 10 "" bytes
expect 0 "memory is left out when comparing timings" "$(bench parse 1,000; memory rss 100)" "$(bench parse 1,000; memory rss 900)" 50
expect 0 "a few nanoseconds more pass under a 100 ns floor" "$(bench query/encode/messagepack 12)" "$(bench query/encode/messagepack 27)" 50 "" ns/iter 100
expect 1 "a timing over the floor and the threshold fails" "$(bench query/encode/messagepack 12)" "$(bench query/encode/messagepack 200)" 50 "messagepack" ns/iter 100
expect 1 "a large timing twice as slow fails under the floor" "$(bench parse 1,000)" "$(bench parse 2,000)" 50 "parse" ns/iter 100
expect 0 "a large timing 20% slower passes under the floor" "$(bench parse 1,000)" "$(bench parse 1,200)" 50 "" ns/iter 100

got=0
"$compare" "$dir/missing" "$dir/current" 50 >"$dir/out" 2>&1 || got=$?
if [ "$got" = 0 ] && grep -qF "no baseline" "$dir/out"; then
    echo "ok   a missing baseline file passes and says so"
else
    echo "FAIL a missing baseline file passes and says so: exit $got"
    sed 's/^/     /' "$dir/out"
    failed=1
fi

exit "$failed"
