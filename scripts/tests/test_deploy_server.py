import contextlib
import datetime
import http.server
import importlib.util
import io
import json
from pathlib import Path
import shutil
import sqlite3
import tempfile
import threading
import unittest
from unittest.mock import patch


SCRIPTS = Path(__file__).parents[1]
SPEC = importlib.util.spec_from_file_location("deploy_server", SCRIPTS / "deploy-server.py")
DEPLOY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DEPLOY)

REVISION = "0123456789abcdef0123456789abcdef01234567"
CONFIG = {
    "service": "tracker.service",
    "live_binary": "/usr/local/bin/tt",
    "database": "data/tt-server.db",
    "base_url": "http://127.0.0.1:8765/",
    "rust_image": "rust:1-bookworm",
}
NOW = datetime.datetime(2026, 10, 16, 12, 0, tzinfo=datetime.timezone.utc)


def create_database(path, active=True):
    with contextlib.closing(sqlite3.connect(path)) as connection, connection:
        connection.execute("CREATE TABLE tasks (id TEXT, name TEXT, archived INTEGER, "
                           "created_at_us INTEGER, updated_at_us INTEGER)")
        connection.execute("CREATE TABLE worklogs (id TEXT, task_id TEXT, start_us INTEGER, end_us INTEGER)")
        connection.execute("PRAGMA user_version = 7")
        connection.execute("INSERT INTO tasks VALUES ('task-1', 'Planning', 0, 1000000, 2000000)")
        connection.execute("INSERT INTO worklogs VALUES ('worklog-1', 'task-1', 3000000, 4000000)")
        if active:
            connection.execute("INSERT INTO worklogs VALUES ('worklog-2', 'task-1', 5000000, NULL)")


def served_routes(task_name="Planning"):
    tasks = [{"id": "task-1", "name": task_name, "archived": False,
              "created_at": "1970-01-01T00:00:01Z", "updated_at": "1970-01-01T00:00:02Z"}]
    active = {"id": "worklog-2", "task_id": "task-1", "start": "1970-01-01T00:00:05Z", "end": None}
    totals = {"rows": [{"task_id": "task-1", "duration_us": 60}], "total_us": 60, "revision": "r1"}
    return {
        "/v1/health": {"status": "ok", "protocol_version": 3},
        "/v1/state/task-list": {"tasks": tasks, "active_worklog": active, "revision": "r1"},
        "/v1/state/daily-overview": {"tasks": tasks, "active_worklog": active, **totals},
        "/v1/reports/task-totals": totals,
        "/v1/tasks": {"tasks": tasks},
        "/v1/tracking": {"active_worklog": active},
        "/v1/worklogs": {"worklogs": [], "next_cursor": None, "revision": "r1"},
    }


@contextlib.contextmanager
def fake_server(routes):
    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            body = json.dumps(routes[self.path.split("?")[0]]).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *arguments):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}"
    finally:
        server.shutdown()
        server.server_close()


class FakeSudo:
    """Applies install and mv to real files and records every privileged command."""

    def __init__(self):
        self.calls = []

    def __call__(self, *arguments):
        self.calls.append(arguments)
        if arguments[0] == "install":
            shutil.copyfile(arguments[-2], arguments[-1])
        elif arguments[0] == "mv":
            Path(arguments[1]).replace(arguments[2])


class DeploymentRoot:
    def __init__(self, directory):
        self.root = Path(directory)
        (self.root / "build").mkdir()
        (self.root / "bin").mkdir()
        (self.root / "data").mkdir()
        self.live = self.root / "live-tt"
        self.live.write_bytes(b"previous binary")
        config = {**CONFIG, "live_binary": str(self.live)}
        (self.root / "deploy.json").write_text(json.dumps(config))
        create_database(self.root / "data/tt-server.db")
        self.archive = self.root / "build/source-0123456.tar"
        self.archive.write_bytes(b"source archive")
        self.candidate = self.root / "bin/tt-0123456"
        self.candidate.write_bytes(b"candidate binary")

    def host(self):
        return DEPLOY.Host(self.root, REVISION, DEPLOY.digest(self.archive), 3)

    def write_manifest(self):
        manifest = {"commit": REVISION, "sha256": DEPLOY.digest(self.candidate)}
        (self.root / "build/preflight-0123456.json").write_text(json.dumps(manifest))


