#!/usr/bin/env python3
"""Forward UI requests to the real tracker server with one-shot fault controls."""

import argparse
import http.client
import json
import os
from pathlib import Path
import socket
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit


def atomic_json(path, value):
    temporary = path.with_suffix(f".{threading.get_ident()}.tmp")
    temporary.write_text(json.dumps(value), encoding="utf-8")
    os.replace(temporary, path)


class ProxyState:
    def __init__(self, directory, upstream):
        self.directory = directory
        self.upstream = urlsplit(upstream)
        self.lock = threading.Lock()
        self.requests = []
        self.consumed = set()
        self.next_id = 0

    def control(self):
        try:
            return json.loads((self.directory / "proxy-control.json").read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            return {}

    def begin(self, method, path, body):
        with self.lock:
            self.next_id += 1
            request = {"id": self.next_id, "method": method, "path": path,
                       "body": body.decode("utf-8", errors="replace"), "status": None}
            self.requests.append(request)
            self.save()
            control = self.control()
            for rule in control.get("rules", [control]):
                token = rule.get("token")
                if not token or token in self.consumed:
                    continue
                if (rule.get("method") == method
                        and rule.get("path") in path):
                    self.consumed.add(token)
                    return request, rule
                break
            return request, {}

    def save(self):
        atomic_json(self.directory / "proxy-requests.json", self.requests)

    def finish(self, request, status):
        with self.lock:
            request["status"] = status
            self.save()

    def hold(self, rule, request):
        atomic_json(self.directory / "proxy-held.json", {"token": rule["token"], "request": request})
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            released = self.control().get("released", [])
            if released == rule["token"] or rule["token"] in released:
                return True
            time.sleep(0.02)
        return False


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"

    def do_GET(self):
        self.forward()

    def do_POST(self):
        self.forward()

    def do_PUT(self):
        self.forward()

    def do_PATCH(self):
        self.forward()

    def do_DELETE(self):
        self.forward()

    def forward(self):
        state = self.server.state
        body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        request, rule = state.begin(self.command, self.path, body)
        mode = rule.get("mode")
        if mode == "holdBefore" and not state.hold(rule, request):
            self.reply(504, b'{"error":"request gate timed out"}')
            state.finish(request, 504)
            return
        if mode in ("fail", "invalidProtocol"):
            status = 503 if mode == "fail" else 200
            payload = b'{"error":"controlled outage"}' if mode == "fail" else b'{"protocol_version":999}'
            self.reply(status, payload)
            state.finish(request, status)
            return
        connection = http.client.HTTPConnection(state.upstream.hostname, state.upstream.port, timeout=20)
        try:
            headers = {key: value for key, value in self.headers.items()
                       if key.lower() not in ("host", "connection", "transfer-encoding")}
            headers["Host"] = state.upstream.netloc
            connection.request(self.command, self.path, body=body, headers=headers)
            response = connection.getresponse()
            payload = response.read()
            status = response.status
            state.finish(request, status)
            if mode == "holdAfter" and not state.hold(rule, request):
                self.reply(504, b'{"error":"response gate timed out"}')
                return
            if mode == "dropAfter":
                self.close_connection = True
                self.connection.shutdown(socket.SHUT_RDWR)
                self.connection.close()
                return
            self.reply(status, payload)
        except (OSError, http.client.HTTPException) as error:
            state.finish(request, 502)
            try:
                self.reply(502, json.dumps({"error": str(error)}).encode())
            except (OSError, http.client.HTTPException):
                pass
        finally:
            connection.close()

    def reply(self, status, payload):
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, message, *args):
        print(message % args, flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--upstream", required=True)
    args = parser.parse_args()
    print("Starting controlled proxy.", flush=True)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    server.state = ProxyState(args.directory, args.upstream)
    atomic_json(args.directory / "proxy-endpoint.json", {"endpoint": f"http://127.0.0.1:{server.server_port}"})
    print(f"Controlled proxy listening on port {server.server_port}.", flush=True)
    server.serve_forever(poll_interval=0.05)


if __name__ == "__main__":
    main()
