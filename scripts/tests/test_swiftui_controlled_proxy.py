"""Check the request controls used by native server E2E tests."""

import concurrent.futures
import http.client
import importlib.util
import json
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest import mock
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


MODULE_PATH = Path(__file__).resolve().parents[2] / "apps/swiftui/Tests/UITests/Support/controlled_proxy.py"
SPEC = importlib.util.spec_from_file_location("swiftui_controlled_proxy", MODULE_PATH)
PROXY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROXY)


class BackendHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.server.reads += 1
        self.reply()

    def do_POST(self):
        self.server.payload = self.rfile.read(int(self.headers["Content-Length"]))
        self.server.writes += 1
        self.reply()

    def reply(self):
        payload = json.dumps({"writes": self.server.writes}).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *_):
        pass


class SilentProxyHandler(PROXY.Handler):
    def log_message(self, *_):
        pass


class ControlledProxyTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.directory = Path(self.temporary.name)
        self.backend = ThreadingHTTPServer(("127.0.0.1", 0), BackendHandler)
        self.backend.reads = 0
        self.backend.writes = 0
        self.backend.payload = None
        self.proxy = ThreadingHTTPServer(("127.0.0.1", 0), SilentProxyHandler)
        self.proxy.state = PROXY.ProxyState(self.directory, f"http://127.0.0.1:{self.backend.server_port}")
        self.threads = []
        for server in (self.backend, self.proxy):
            thread = threading.Thread(target=lambda current=server: current.serve_forever(poll_interval=0.01), daemon=True)
            thread.start()
            self.threads.append(thread)
        self.executor = concurrent.futures.ThreadPoolExecutor(max_workers=2)

    def tearDown(self):
        self.executor.shutdown(wait=True)
        for server in (self.proxy, self.backend):
            server.shutdown()
            server.server_close()
        for thread in self.threads:
            thread.join(timeout=2)
        self.temporary.cleanup()

    def call(self, method="GET", path="/v1/snapshot", body=None):
        connection = http.client.HTTPConnection("127.0.0.1", self.proxy.server_port, timeout=3)
        try:
            connection.request(method, path, body=body)
            response = connection.getresponse()
            return response.status, json.loads(response.read())
        finally:
            connection.close()

    def arm(self, mode, method="GET", path="/v1/snapshot"):
        self.token = str(time.monotonic_ns())
        PROXY.atomic_json(self.directory / "proxy-control.json",
                          {"token": self.token, "mode": mode, "method": method, "path": path})

    def wait_for_hold(self):
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            try:
                held = json.loads((self.directory / "proxy-held.json").read_text())
                if held["token"] == self.token:
                    return held
            except FileNotFoundError:
                pass
            time.sleep(0.005)
        self.fail("The request did not reach the gate.")

    def release(self):
        PROXY.atomic_json(self.directory / "proxy-control.json", {"released": self.token})

    def test_main_publishes_endpoint_without_reverse_dns(self):
        original_close = PROXY.ThreadingHTTPServer.server_close
        with mock.patch("sys.argv", [str(MODULE_PATH), "--directory", str(self.directory),
                                     "--upstream", "http://127.0.0.1:1"]), \
             mock.patch.object(PROXY.socket, "getfqdn", side_effect=AssertionError("Reverse DNS is unavailable")), \
             mock.patch.object(PROXY.ThreadingHTTPServer, "serve_forever"), \
             mock.patch.object(PROXY.ThreadingHTTPServer, "server_close", autospec=True,
                               side_effect=original_close) as close:
            PROXY.main()
        endpoint = json.loads((self.directory / "proxy-endpoint.json").read_text())["endpoint"]
        self.assertTrue(endpoint.startswith("http://127.0.0.1:"))
        self.assertEqual(close.call_count, 1)

    def test_forwarding_preserves_body_and_records_upstream_status(self):
        payload = b'{"name":"Planning"}'
        self.assertEqual(self.call("POST", "/v1/tasks", payload), (200, {"writes": 1}))
        self.assertEqual(self.backend.payload, payload)
        requests = json.loads((self.directory / "proxy-requests.json").read_text())
        self.assertEqual(requests[0]["method"], "POST")
        self.assertEqual(requests[0]["body"], payload.decode())
        self.assertEqual(requests[0]["status"], 200)

    def test_failure_rule_matches_one_request_and_does_not_reach_backend(self):
        self.arm("fail")
        self.assertEqual(self.call()[0], 503)
        self.assertEqual(self.backend.reads, 0)
        self.assertEqual(self.call(), (200, {"writes": 0}))
        self.assertEqual(self.backend.reads, 1)

    def test_hold_before_keeps_the_request_out_of_backend_until_release(self):
        self.arm("holdBefore")
        future = self.executor.submit(self.call)
        self.wait_for_hold()
        self.assertEqual(self.backend.reads, 0)
        self.assertFalse(future.done())
        self.release()
        self.assertEqual(future.result(timeout=3), (200, {"writes": 0}))
        self.assertEqual(self.backend.reads, 1)

    def test_hold_after_returns_the_captured_response_after_other_client_writes(self):
        self.arm("holdAfter")
        future = self.executor.submit(self.call)
        self.wait_for_hold()
        self.assertEqual(self.backend.reads, 1)
        self.assertFalse(future.done())
        self.assertEqual(self.call("POST", "/v1/tasks", b"{}"), (200, {"writes": 1}))
        self.release()
        self.assertEqual(future.result(timeout=3), (200, {"writes": 0}))

    def test_dropped_write_response_commits_once_and_logs_the_success(self):
        self.arm("dropAfter", method="POST", path="/v1/tasks")
        with self.assertRaises(http.client.RemoteDisconnected):
            self.call("POST", "/v1/tasks", b"{}")
        self.assertEqual(self.backend.writes, 1)
        self.assertEqual(self.call(), (200, {"writes": 1}))
        requests = json.loads((self.directory / "proxy-requests.json").read_text())
        self.assertEqual(requests[0]["status"], 200)

    def test_fault_plan_drops_write_then_blocks_reconciliation_once(self):
        rules = [{"token": "write", "mode": "dropAfter", "method": "POST", "path": "/v1/tasks"},
                 {"token": "refresh", "mode": "fail", "method": "GET", "path": "/v1/snapshot"}]
        PROXY.atomic_json(self.directory / "proxy-control.json", {"rules": rules})
        self.assertEqual(self.call(), (200, {"writes": 0}))
        self.assertEqual(self.backend.reads, 1)
        with self.assertRaises(http.client.RemoteDisconnected):
            self.call("POST", "/v1/tasks", b"{}")
        self.assertEqual(self.backend.writes, 1)
        self.assertEqual(self.call()[0], 503)
        self.assertEqual(self.backend.reads, 1)
        self.assertEqual(self.call(), (200, {"writes": 1}))
        self.assertEqual(self.backend.writes, 1)

    def test_fault_plan_drops_both_transport_attempts_before_reconciliation(self):
        rules = [{"token": token, "mode": "dropAfter", "method": "POST", "path": "/v1/tasks"}
                 for token in ("initial", "transport-retry")]
        rules.append({"token": "refresh", "mode": "fail", "method": "GET", "path": "/v1/snapshot"})
        PROXY.atomic_json(self.directory / "proxy-control.json", {"rules": rules})
        payload = b'{"request_id":"retained-write","name":"Planning"}'
        for attempt in range(2):
            with self.assertRaises(http.client.RemoteDisconnected):
                self.call("POST", "/v1/tasks", payload)
            self.assertEqual(self.backend.writes, attempt + 1)
            # An intervening read must not consume the next write fault.
            if attempt == 0:
                self.assertEqual(self.call(), (200, {"writes": 1}))
        reads = self.backend.reads
        self.assertEqual(self.call()[0], 503)
        self.assertEqual(self.backend.reads, reads)
        self.assertEqual(self.call(), (200, {"writes": 2}))
        requests = json.loads((self.directory / "proxy-requests.json").read_text())
        writes = [request for request in requests if request["method"] == "POST"]
        self.assertEqual([request["body"] for request in writes], [payload.decode()] * 2)
        self.assertEqual([request["status"] for request in writes], [200, 200])

    def test_replacing_fault_plan_releases_previous_held_request(self):
        self.arm("holdBefore")
        future = self.executor.submit(self.call)
        self.wait_for_hold()
        replacement = {"token": "replacement", "mode": "fail", "method": "GET", "path": "/v1/health"}
        PROXY.atomic_json(self.directory / "proxy-control.json", {"rules": [replacement], "released": [self.token]})
        self.assertEqual(future.result(timeout=3), (200, {"writes": 0}))
        self.assertEqual(self.call(path="/v1/health")[0], 503)

    def test_invalid_protocol_is_a_successful_http_response_with_incompatible_data(self):
        self.arm("invalidProtocol", path="/v1/health")
        self.assertEqual(self.call(path="/v1/health"), (200, {"protocol_version": 999}))
        self.assertEqual(self.backend.reads, 0)


if __name__ == "__main__":
    unittest.main()