class ConfigTests(unittest.TestCase):
    def test_valid_config_strips_trailing_slash(self):
        config = DEPLOY.load_config(json.dumps(CONFIG))
        self.assertEqual(config["base_url"], "http://127.0.0.1:8765")

    def test_rejects_missing_unknown_and_invalid_values(self):
        cases = [
            ({key: value for key, value in CONFIG.items() if key != "service"}, "missing: service"),
            ({**CONFIG, "port": "8765"}, "unknown keys: port"),
            ({**CONFIG, "service": ""}, "service must be a non-empty string"),
            ({**CONFIG, "base_url": "100.1.2.3:8765"}, "base_url must start with"),
        ]
        for config, message in cases:
            with self.subTest(message=message), self.assertRaisesRegex(DEPLOY.DeployError, message):
                DEPLOY.load_config(json.dumps(config))
        with self.assertRaisesRegex(DEPLOY.DeployError, "not valid JSON"):
            DEPLOY.load_config("{")
        with self.assertRaisesRegex(DEPLOY.DeployError, "JSON object"):
            DEPLOY.load_config("[]")

    def test_reads_the_protocol_version_from_source(self):
        self.assertEqual(DEPLOY.protocol_version("//! Protocol\npub const VERSION: u32 = 3;\n"), 3)
        with self.assertRaisesRegex(DEPLOY.DeployError, "protocol VERSION"):
            DEPLOY.protocol_version("pub const OTHER: u32 = 3;\n")


