"""Lets macOS Screen Sharing into the demo's VNC server.

Screen Sharing speaks RFB 3.3 to a server it doesn't know, where the only
login is the classic VNC password, and it refuses a server without one.
wayvnc has no classic VNC password, by design. So wayvnc runs without a
login, on a port of its own, and this greets the viewer in its place: it asks
for a password, takes any, then passes the session through untouched. The
port is published to localhost only, so the password keeps nothing out.

Usage: vnc-proxy.py <address> <port> <wayvnc port>
"""

import os
import socket
import sys
import threading

# RFB 3.3: the server names the one security type, then (for VNC
# authentication) sends a 16-byte challenge and answers with a result.
VERSION = b"RFB 003.003\n"
NO_AUTH = (1).to_bytes(4, "big")
VNC_AUTH = (2).to_bytes(4, "big")
AUTH_OK = (0).to_bytes(4, "big")


def receive(sock, n):
    data = b""
    while len(data) < n:
        chunk = sock.recv(n - len(data))
        if not chunk:
            raise ConnectionError("closed during the handshake")
        data += chunk
    return data


def pipe(source, sink):
    try:
        while chunk := source.recv(65536):
            sink.sendall(chunk)
    except OSError:
        pass
    for sock in (source, sink):
        try:
            sock.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass


def serve(viewer, upstream_port):
    try:
        server = socket.create_connection(("127.0.0.1", upstream_port))
        receive(server, len(VERSION))
        server.sendall(VERSION)
        if receive(server, 4) != NO_AUTH:
            raise ConnectionError("wayvnc asks for a login")
        viewer.sendall(VERSION)
        receive(viewer, len(VERSION))
        viewer.sendall(VNC_AUTH + os.urandom(16))
        receive(viewer, 16)
        viewer.sendall(AUTH_OK)
    except OSError as error:
        print(f"vnc-proxy: {error}", file=sys.stderr, flush=True)
        viewer.close()
        return
    threading.Thread(target=pipe, args=(viewer, server), daemon=True).start()
    pipe(server, viewer)


def main():
    address, port, upstream_port = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    listener = socket.create_server((address, port))
    while True:
        viewer, _ = listener.accept()
        threading.Thread(target=serve, args=(viewer, upstream_port), daemon=True).start()


if __name__ == "__main__":
    main()
