#!/usr/bin/env bash
# Checks that release-please bumps every workspace crate's Cargo.lock entry.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
config=${RELEASE_PLEASE_CONFIG:-$root/release-please-config.json}
failed=0

# The workspace members' package names, read from each member's Cargo.toml,
# so the check needs no jq.
members=$(sed -n 's/^members = \[\(.*\)\]/\1/p' "$root/Cargo.toml" | tr -d '" ' | tr ',' '\n' |
    while read -r dir; do sed -n 's/^name = "\(.*\)"/\1/p' "$root/$dir/Cargo.toml" | head -1; done)
lock_path=$(grep -F '"Cargo.lock"' -A1 "$config" | grep -F jsonpath)

if [ -z "$members" ]; then
    echo "FAIL no workspace members found in $root/Cargo.toml" >&2
    exit 1
fi

for crate in $members; do
    if grep -qF "@.name.value == '$crate'" <<<"$lock_path"; then
        echo "ok   $crate is bumped in Cargo.lock"
    else
        echo "FAIL $crate is a workspace member missing from release-please-config.json's Cargo.lock extra-files"
        failed=1
    fi
done

exit $failed
