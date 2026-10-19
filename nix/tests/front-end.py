"""A bare Front end for the NixOS VM test, speaking JSON over the Daemon's socket.

    front-end.py QUERY          prints the Results, one `extension<TAB>id<TAB>title` per line
    front-end.py --run QUERY    also runs the top Result's primary Action
"""

import json
import os
import socket
import sys


def send(sock, message):
    payload = json.dumps(message).encode()
    sock.sendall(b"%d\n" % len(payload) + payload)


def recv(reader):
    header = reader.readline()
    if not header:
        sys.exit("front-end: the Daemon hung up")
    return json.loads(reader.read(int(header)))


def main():
    args = sys.argv[1:]
    run = args[:1] == ["--run"]
    query = " ".join(args[1:] if run else args)

    sock = socket.socket(socket.AF_UNIX)
    sock.settimeout(10)
    sock.connect(os.path.join(os.environ["XDG_RUNTIME_DIR"], "kotodama.sock"))
    reader = sock.makefile("rb")
    # The handshake: this Front end speaks JSON only.
    send(sock, {"version": 1, "formats": ["json"]})
    recv(reader)

    send(sock, {"query": {"text": query}})
    while True:
        message = recv(reader)
        if isinstance(message, dict) and "results" in message:
            results = message["results"]["results"]
            break
    for result in results:
        print("%s\t%s\t%s" % (result["extension"], result["item"]["id"], result["item"]["title"]))
    if not run:
        return

    if not results:
        sys.exit("front-end: nothing to run")
    top = results[0]
    send(
        sock,
        {
            "run_action": {
                "result": {"extension": top["extension"], "id": top["item"]["id"]},
                "action": {"extension": top["extension"], "id": top["item"]["actions"][0]["id"]},
            }
        },
    )
    while True:
        message = recv(reader)
        if message == "close":
            send(sock, "closed")
            return
        if isinstance(message, dict) and "action_failed" in message:
            sys.exit("front-end: " + message["action_failed"]["message"])


main()