class BuildTests(unittest.TestCase):
    def test_docker_build_mounts_caches_and_a_read_only_source(self):
        with tempfile.TemporaryDirectory() as directory:
            host = DeploymentRoot(directory).host()
            command = DEPLOY.docker_build_command(host, Path("/work/source"), ".tt-0123456.new", 1000, 1000)
        self.assertIn("/work/source:/src:ro", command)
        self.assertIn(f"{host.build / 'cargo-home'}:/cargo-home", command)
        self.assertIn(f"{host.build / 'target'}:/target", command)
        self.assertIn("rust:1-bookworm", command)
        self.assertIn("cargo build --release --locked -p tracker-tui", command[-1])
        self.assertIn("install -o 1000 -g 1000 -m 0755 /target/release/tt /out/.tt-0123456.new", command[-1])

    def test_existing_candidate_is_reused_unless_rebuild_is_requested(self):
        with tempfile.TemporaryDirectory() as directory:
            host = DeploymentRoot(directory).host()
            with patch.object(DEPLOY, "run") as run, patch("sys.stdout", io.StringIO()):
                DEPLOY.remote_build(host, rebuild=False)
            run.assert_not_called()

    def test_rejects_an_archive_that_changed_after_upload(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            host = root.host()
            root.archive.write_bytes(b"changed archive")
            with self.assertRaisesRegex(DEPLOY.DeployError, "does not match"):
                DEPLOY.remote_build(host, rebuild=True)


class VerifyTests(unittest.TestCase):
    def test_served_state_matching_the_database_passes(self):
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "tt.db"
            create_database(database)
            expected = DEPLOY.database_state(database)
            self.assertEqual(expected["schema"], 7)
            with fake_server(served_routes()) as base:
                DEPLOY.verify(base, 3, expected, now=NOW)

    def test_changed_task_name_and_missing_timer_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "tt.db"
            create_database(database)
            expected = DEPLOY.database_state(database)
            with fake_server(served_routes(task_name="Renamed")) as base:
                with self.assertRaisesRegex(DEPLOY.DeployError, "task catalog differs"):
                    DEPLOY.verify(base, 3, expected, now=NOW)
            idle = Path(directory) / "idle.db"
            create_database(idle, active=False)
            with fake_server(served_routes()) as base:
                with self.assertRaisesRegex(DEPLOY.DeployError, "running worklog that the database lacks"):
                    DEPLOY.verify(base, 3, DEPLOY.database_state(idle), now=NOW)

    def test_wrong_protocol_version_times_out(self):
        with fake_server(served_routes()) as base:
            with self.assertRaisesRegex(DEPLOY.DeployError, "unexpected health response"):
                DEPLOY.wait_until_ready(base, 4, timeout=0)


class DeployTests(unittest.TestCase):
    def test_successful_deployment_records_the_result_and_removes_the_backup(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            root.write_manifest()
            sudo = FakeSudo()
            with patch.object(DEPLOY, "sudo", sudo), patch.object(DEPLOY, "verify"), \
                    patch("sys.stdout", io.StringIO()):
                DEPLOY.remote_deploy(root.host(), keep_backup=False)
            self.assertEqual(root.live.read_bytes(), b"candidate binary")
            self.assertEqual([call[:2] for call in sudo.calls if call[0] == "systemctl"],
                             [("systemctl", "stop"), ("systemctl", "start"), ("systemctl", "is-active")])
            latest = json.loads((root.root / "build/deployment-latest.json").read_text())
            self.assertEqual(latest, json.loads((root.root / "build/deployment-0123456.json").read_text()))
            self.assertEqual((latest["deployment"], latest["tasks"], latest["worklogs"]), ("passed", 1, 2))
            self.assertIn("backup_files_removed_at", latest)
            self.assertFalse(Path(latest["backup_directory"]).exists())

    def test_failed_verification_restores_the_previous_binary_and_keeps_the_backup(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            root.write_manifest()
            sudo = FakeSudo()
            failure = DEPLOY.DeployError("The served task catalog differs from the database")
            with patch.object(DEPLOY, "sudo", sudo), patch.object(DEPLOY, "verify", side_effect=failure), \
                    patch("sys.stderr", io.StringIO()) as errors:
                with self.assertRaises(DEPLOY.DeployError):
                    DEPLOY.remote_deploy(root.host(), keep_backup=False)
            self.assertEqual(root.live.read_bytes(), b"previous binary")
            self.assertEqual(sudo.calls[-1], ("systemctl", "start", "tracker.service"))
            self.assertIn("rolled back", errors.getvalue())
            backups = list((root.root / "backups").iterdir())
            self.assertEqual(len(backups), 1)
            self.assertTrue((backups[0] / "tt-server.db").exists())
            self.assertFalse((root.root / "build/deployment-latest.json").exists())

    def test_rejects_a_candidate_that_changed_after_preflight(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            root.write_manifest()
            root.candidate.write_bytes(b"rebuilt binary")
            with patch.object(DEPLOY, "sudo") as sudo:
                with self.assertRaisesRegex(DEPLOY.DeployError, "run the preflight again"):
                    DEPLOY.remote_deploy(root.host(), keep_backup=False)
            sudo.assert_not_called()


class WorkstationTests(unittest.TestCase):
    def run_workstation(self, *extra):
        helpers = []
        arguments = DEPLOY.workstation_parser().parse_args(["--host", "server", "--root", "/srv/tt/", *extra])
        with patch.object(DEPLOY, "resolve_revision", return_value=REVISION), \
                patch.object(DEPLOY, "capture", return_value="pub const VERSION: u32 = 3;\n"), \
                patch.object(DEPLOY, "remote", return_value=json.dumps(CONFIG)), \
                patch.object(DEPLOY, "run"), patch.object(DEPLOY, "upload_archive"), \
                patch.object(DEPLOY, "remote_helper", side_effect=lambda *call: helpers.append(call)), \
                patch.object(DEPLOY, "check_health") as health, patch("sys.stdout", io.StringIO()):
            DEPLOY.deploy_from_workstation(arguments)
        return helpers, health

    def test_preflight_only_never_runs_the_deploy_step(self):
        helpers, health = self.run_workstation("--preflight-only")
        self.assertEqual([call[2] for call in helpers], ["build", "preflight"])
        self.assertTrue(all(call[:2] == ("server", "/srv/tt") for call in helpers))
        health.assert_not_called()

    def test_full_deployment_passes_options_and_checks_health(self):
        helpers, health = self.run_workstation("--rebuild", "--keep-backup")
        self.assertEqual([call[2] for call in helpers], ["build", "preflight", "deploy"])
        self.assertIn("--rebuild", helpers[0])
        self.assertIn("--keep-backup", helpers[2])
        self.assertEqual(helpers[1][3:], ("--revision", REVISION, "--archive-sha256", helpers[1][6],
                                           "--protocol-version", "3"))
        health.assert_called_once_with("http://127.0.0.1:8765", 3)

    def test_upload_refuses_to_replace_a_different_archive(self):
        with patch.object(DEPLOY, "remote", return_value="abc  /srv/tt/build/source-0123456.tar\n"), \
                patch.object(DEPLOY, "run") as run:
            with self.assertRaisesRegex(DEPLOY.DeployError, "different contents"):
                DEPLOY.upload_archive("server", "/srv/tt", Path("source.tar"), "source-0123456.tar", "def")
            with patch("sys.stdout", io.StringIO()):
                DEPLOY.upload_archive("server", "/srv/tt", Path("source.tar"), "source-0123456.tar", "abc")
            run.assert_not_called()

    def test_main_reports_errors_without_a_traceback(self):
        failure = DEPLOY.DeployError("Unknown revision: missing")
        with patch.object(DEPLOY, "deploy_from_workstation", side_effect=failure), \
                patch("sys.stderr", io.StringIO()) as errors:
            self.assertEqual(DEPLOY.main(["--host", "server", "--root", "/srv/tt", "--revision", "missing"]), 1)
        self.assertIn("Deployment stopped: Unknown revision: missing", errors.getvalue())


if __name__ == "__main__":
    unittest.main()
