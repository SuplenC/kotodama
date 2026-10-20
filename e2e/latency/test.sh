# shellcheck shell=bash
# With 10k Indexed items, the time from a keystroke's query to the list model
# showing its Results stays within one frame at 120 Hz (8 ms). The module
# measures it and logs it with KOTODAMA_LOG=debug.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --release --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama --bin kotodamad --bin kotodama
imports=$("$root/scripts/qml-module" --release)

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state"

ext="$XDG_CONFIG_HOME/kotodama/extensions/many"
mkdir -p "$ext"
cp "$root/crates/kotodamad/tests/fixtures/recorder/extension.sh" "$ext/"
printf 'name = "many"\ncommand = ["extension.sh"]\n' >"$ext/manifest.toml"
words=(firefox files terminal settings music photos editor mail calendar notes)
jq -cn --argjson words "$(printf '%s\n' "${words[@]}" | jq -R . | jq -cs .)" '
  [range(10000) as $n | {
    id: "item-\($n)", kind: "url", url: "https://\($n)",
    title: "\($words[$n % 10] | ascii_upcase[0:1] + .[1:]) \($words[($n / 10 | floor) % 10]) \($n)",
    subtitle: "Item \($n)",
    actions: [{id: "go", title: "Go"}]
  }]' >"$ext/items.json"

spawn "$target/release/kotodamad"
KOTODAMA_LOG=debug QML_IMPORT_PATH=$imports spawn quickshell --path shell.qml
wait_for_log quickshell.log "connected: true"

kotodama="$target/release/kotodama"
mark=$(log_lines quickshell.log)
"$kotodama" open
wait_for_log quickshell.log "focused: true" "$mark"
# The Extension sends the 10k Items in one message, so they are all in once
# "9999" finds any. The search logs only when its Results change, and opening
# with the query already in the box changes nothing, so each try first types
# another query; otherwise each try waits out wait_for_log's 10 s.
indexed=false
for _ in $(seq 50); do
    "$kotodama" open --query 999
    mark=$(log_lines quickshell.log)
    "$kotodama" open --query 9999
    wait_for_log quickshell.log "results: " "$mark" 2>/dev/null || true
    if tail -n "+$((mark + 1))" "$E2E_OUT/quickshell.log" | grep -q 'results: [1-9]'; then
        indexed=true
        break
    fi
    sleep 0.1
done
if [ "$indexed" != true ]; then
    echo "the Items were never indexed" >&2
    exit 1
fi

mark=$(log_lines quickshell.log)
# One key every 50 ms, as a quick typist would.
for words in "firefox music" "mail" "settings" "terminal 42"; do
    "$kotodama" open
    wtype -s 200 -d 50 "$words" -s 200
done

latencies=$(tail -n "+$((mark + 1))" "$E2E_OUT/quickshell.log" | sed -n 's/.*query to model update: \([0-9]*\) µs.*/\1/p')
count=$(wc -l <<<"$latencies")
worst=$(sort -n <<<"$latencies" | tail -n 1)
echo "$count updates, worst $worst µs: $(tr '\n' ' ' <<<"$latencies")" >"$E2E_OUT/latency.txt"
cat "$E2E_OUT/latency.txt" >&2
[ "$count" -ge 20 ]
[ "$worst" -lt 8000 ]
