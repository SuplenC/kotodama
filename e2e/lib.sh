# shellcheck shell=bash
# Helpers for e2e tests. e2e/run sources this before each test; it sets
# E2E_OUT (artifact dir), E2E_UPDATE (1 rewrites references), WAYLAND_DISPLAY and SWAYSOCK.

# Starts a command in the background and kills it when the test exits.
spawn() {
    "$@" >>"$E2E_OUT/$(basename "$1").log" 2>&1 &
    E2E_PIDS+=("$!")
}

E2E_PIDS=()
stop_spawned() {
    local pid
    for pid in "${E2E_PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
}
trap stop_spawned EXIT

# Waits until the output matches a reference image, then succeeds; fails after
# a timeout and leaves the screenshot and a diff in E2E_OUT. With E2E_UPDATE=1,
# waits for the screen to settle and writes the reference instead.
expect_screen() {
    local reference=$1 name shot diff pixels
    name=$(basename "$reference" .png)
    shot="$E2E_OUT/$name.png"
    diff="$E2E_OUT/$name.diff.png"

    if [ "${E2E_UPDATE:-0}" = 1 ]; then
        sleep 3
        grim "$reference"
        echo "updated $reference"
        return 0
    fi

    for _ in $(seq 50); do
        grim "$shot"
        # AE counts pixels that differ by more than the fuzz; 0 is a match.
        pixels=$(compare -metric AE -fuzz 2% "$reference" "$shot" "$diff" 2>&1 >/dev/null || true)
        # ImageMagick 7 prints "<count> (<fraction>)"; keep the count.
        if [ "${pixels%% *}" = 0 ]; then
            return 0
        fi
        sleep 0.2
    done
    echo "screen does not match $reference: $pixels pixels differ (see $shot, $diff)" >&2
    return 1
}
