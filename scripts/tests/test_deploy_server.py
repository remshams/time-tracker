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
MIGRATIONS_SOURCE = """type Migration = fn(&Transaction<'_>) -> Result<(), StorageError>;

const MIGRATIONS: &[Migration] = &[
    apply_baseline,
    apply_task_timestamps,
];

pub(crate) const LATEST_VERSION: i64 = MIGRATIONS.len() as i64;
"""


def create_database(path, active=True, zero_duration=False):
    with contextlib.closing(sqlite3.connect(path)) as connection, connection:
        connection.execute("CREATE TABLE tasks (id TEXT, name TEXT, archived INTEGER, "
                           "created_at_us INTEGER, updated_at_us INTEGER)")
        connection.execute("CREATE TABLE worklogs (id TEXT, task_id TEXT, start_us INTEGER, end_us INTEGER)")
        connection.execute("PRAGMA user_version = 7")
        connection.execute("INSERT INTO tasks VALUES ('task-1', 'Planning', 0, 1000000, 2000000)")
        connection.execute("INSERT INTO worklogs VALUES ('worklog-1', 'task-1', 3000000, 4000000)")
        if active:
            connection.execute("INSERT INTO worklogs VALUES ('worklog-2', 'task-1', 5000000, NULL)")
        if zero_duration:
            connection.execute("INSERT INTO worklogs VALUES ('worklog-0', 'task-1', 4500000, 4500000)")


def migrate_to_version_8(path):
    """Stands in for a candidate migration that deletes zero-duration worklogs."""
    with contextlib.closing(sqlite3.connect(path)) as connection, connection:
        connection.execute("DELETE FROM worklogs WHERE end_us = start_us")
        connection.execute("PRAGMA user_version = 8")


class FakeCandidate:
    """Replaces the candidate process and applies its startup migration to the --db file."""

    migration = None

    def __init__(self, command, **options):
        if command[1:2] != ["--bind"] or not Path(command[0]).name.startswith("tt-server-"):
            raise AssertionError(f"Expected the standalone server command, got {command}")
        if FakeCandidate.migration:
            FakeCandidate.migration(command[command.index("--db") + 1])

    def terminate(self):
        pass

    def wait(self, timeout=None):
        return 0


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
    """Applies install and mv to real files and records every privileged command.

    When the started live binary is the candidate, start runs the candidate migration."""

    def __init__(self, live=None, database=None, migration=None):
        self.calls = []
        self.live, self.database, self.migration = live, database, migration

    def __call__(self, *arguments):
        self.calls.append(arguments)
        if arguments[:2] == ("systemctl", "start") and self.migration \
                and self.live.read_bytes() == b"candidate binary":
            self.migration(self.database)
        if arguments[0] == "install":
            shutil.copyfile(arguments[-2], arguments[-1])
        elif arguments[0] == "mv":
            Path(arguments[1]).replace(arguments[2])
        elif arguments[0] == "rm":
            Path(arguments[-1]).unlink(missing_ok=True)


