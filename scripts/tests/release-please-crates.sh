#!/usr/bin/env bash
# Checks that every workspace crate is a release-please package of its own: a
# package in release-please-config.json whose Cargo.lock entry it bumps, and a
# version in .release-please-manifest.json.
#
# A release line (docs/releasing.md) is the exception. Its branch has a
# .release-line file and releases only its own packages, so there the check is
# that each package in the manifest is a workspace member with a package in the
# config, and that the config has no other.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
config=${RELEASE_PLEASE_CONFIG:-$root/release-please-config.json}
manifest=${RELEASE_PLEASE_MANIFEST:-$root/.release-please-manifest.json}
failed=0

# The workspace members' directories, so the check needs no jq.
members=$(sed -n 's/^members = \[\(.*\)\]/\1/p' "$root/Cargo.toml" | tr -d '" ' | tr ',' '\n')

if [ -z "$members" ]; then
    echo "FAIL no workspace members found in $root/Cargo.toml" >&2
    exit 1
fi

if [ -f "$root/.release-line" ]; then
    line=$(cat "$root/.release-line")
    packages=$(sed -n 's/^ *"\(crates\/[^"]*\)": .*/\1/p' "$manifest")
    configured=$(sed -n 's/^ *"\(crates\/[^"]*\)": {.*/\1/p' "$config")
    if [ -z "$packages" ]; then
        echo "FAIL release line $line: .release-please-manifest.json lists no package"
        exit 1
    fi
    for dir in $packages; do
        if ! grep -qx "$dir" <<<"$members"; then
            echo "FAIL release line $line: $dir is in .release-please-manifest.json but no workspace member"
            failed=1
        elif ! grep -qx "$dir" <<<"$configured"; then
            echo "FAIL release line $line: $dir is in .release-please-manifest.json but no package in release-please-config.json"
            failed=1
        else
            echo "ok   release line $line releases $dir"
        fi
    done
    for dir in $configured; do
        if ! grep -qx "$dir" <<<"$packages"; then
            echo "FAIL release line $line: $dir is a package in release-please-config.json but not in .release-please-manifest.json"
            failed=1
        fi
    done
    exit $failed
fi

for dir in $members; do
    crate=$(sed -n 's/^name = "\(.*\)"/\1/p' "$root/$dir/Cargo.toml" | head -1)
    # The package's block: its key, then its extra-files.
    block=$(grep -F "\"$dir\": {" -A3 "$config" || true)
    if [ -z "$block" ]; then
        echo "FAIL $crate ($dir) is a workspace member but no package in release-please-config.json"
        failed=1
    elif ! grep -qF "\"path\": \"/Cargo.lock\", \"jsonpath\": \"\$.package[?(@.name.value == '$crate')].version\"" <<<"$block"; then
        echo "FAIL $crate's package in release-please-config.json doesn't bump its Cargo.lock entry"
        failed=1
    elif ! grep -qF "\"$dir\": " "$manifest"; then
        echo "FAIL $crate ($dir) has no version in .release-please-manifest.json"
        failed=1
    elif grep -qx 'version.workspace = true' "$root/$dir/Cargo.toml"; then
        echo "FAIL $crate takes the workspace's version instead of its own"
        failed=1
    else
        echo "ok   $crate is released on its own"
    fi
done

exit $failed
