#!/usr/bin/env bash
# Runs the given command inside the flake's dev shell. The repo is mounted at /work.
set -euo pipefail
cd /work
if [ "$#" -eq 0 ]; then
    set -- bash
fi
exec nix develop /work --command "$@"