class DeploymentRoot:
    def __init__(self, directory, zero_duration=False):
        self.root = Path(directory)
        (self.root / "build").mkdir()
        (self.root / "bin").mkdir()
        (self.root / "data").mkdir()
        self.live = self.root / "live-tt"
        self.live.write_bytes(b"previous binary")
        config = {**CONFIG, "live_binary": str(self.live)}
        (self.root / "deploy.json").write_text(json.dumps(config))
        self.database = self.root / "data/tt-server.db"
        create_database(self.database, zero_duration=zero_duration)
        self.archive = self.root / "build/source-0123456.tar"
        self.archive.write_bytes(b"source archive")
        self.candidate = self.root / "bin/tt-server-0123456"
        self.candidate.write_bytes(b"candidate binary")

    def host(self, schema=7):
        return DEPLOY.Host(self.root, REVISION, DEPLOY.digest(self.archive), 3, schema)

    def write_manifest(self, migration=None):
        before = DEPLOY.database_state(self.database)
        after = before
        if migration:
            copied = self.root / "copy.db"
            DEPLOY.backup_database(self.database, copied)
            migration(copied)
            after = DEPLOY.database_state(copied)
            copied.unlink()
        manifest = {"commit": REVISION, "sha256": DEPLOY.digest(self.candidate),
                    "database_before_sha256": DEPLOY.fingerprint(before),
                    "database_after_sha256": DEPLOY.fingerprint(after)}
        (self.root / "build/preflight-0123456.json").write_text(json.dumps(manifest))

    def manifest(self):
        return json.loads((self.root / "build/preflight-0123456.json").read_text())

    def preflight(self, schema=7, migration=None, accept=False, failure=None):
        FakeCandidate.migration = migration
        with patch.object(DEPLOY.subprocess, "Popen", FakeCandidate), \
                patch.object(DEPLOY, "verify", side_effect=failure), \
                patch.object(DEPLOY, "wait_until_ready"), patch("sys.stdout", io.StringIO()) as output:
            DEPLOY.remote_preflight(self.host(schema), accept)
        return output.getvalue()


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

    def test_counts_the_migrations_in_source(self):
        self.assertEqual(DEPLOY.schema_version(MIGRATIONS_SOURCE), 2)
        source = (SCRIPTS.parent / DEPLOY.MIGRATION_SOURCE).read_text()
        self.assertGreaterEqual(DEPLOY.schema_version(source), 7)
        with self.assertRaisesRegex(DEPLOY.DeployError, "MIGRATIONS list"):
            DEPLOY.schema_version("const OTHER: &[Migration] = &[\n    apply_baseline,\n];\n")


