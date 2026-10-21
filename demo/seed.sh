#!/usr/bin/env bash
# Fills the demo's home with things for the folders Extension to find: a few
# git projects (one with a worktree, two that share a name) and a scripts
# folder that shows each header option. scripts/demo runs it once per fresh
# home; it changes nothing outside the directory it is given.
# Usage: demo/seed.sh HOME
set -euo pipefail

home=$1

# A repository with one commit, so lazygit and worktrees have history to show.
repo() {
    local dir=$1 about=$2
    mkdir -p "$dir"
    printf '# %s\n\n%s\n' "$(basename "$dir")" "$about" >"$dir/README.md"
    git -C "$dir" init --quiet --initial-branch=main
    git -C "$dir" add README.md
    git -C "$dir" -c user.name=Demo -c user.email=demo@example.com \
        commit --quiet -m "Start $(basename "$dir")"
}

projects="$home/Projects"
repo "$projects/aurora" "A weather dashboard."
# A linked worktree: it shows under its repository, as aurora#feature-x.
git -C "$projects/aurora" worktree add --quiet -b feature-x "$projects/aurora-feature-x"
repo "$projects/nimbus" "A cloud-cost report."
repo "$projects/dotfiles" "Configuration for the demo desktop."
# Two projects with one name: their parent folders tell them apart.
repo "$projects/work/api" "The work API."
repo "$projects/home/api" "A home-automation API."

scripts="$home/Scripts"
mkdir -p "$scripts/work"
cat >"$scripts/deploy-staging.sh" <<'SH'
#!/bin/sh
# kotodama: name=Deploy staging
echo "Deploying aurora to staging..."
sleep 1
echo "Done: https://staging.example.com"
SH
cat >"$scripts/backup.sh" <<'SH'
#!/bin/sh
# kotodama: terminal=false
# Runs in the background as a job; its last line is what the job reports.
sleep 3
echo "Backed up 42 files"
SH
cat >"$scripts/greet.sh" <<'SH'
#!/bin/sh
# kotodama: args=prompt
echo "Hello, ${*:-world}!"
SH
cat >"$scripts/work/cleanup.sh" <<'SH'
#!/bin/sh
echo "Cleaning up build folders..."
SH
chmod +x "$scripts"/*.sh "$scripts"/work/*.sh
