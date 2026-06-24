#!/usr/bin/env python3
"""WebSocket client test for Boyia WebSocketServer (ws://127.0.0.1:8080/).

Usage:
  1. Start server:  cargo run -p boyia_cli -- test/ws_server.boyia
  2. Run client:    python3 test/ws_client.py

Requires: pip install websocket-client
"""

from __future__ import annotations

import argparse
import sys

try:
    from websocket import WebSocketException, WebSocketTimeoutException, create_connection
except ImportError:
    print("Missing dependency. Install with: pip install websocket-client", file=sys.stderr)
    sys.exit(1)

DEFAULT_URL = "ws://127.0.0.1:8080/"
DEFAULT_MESSAGE = "hello from ws_client.py"


def main() -> int:
    parser = argparse.ArgumentParser(description="Connect to Boyia WebSocketServer")
    parser.add_argument("--url", default=DEFAULT_URL, help=f"WebSocket URL (default: {DEFAULT_URL})")
    parser.add_argument("--message", "-m", default=DEFAULT_MESSAGE, help="Text frame to send")
    parser.add_argument("--recv-timeout", type=float, default=3.0, help="Seconds to wait for a reply")
    args = parser.parse_args()

    print(f"Connecting to {args.url} ...")
    try:
        ws = create_connection(args.url, timeout=5)
    except OSError as err:
        print(f"Connection failed: {err}", file=sys.stderr)
        print("Is the server running?  cargo run -p boyia_cli -- test/ws_server.boyia", file=sys.stderr)
        return 1
    except WebSocketException as err:
        print(f"WebSocket error: {err}", file=sys.stderr)
        return 1

    try:
        ws.send(args.message)
        print(f"Sent: {args.message}")

        ws.settimeout(args.recv_timeout)
        try:
            reply = ws.recv()
            if isinstance(reply, bytes):
                reply = reply.decode("utf-8", errors="replace")
            print(f"Received: {reply}")
        except WebSocketTimeoutException:
            print(f"(no reply within {args.recv_timeout}s)")
    finally:
        ws.close()
        print("Closed.")

    return 0


if __name__ == "__main__":
    sys.exit(main())