class BuildTests(unittest.TestCase):
    def test_docker_build_mounts_caches_and_a_read_only_source(self):
        with tempfile.TemporaryDirectory() as directory:
            host = DeploymentRoot(directory).host()
            command = DEPLOY.docker_build_command(host, Path("/work/source"), ".tt-server-0123456.new", 1000, 1000)
        self.assertIn("/work/source:/src:ro", command)
        self.assertIn(f"{host.build / 'cargo-home'}:/cargo-home", command)
        self.assertIn(f"{host.build / 'target'}:/target", command)
        self.assertIn("rust:1-bookworm", command)
        self.assertIn("cargo build --release --locked -p tracker-server-app --bin tt-server", command[-1])
        self.assertIn("install -o 1000 -g 1000 -m 0755 /target/release/tt-server /out/.tt-server-0123456.new", command[-1])

    def test_existing_candidate_is_reused_unless_rebuild_is_requested(self):
        with tempfile.TemporaryDirectory() as directory:
            host = DeploymentRoot(directory).host()
            with patch.object(DEPLOY, "run") as run, patch("sys.stdout", io.StringIO()):
                DEPLOY.remote_build(host, rebuild=False)
            run.assert_not_called()

    def test_old_tui_candidate_is_not_reused_and_build_log_names_the_server(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            root.candidate.unlink()
            (root.root / "bin/tt-0123456").write_bytes(b"old TUI candidate")
            host = root.host()

            def build(*command, **options):
                if command[0] == "docker":
                    (host.candidate.parent / f".{host.candidate.name}.new").write_bytes(b"server binary")

            with patch.object(DEPLOY, "run", side_effect=build) as run, patch("sys.stdout", io.StringIO()):
                DEPLOY.remote_build(host, rebuild=False)
            self.assertEqual(run.call_count, 2)
            self.assertEqual(host.candidate.read_bytes(), b"server binary")
            self.assertTrue((host.build / "build-tt-server-0123456.log").is_file())

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
                DEPLOY.verify(base, expected, now=NOW)

    def test_changed_task_name_and_missing_timer_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "tt.db"
            create_database(database)
            expected = DEPLOY.database_state(database)
            with fake_server(served_routes(task_name="Renamed")) as base:
                with self.assertRaisesRegex(DEPLOY.DeployError, "task catalog differs"):
                    DEPLOY.verify(base, expected, now=NOW)
            idle = Path(directory) / "idle.db"
            create_database(idle, active=False)
            with fake_server(served_routes()) as base:
                with self.assertRaisesRegex(DEPLOY.DeployError, "running worklog that the database lacks"):
                    DEPLOY.verify(base, DEPLOY.database_state(idle), now=NOW)

    def test_wrong_protocol_version_times_out(self):
        with fake_server(served_routes()) as base:
            with self.assertRaisesRegex(DEPLOY.DeployError, "unexpected health response"):
                DEPLOY.wait_until_ready(base, 4, timeout=0)


class PreflightTests(unittest.TestCase):
    def test_unchanged_database_needs_no_approval(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            root.preflight()
            manifest = root.manifest()
            self.assertEqual(manifest["database_before_sha256"], manifest["database_after_sha256"])
            self.assertIsNone(manifest["migration_changes"])
            self.assertEqual((manifest["schema_version"], manifest["preflight_worklogs"]), (7, 2))
            self.assertEqual(len(list((root.root / "build").glob("smoke-*"))), 0)

    def test_migration_changes_need_approval(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory, zero_duration=True)
            with self.assertRaisesRegex(DEPLOY.DeployError, r"\(schema version 7 -> 8; worklogs: 1 removed\)"
                                                           r".*--accept-migration-changes"):
                root.preflight(schema=8, migration=migrate_to_version_8)
            self.assertFalse((root.root / "build/preflight-0123456.json").exists())

    def test_approved_migration_records_both_fingerprints(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory, zero_duration=True)
            before = DEPLOY.database_state(root.database)
            output = root.preflight(schema=8, migration=migrate_to_version_8, accept=True)
            manifest = root.manifest()
            self.assertIn("Approved migration changes: schema version 7 -> 8; worklogs: 1 removed", output)
            self.assertEqual(manifest["migration_changes"], "schema version 7 -> 8; worklogs: 1 removed")
            self.assertEqual(manifest["database_before_sha256"], DEPLOY.fingerprint(before))
            self.assertNotEqual(manifest["database_after_sha256"], manifest["database_before_sha256"])
            self.assertEqual((manifest["schema_version"], manifest["preflight_worklogs"]), (8, 2))
            self.assertEqual(DEPLOY.database_state(root.database), before)

    def test_schema_must_end_at_the_candidates_latest_migration(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            with self.assertRaisesRegex(DEPLOY.DeployError, "left schema version 7, but its migrations end at 8"):
                root.preflight(schema=8, accept=True)
            with self.assertRaisesRegex(DEPLOY.DeployError, "left schema version 8, but its migrations end at 7"):
                root.preflight(schema=7, migration=migrate_to_version_8, accept=True)
            with self.assertRaisesRegex(DEPLOY.DeployError, "schema version 7, newer than the candidate's 6"):
                root.preflight(schema=6)

    def test_failed_candidate_keeps_and_names_its_log_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            failure = DEPLOY.DeployError("The served task catalog differs from the database")
            with self.assertRaisesRegex(DEPLOY.DeployError, "catalog differs from the database. "
                                                            "The server log and database copy are in ") as raised:
                root.preflight(failure=failure)
            smoke = list((root.root / "build").glob("smoke-*"))
            self.assertEqual(len(smoke), 1)
            self.assertIn(str(smoke[0]), str(raised.exception))
            self.assertTrue((smoke[0] / "tt-server.log").exists())
            self.assertTrue((smoke[0] / "tt-server.db").exists())
            self.assertFalse((root.root / "build/preflight-0123456.json").exists())

    def test_failure_before_the_candidate_starts_removes_the_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            with self.assertRaisesRegex(DEPLOY.DeployError, "newer than the candidate's 6"):
                root.preflight(schema=6)
            root.database.unlink()
            with self.assertRaisesRegex(DEPLOY.DeployError, "Could not copy the database .*tt-server.db"):
                root.preflight()
            self.assertEqual(list((root.root / "build").glob("smoke-*")), [])

    def test_failed_cleanup_keeps_the_original_error_and_names_the_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            with patch.object(DEPLOY.shutil, "rmtree", side_effect=PermissionError("denied")), \
                    self.assertRaisesRegex(DEPLOY.DeployError, "newer than the candidate's 6. "
                                                               "Could not remove the preflight directory ") as raised:
                root.preflight(schema=6)
            smoke = list((root.root / "build").glob("smoke-*"))
            self.assertEqual(len(smoke), 1)
            self.assertIn(str(smoke[0]), str(raised.exception))

    def test_describes_added_removed_and_changed_rows(self):
        before = {"schema": 7, "tasks": [["task-1", "Planning"], ["task-2", "Review"]],
                  "worklogs": [["worklog-1", "task-1"]]}
        after = {"schema": 7, "tasks": [["task-1", "Renamed"], ["task-3", "Support"]],
                 "worklogs": [["worklog-1", "task-1"]]}
        self.assertEqual(DEPLOY.describe_changes(before, after), "tasks: 1 removed, 1 added, 1 changed")
        self.assertEqual(DEPLOY.describe_changes(before, before), "")


class DeployTests(unittest.TestCase):
    def deploy(self, root, sudo, **patches):
        with patch.object(DEPLOY, "sudo", sudo), patch.object(DEPLOY, "verify", **patches), \
                patch.object(DEPLOY, "wait_until_ready"), patch("sys.stdout", io.StringIO()), \
                patch("sys.stderr", io.StringIO()) as errors:
            try:
                DEPLOY.remote_deploy(root.host(), keep_backup=False)
            finally:
                self.errors = errors.getvalue()

    def test_approved_migration_deploys_and_records_the_migrated_state(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory, zero_duration=True)
            root.write_manifest(migrate_to_version_8)
            self.deploy(root, FakeSudo(root.live, root.database, migrate_to_version_8))
            latest = json.loads((root.root / "build/deployment-latest.json").read_text())
            self.assertEqual((latest["tasks"], latest["worklogs"]), (1, 2))
            self.assertEqual(DEPLOY.database_state(root.database)["schema"], 8)

    def test_failure_after_a_migration_restores_the_database_and_the_previous_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory, zero_duration=True)
            before = DEPLOY.database_state(root.database)
            root.write_manifest(migrate_to_version_8)
            sudo = FakeSudo(root.live, root.database, migrate_to_version_8)
            failure = DEPLOY.DeployError("The served task catalog differs from the database")
            with self.assertRaises(DEPLOY.DeployError):
                self.deploy(root, sudo, side_effect=failure)
            self.assertEqual(DEPLOY.database_state(root.database), before)
            self.assertEqual(root.live.read_bytes(), b"previous binary")
            self.assertEqual([call[:2] for call in sudo.calls if call[0] == "systemctl"],
                             [("systemctl", "stop"), ("systemctl", "start"),
                              ("systemctl", "stop"), ("systemctl", "start")])
            self.assertIn("rolled back", self.errors)

    def test_unapproved_startup_changes_are_rolled_back(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory, zero_duration=True)
            before = DEPLOY.database_state(root.database)
            root.write_manifest()
            with self.assertRaisesRegex(DEPLOY.DeployError, "differently from the preflight"):
                self.deploy(root, FakeSudo(root.live, root.database, migrate_to_version_8))
            self.assertEqual(DEPLOY.database_state(root.database), before)
            self.assertEqual(root.live.read_bytes(), b"previous binary")

    def test_live_changes_after_the_preflight_stop_the_deployment_before_the_swap(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            root.write_manifest()
            with contextlib.closing(sqlite3.connect(root.database)) as connection, connection:
                connection.execute("UPDATE tasks SET name = 'Renamed'")
            sudo = FakeSudo()
            with self.assertRaisesRegex(DEPLOY.DeployError, "changed after the preflight"):
                self.deploy(root, sudo)
            self.assertEqual(root.live.read_bytes(), b"previous binary")
            self.assertEqual(sudo.calls[-2:], [("systemctl", "start", "tracker.service"),
                                               ("rm", "-f", f"{root.live}.new-0123456")])

    def test_failed_restore_reports_an_unfinished_rollback(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            root.write_manifest()
            sudo = FakeSudo()
            restore = DEPLOY.DeployError("The restored database differs")
            failure = DEPLOY.DeployError("The served task catalog differs from the database")
            with patch.object(DEPLOY, "restore_database", side_effect=restore):
                with self.assertRaisesRegex(DEPLOY.DeployError, "restored database differs"):
                    self.deploy(root, sudo, side_effect=failure)
            self.assertIn("rollback did not finish", self.errors)
            self.assertEqual(sudo.calls[-1], ("systemctl", "stop", "tracker.service"))

    def test_restore_rejects_a_backup_with_different_rows(self):
        with tempfile.TemporaryDirectory() as directory:
            backup, live = Path(directory) / "backup.db", Path(directory) / "live.db"
            create_database(backup)
            create_database(live, active=False)
            expected = DEPLOY.database_state(live)
            with self.assertRaisesRegex(DEPLOY.DeployError, "restored database differs"):
                DEPLOY.restore_database(backup, live, expected)
            DEPLOY.restore_database(backup, live, DEPLOY.database_state(backup))

    def test_rejects_a_preflight_record_without_fingerprints_or_a_read_only_database(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            manifest = {"commit": REVISION, "sha256": DEPLOY.digest(root.candidate)}
            (root.root / "build/preflight-0123456.json").write_text(json.dumps(manifest))
            with patch.object(DEPLOY, "sudo") as sudo:
                with self.assertRaisesRegex(DEPLOY.DeployError,
                                            "is missing: database_after_sha256, database_before_sha256;"):
                    DEPLOY.remote_deploy(root.host(), keep_backup=False)
                root.write_manifest()
                with patch.object(DEPLOY.os, "access", return_value=False), \
                        self.assertRaisesRegex(DEPLOY.DeployError, "must be writable"):
                    DEPLOY.remote_deploy(root.host(), keep_backup=False)
            sudo.assert_not_called()

    def test_rejects_a_missing_corrupt_or_incomplete_preflight_record(self):
        cases = [
            (None, "preflight-0123456.json does not exist; run the preflight first"),
            ("{", "preflight-0123456.json is not valid JSON .*; run the preflight again"),
            ("[]", "preflight-0123456.json must contain a JSON object"),
            (json.dumps({"sha256": "abc", "database_before_sha256": "a", "database_after_sha256": "b"}),
             "preflight-0123456.json is missing: commit; run the preflight again"),
            (json.dumps({}), "is missing: commit, database_after_sha256, database_before_sha256, sha256;"),
        ]
        for text, message in cases:
            with self.subTest(message=message), tempfile.TemporaryDirectory() as directory:
                root = DeploymentRoot(directory)
                if text is not None:
                    (root.root / "build/preflight-0123456.json").write_text(text)
                with patch.object(DEPLOY, "sudo") as sudo, self.assertRaisesRegex(DEPLOY.DeployError, message):
                    DEPLOY.remote_deploy(root.host(), keep_backup=False)
                sudo.assert_not_called()
                self.assertFalse((root.root / "backups").exists())

    def test_existing_staged_binary_stops_the_deployment_before_the_backup(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            root.write_manifest()
            Path(f"{root.live}.new-0123456").write_bytes(b"stale binary")
            with patch.object(DEPLOY, "sudo") as sudo, \
                    self.assertRaisesRegex(DEPLOY.DeployError, r"live-tt\.new-0123456 already exists"):
                DEPLOY.remote_deploy(root.host(), keep_backup=False)
            sudo.assert_not_called()
            self.assertFalse((root.root / "backups").exists())

    def test_unreadable_database_is_reported_with_its_path(self):
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "tt.db"
            database.write_bytes(b"not a database" * 100)
            with self.assertRaisesRegex(DEPLOY.DeployError, f"Could not read the database {database}: "):
                DEPLOY.database_state(database)
            with self.assertRaisesRegex(DEPLOY.DeployError, f"Could not copy the database {database} to "):
                DEPLOY.backup_database(database, Path(directory) / "copy.db")

    def test_successful_deployment_records_the_result_and_removes_the_backup(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            root.write_manifest()
            sudo = FakeSudo()
            with patch.object(DEPLOY, "sudo", sudo), patch.object(DEPLOY, "verify"), \
                    patch.object(DEPLOY, "wait_until_ready"), patch("sys.stdout", io.StringIO()):
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
                    patch.object(DEPLOY, "wait_until_ready"), patch("sys.stderr", io.StringIO()) as errors:
                with self.assertRaises(DEPLOY.DeployError):
                    DEPLOY.remote_deploy(root.host(), keep_backup=False)
            self.assertEqual(root.live.read_bytes(), b"previous binary")
            self.assertEqual(sudo.calls[-1], ("systemctl", "start", "tracker.service"))
            self.assertIn("rolled back", errors.getvalue())
            backups = list((root.root / "backups").iterdir())
            self.assertEqual(len(backups), 1)
            self.assertTrue((backups[0] / "tt-server.db").exists())
            self.assertFalse((root.root / "build/deployment-latest.json").exists())

    def test_failure_before_the_swap_restarts_the_service_and_removes_the_staged_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            root.write_manifest()
            sudo = FakeSudo()
            failure = DEPLOY.DeployError("The database copy failed its integrity check")
            with patch.object(DEPLOY, "sudo", sudo), patch.object(DEPLOY, "backup_database", side_effect=failure), \
                    patch("sys.stderr", io.StringIO()):
                with self.assertRaises(DEPLOY.DeployError):
                    DEPLOY.remote_deploy(root.host(), keep_backup=False)
            staged = f"{root.live}.new-0123456"
            self.assertEqual(root.live.read_bytes(), b"previous binary")
            self.assertFalse(Path(staged).exists())
            self.assertEqual(sudo.calls[-2:], [("systemctl", "start", "tracker.service"), ("rm", "-f", staged)])

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
        self.remote_calls = []

        def remote(host, *arguments):
            self.remote_calls.append((host, *arguments))
            return json.dumps(CONFIG) if arguments[0] == "cat" else ""

        arguments = DEPLOY.workstation_parser().parse_args(["--host", "server", "--root", "/srv/tt/", *extra])
        with patch.object(DEPLOY, "resolve_revision", return_value=REVISION), \
                patch.object(DEPLOY, "capture", return_value="pub const VERSION: u32 = 3;\n" + MIGRATIONS_SOURCE), \
                patch.object(DEPLOY, "remote", side_effect=remote), \
                patch.object(DEPLOY, "run"), patch.object(DEPLOY, "upload_archive"), \
                patch.object(DEPLOY, "remote_helper", side_effect=lambda *call: helpers.append(call)), \
                patch.object(DEPLOY, "check_health") as health, patch("sys.stdout", io.StringIO()):
            DEPLOY.deploy_from_workstation(arguments)
        return helpers, health

    def test_preflight_only_never_runs_the_deploy_step(self):
        helpers, health = self.run_workstation("--preflight-only")
        self.assertEqual([call[2] for call in helpers], ["build", "preflight"])
        self.assertNotIn("--accept-migration-changes", helpers[1])
        self.assertEqual(self.remote_calls[1], ("server", "mkdir", "-p", "/srv/tt/build", "/srv/tt/bin"))
        self.assertTrue(all(call[:2] == ("server", "/srv/tt") for call in helpers))
        health.assert_not_called()

    def test_full_deployment_passes_options_and_checks_health(self):
        helpers, health = self.run_workstation("--rebuild", "--keep-backup", "--accept-migration-changes")
        self.assertEqual([call[2] for call in helpers], ["build", "preflight", "deploy"])
        self.assertIn("--rebuild", helpers[0])
        self.assertIn("--keep-backup", helpers[2])
        self.assertEqual(helpers[1][3:], ("--revision", REVISION, "--archive-sha256", helpers[1][6],
                                           "--protocol-version", "3", "--schema-version", "2",
                                           "--accept-migration-changes"))
        self.assertNotIn("--accept-migration-changes", helpers[2])
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

    def test_main_reports_json_errors_without_a_traceback(self):
        failure = json.JSONDecodeError("Expecting value", "", 0)
        with patch.object(DEPLOY, "deploy_from_workstation", side_effect=failure), \
                patch("sys.stderr", io.StringIO()) as errors:
            self.assertEqual(DEPLOY.main(["--host", "server", "--root", "/srv/tt"]), 1)
        self.assertIn(f"Deployment stopped: {failure}", errors.getvalue())

    def test_main_reports_host_step_failures_without_a_traceback(self):
        with tempfile.TemporaryDirectory() as directory:
            root = DeploymentRoot(directory)
            (root.root / "build/preflight-0123456.json").write_text("{")
            common = ["--root", str(root.root), "--revision", REVISION, "--archive-sha256",
                      DEPLOY.digest(root.archive), "--protocol-version", "3", "--schema-version", "7"]
            root.database.unlink()
            for step, message in [("preflight", "Could not copy the database"),
                                  ("deploy", "is not valid JSON")]:
                with self.subTest(step=step), patch("sys.stderr", io.StringIO()) as errors:
                    self.assertEqual(DEPLOY.main(["remote", step, *common]), 1)
                self.assertIn("Deployment stopped: ", errors.getvalue())
                self.assertIn(message, errors.getvalue())


if __name__ == "__main__":
    unittest.main()
