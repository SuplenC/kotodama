#!/usr/bin/env python3
"""A Kotodama Extension in Python, with the standard library alone.

It answers Live queries: `case hello world` lists the text in each case, and
Enter copies the one picked, or Ctrl+T types it into the window you came
from. Its config table picks the cases it offers, and when none of them is
one it knows, it tells the Daemon it can't do anything until that is fixed.

stdin and stdout carry the protocol, so nothing else may write to stdout:
log to stderr. See README.md beside it, and docs/extensions.md.
"""

import json
import re
import sys

# The protocol version this script is written for. A newer Daemon talks to it
# in 1.0's terms; an older one can't.
VERSION = (1, 0)

STDIN = sys.stdin.buffer
STDOUT = sys.stdout.buffer


def log(message):
    print(f"text-case: {message}", file=sys.stderr, flush=True)


def read_frame():
    """One frame's payload, or None once the Daemon hangs up."""
    header = STDIN.readline()
    if not header:
        return None
    length = int(header)
    payload = STDIN.read(length)
    if len(payload) < length:
        return None
    return payload


def send(message):
    """Sends a message as one frame: its length in bytes, a newline, then the JSON."""
    payload = json.dumps(message, separators=(",", ":")).encode()
    STDOUT.write(b"%d\n" % len(payload) + payload)
    STDOUT.flush()


def words(text):
    """The words of `text`, split at spaces, punctuation and lowercase-to-uppercase steps."""
    text = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", text)
    return [word for word in re.split(r"[^0-9A-Za-z]+", text) if word]


CASES = {
    "upper": ("UPPER", lambda text: text.upper()),
    "lower": ("lower", lambda text: text.lower()),
    "title": ("Title", lambda text: " ".join(w.capitalize() for w in text.split())),
    "snake": ("snake_case", lambda text: "_".join(w.lower() for w in words(text))),
    "kebab": ("kebab-case", lambda text: "-".join(w.lower() for w in words(text))),
    "camel": (
        "camelCase",
        lambda text: "".join(
            w.lower() if i == 0 else w.capitalize() for i, w in enumerate(words(text))
        ),
    ),
}

# Every Result offers both: Enter runs the first, the primary Action.
ACTIONS = [
    {"id": "copy", "title": "Copy"},
    {"id": "type", "title": "Type it", "shortcut": "ctrl+t"},
]


def results(text, cases):
    """The Live results for `text`: one Item per case, best first."""
    found = []
    for rank, case in enumerate(cases):
        name, convert = CASES[case]
        converted = convert(text)
        if not converted:
            continue
        found.append(
            {
                "item": {
                    # The id carries the case and the text, so running an
                    # Action needs nothing remembered.
                    "id": f"{case}:{text}",
                    "kind": "text-case.text",
                    "title": converted,
                    "subtitle": name,
                    "actions": ACTIONS,
                },
                "score": round(1 - rank / len(CASES), 3),
            }
        )
    return found


def run_action(run, cases):
    """The outcome of running Action `run["action"]` on one of our Results."""
    case, _, text = run["item"]["id"].partition(":")
    if case not in CASES:
        return {"failed": {"message": f"no such case: {case}"}}
    converted = CASES[case][1](text)
    if run["action"] == "copy":
        return {"copy": {"text": converted}}
    if run["action"] == "type":
        return {"type": {"text": converted}}
    return {"failed": {"message": f"unknown action {run['action']}"}}


def configured_cases(config):
    """The cases the config table asks for, all of them by default, and the names it gets wrong."""
    asked = config.get("cases", list(CASES))
    if not isinstance(asked, list):
        return [], ["`cases` should be a list"]
    known = [case for case in asked if case in CASES]
    unknown = [str(case) for case in asked if case not in CASES]
    return known, unknown


def main():
    # The handshake is always JSON: both sides send a hello at once. Offering
    # JSON alone settles the Wire format on JSON.
    send({"version": VERSION[0], "minor": VERSION[1], "formats": ["json"]})
    hello = read_frame()
    if hello is None:
        return 0
    daemon = json.loads(hello)
    if (daemon["version"], daemon.get("minor", 0)) < VERSION:
        log(f"this Daemon speaks protocol {daemon['version']}.{daemon.get('minor', 0)}; update Kotodama")
        return 1

    cases = list(CASES)
    while (frame := read_frame()) is not None:
        message = json.loads(frame)
        # A message is {"<name>": {...}}, or just "<name>" without fields.
        name, body = next(iter(message.items())) if isinstance(message, dict) else (message, None)

        if name == "config":
            # Our [extensions.text-case] table, sent first and on every change.
            cases, unknown = configured_cases(body)
            if unknown:
                log(f"unknown cases in the config: {', '.join(unknown)}")
            if cases:
                send({"status": {"usable": True}})
            else:
                note = f"Pick cases in [extensions.text-case] cases, from {', '.join(CASES)}"
                send({"status": {"usable": False, "note": note}})
        elif name == "live_query":
            send(
                {
                    "live_results": {
                        "live_query": body["id"],
                        "results": results(body["query"], cases),
                        "done": True,
                    }
                }
            )
        elif name == "run_action":
            outcome = run_action(body, cases)
            send({"action_done": {"invocation": body["invocation"], "outcome": outcome}})
        elif name == "rejected":
            # The Daemon refused something we sent, and says why.
            log(f"the Daemon rejected a message: {body['reason']}")
        # Anything else is ignored, so a newer Daemon can send more.
    return 0


if __name__ == "__main__":
    sys.exit(main())
