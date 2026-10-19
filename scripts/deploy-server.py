#!/usr/bin/env python3
"""Build a revision on a server host, check it against a database copy, and deploy it.

Run it from a checkout on your workstation:

    python3 scripts/deploy-server.py --host HOST --root /path/to/deployment

The script archives the revision, uploads itself and the archive to the host, and runs
the build, preflight and deployment steps there through SSH. Host-specific settings live
in ROOT/deploy.json on the host, so this public repository holds no addresses or paths.
"""

import argparse
import contextlib
import datetime
import hashlib
import json
import os
import pathlib
import re
import shlex
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.parse
import urllib.request


REPOSITORY = pathlib.Path(__file__).resolve().parents[1]
PROTOCOL_SOURCE = "crates/tracker-protocol/src/lib.rs"
MIGRATION_SOURCE = "crates/tracker-storage/src/migrate.rs"
PACKAGE = "tracker-tui"
BINARY = "tt"
CONFIG_KEYS = {"service", "live_binary", "database", "base_url", "rust_image"}
HELPER = "build/deploy-server.py"


class DeployError(Exception):
    pass


# Shared helpers


def run(*arguments, **options):
    return subprocess.run(arguments, check=True, **options)


def capture(*arguments):
    return subprocess.run(arguments, check=True, capture_output=True, text=True).stdout


def digest(path):
    hasher = hashlib.sha256()
    with pathlib.Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            hasher.update(block)
    return hasher.hexdigest()


def short(revision):
    return revision[:7]


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc)


def load_config(text):
    try:
        config = json.loads(text)
    except json.JSONDecodeError as error:
        raise DeployError(f"deploy.json is not valid JSON: {error}") from error
    if not isinstance(config, dict):
        raise DeployError("deploy.json must contain a JSON object")
    missing = sorted(CONFIG_KEYS - set(config))
    if missing:
        raise DeployError(f"deploy.json is missing: {', '.join(missing)}")
    unknown = sorted(set(config) - CONFIG_KEYS)
    if unknown:
        raise DeployError(f"deploy.json has unknown keys: {', '.join(unknown)}")
    for key in sorted(CONFIG_KEYS):
        if not isinstance(config[key], str) or not config[key]:
            raise DeployError(f"deploy.json value {key} must be a non-empty string")
    if not config["base_url"].startswith(("http://", "https://")):
        raise DeployError("deploy.json base_url must start with http:// or https://")
    config["base_url"] = config["base_url"].rstrip("/")
    return config


def protocol_version(source):
    match = re.search(r"^pub const VERSION: u32 = (\d+);$", source, re.MULTILINE)
    if match is None:
        raise DeployError(f"Could not find the protocol VERSION in {PROTOCOL_SOURCE}")
    return int(match.group(1))


def schema_version(source):
    match = re.search(r"^const MIGRATIONS: &\[Migration\] = &\[\n(.*?)^\];$", source, re.MULTILINE | re.DOTALL)
    if match is None:
        raise DeployError(f"Could not find the MIGRATIONS list in {MIGRATION_SOURCE}")
    return len(re.findall(r"^\s*\w+,$", match.group(1), re.MULTILINE))


# Workstation side


def resolve_revision(revision):
    try:
        return capture("git", "-C", str(REPOSITORY), "rev-parse", "--verify", f"{revision}^{{commit}}").strip()
    except subprocess.CalledProcessError as error:
        raise DeployError(f"Unknown revision: {revision}") from error


def remote(host, *arguments):
    return capture("ssh", "-o", "BatchMode=yes", host, shlex.join(arguments))


def remote_helper(host, root, *arguments):
    command = ["python3", f"{root}/{HELPER}", "remote", *arguments, "--root", root]
    run("ssh", "-o", "BatchMode=yes", host, shlex.join(command))


def upload_archive(host, root, archive, name, archive_hash):
    destination = f"{root}/build/{name}"
    existing = remote(host, "sh", "-c", f"test ! -e {shlex.quote(destination)} || sha256sum {shlex.quote(destination)}")
    if existing:
        if existing.split()[0] != archive_hash:
            raise DeployError(f"{destination} already exists with different contents")
        print(f"Reusing uploaded source archive {name}", flush=True)
        return
    run("scp", "-q", "-o", "BatchMode=yes", str(archive), f"{host}:{destination}")


def check_health(base_url, version):
    health = get_json(base_url, "/v1/health")
    if health != {"status": "ok", "protocol_version": version}:
        raise DeployError(f"Unexpected health response from {base_url}: {health}")


def deploy_from_workstation(arguments):
    revision = resolve_revision(arguments.revision)
    version = protocol_version(capture("git", "-C", str(REPOSITORY), "show", f"{revision}:{PROTOCOL_SOURCE}"))
    schema = schema_version(capture("git", "-C", str(REPOSITORY), "show", f"{revision}:{MIGRATION_SOURCE}"))
    host, root = arguments.host, arguments.root.rstrip("/")
    config = load_config(remote(host, "cat", f"{root}/deploy.json"))
    remote(host, "mkdir", "-p", f"{root}/build", f"{root}/bin")
    print(f"Deploying {revision} (protocol {version}) to {host}:{root}", flush=True)
    with tempfile.TemporaryDirectory(prefix="tt-deploy-") as directory:
        archive = pathlib.Path(directory) / f"source-{short(revision)}.tar"
        with archive.open("wb") as output:
            run("git", "-C", str(REPOSITORY), "archive", "--format=tar", revision, stdout=output)
        archive_hash = digest(archive)
        upload_archive(host, root, archive, archive.name, archive_hash)
    run("scp", "-q", "-o", "BatchMode=yes", str(pathlib.Path(__file__).resolve()), f"{host}:{root}/{HELPER}")
    common = ["--revision", revision, "--archive-sha256", archive_hash, "--protocol-version", str(version),
              "--schema-version", str(schema)]
    remote_helper(host, root, "build", *common, *(["--rebuild"] if arguments.rebuild else []))
    remote_helper(host, root, "preflight", *common,
                  *(["--accept-migration-changes"] if arguments.accept_migration_changes else []))
    if arguments.preflight_only:
        print("Preflight passed. The live service was not changed.", flush=True)
        return
    remote_helper(host, root, "deploy", *common, *(["--keep-backup"] if arguments.keep_backup else []))
    check_health(config["base_url"], version)
    print(f"Deployed {short(revision)}; {config['base_url']} reports protocol {version}.", flush=True)


# Host side


class Host:
    def __init__(self, root, revision, archive_hash, version, schema):
        self.root = pathlib.Path(root)
        self.config = load_config((self.root / "deploy.json").read_text())
        self.revision = revision
        self.short = short(revision)
        self.archive_hash = archive_hash
        self.version = version
        self.schema = schema
        self.build = self.root / "build"
        self.archive = self.build / f"source-{self.short}.tar"
        self.candidate = self.root / "bin" / f"{BINARY}-{self.short}"
        self.manifest = self.build / f"preflight-{self.short}.json"
        self.database = self.root / self.config["database"]
        self.live = pathlib.Path(self.config["live_binary"])

    def check_archive(self):
        if digest(self.archive) != self.archive_hash:
            raise DeployError(f"{self.archive} does not match the uploaded archive")


def docker_build_command(host, source, output, uid, gid):
    script = (f"cargo build --release --locked -p {PACKAGE} && "
              f"install -o {uid} -g {gid} -m 0755 /target/release/{BINARY} /out/{output}")
    return ["docker", "run", "--rm",
            "-v", f"{source}:/src:ro",
            "-v", f"{host.build / 'cargo-home'}:/cargo-home", "-e", "CARGO_HOME=/cargo-home",
            "-v", f"{host.build / 'target'}:/target", "-e", "CARGO_TARGET_DIR=/target",
            "-v", f"{host.candidate.parent}:/out",
            "-w", "/src", host.config["rust_image"], "sh", "-c", script]


def remote_build(host, rebuild):
    host.check_archive()
    if host.candidate.exists() and not rebuild:
        print(f"Reusing built binary {host.candidate}", flush=True)
        return
    source = host.build / f"source-{host.short}"
    if source.exists():
        shutil.rmtree(source)
    source.mkdir()
    run("tar", "-xf", str(host.archive), "-C", str(source))
    staged = f".{host.candidate.name}.new"
    log_path = host.build / f"build-{host.short}.log"
    print(f"Building {PACKAGE} in {host.config['rust_image']}; log: {log_path}", flush=True)
    try:
        with log_path.open("w") as log:
            run(*docker_build_command(host, source, staged, *ids()), stdout=log, stderr=subprocess.STDOUT)
    except subprocess.CalledProcessError as error:
        raise DeployError(f"The build failed; see {log_path}") from error
    finally:
        shutil.rmtree(source)
    (host.candidate.parent / staged).replace(host.candidate)
    print(f"Built {host.candidate} ({digest(host.candidate)})", flush=True)


def ids():
    return os.getuid(), os.getgid()


def database_state(path):
    try:
        with contextlib.closing(sqlite3.connect(f"file:{path}?mode=ro", uri=True)) as connection:
            connection.execute("PRAGMA query_only=ON")
            connection.execute("BEGIN")
            return {
                "schema": connection.execute("PRAGMA user_version").fetchone()[0],
                "tasks": [list(row) for row in connection.execute(
                    "SELECT id,name,archived,created_at_us,updated_at_us FROM tasks ORDER BY id")],
                "worklogs": [list(row) for row in connection.execute(
                    "SELECT id,task_id,start_us,end_us FROM worklogs ORDER BY id")],
            }
    except sqlite3.Error as error:
        raise DeployError(f"Could not read the database {path}: {error}") from error


def fingerprint(state):
    return hashlib.sha256(json.dumps(state, separators=(",", ":")).encode()).hexdigest()


def row_changes(before, after):
    before, after = {row[0]: row for row in before}, {row[0]: row for row in after}
    return {"removed": len(before.keys() - after.keys()), "added": len(after.keys() - before.keys()),
            "changed": sum(before[key] != after[key] for key in before.keys() & after.keys())}


def describe_changes(before, after):
    parts = [f"schema version {before['schema']} -> {after['schema']}"] if before["schema"] != after["schema"] else []
    for table in ("tasks", "worklogs"):
        counts = row_changes(before[table], after[table])
        listed = ", ".join(f"{count} {kind}" for kind, count in counts.items() if count)
        if listed:
            parts.append(f"{table}: {listed}")
    return "; ".join(parts)


def check_migration(before, after, latest, accepted):
    require(after["schema"] == latest,
            f"The candidate left schema version {after['schema']}, but its migrations end at {latest}")
    if after == before:
        return None
    summary = describe_changes(before, after)
    require(accepted, f"Startup migrated the copied database ({summary}). "
                      "Check the changes and run again with --accept-migration-changes to approve them")
    print(f"Approved migration changes: {summary}", flush=True)
    return summary


def backup_database(source_path, destination):
    try:
        with contextlib.closing(sqlite3.connect(f"file:{source_path}?mode=ro", uri=True)) as source, \
                contextlib.closing(sqlite3.connect(destination)) as target:
            source.backup(target)
            integrity = target.execute("PRAGMA integrity_check").fetchone()[0]
    except sqlite3.Error as error:
        raise DeployError(f"Could not copy the database {source_path} to {destination}: {error}") from error
    require(integrity == "ok", f"The database copy at {destination} failed its integrity check")


def get_json(base, route):
    with urllib.request.urlopen(base + route, timeout=3) as response:
        if response.status != 200:
            raise DeployError(f"{route} returned HTTP {response.status}")
        return json.load(response)


def wait_until_ready(base, version, timeout=20):
    deadline = time.monotonic() + timeout
    while True:
        try:
            if get_json(base, "/v1/health") == {"status": "ok", "protocol_version": version}:
                return
            raise DeployError(f"{base} reported an unexpected health response")
        except Exception:
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.2)


def timestamp_us(value):
    instant = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    delta = instant - datetime.datetime(1970, 1, 1, tzinfo=datetime.timezone.utc)
    return (delta.days * 86400 + delta.seconds) * 1000000 + delta.microseconds


def require(condition, message):
    if not condition:
        raise DeployError(message)


def verify(base, expected, now=None):
    state = get_json(base, "/v1/state/task-list")
    require(set(state) == {"tasks", "active_worklog", "revision"} and state["revision"],
            "The task-list state has an unexpected shape")
    catalog = sorted([task["id"], task["name"], int(task["archived"]),
                      timestamp_us(task["created_at"]), timestamp_us(task["updated_at"])]
                     for task in state["tasks"])
    require(catalog == expected["tasks"], "The served task catalog differs from the database")
    active_rows = [row for row in expected["worklogs"] if row[3] is None]
    active = state["active_worklog"]
    if active_rows:
        require(len(active_rows) == 1 and active is not None, "The running worklog is missing")
        served = [active["id"], active["task_id"], timestamp_us(active["start"]), active["end"]]
        require(served == active_rows[0], "The served running worklog differs from the database")
    else:
        require(active is None, "The server reports a running worklog that the database lacks")
    now = now or utc_now()
    start = now.replace(hour=0, minute=0, second=0, microsecond=0)
    query = urllib.parse.urlencode({"start": start.isoformat(),
                                    "end": (start + datetime.timedelta(days=1)).isoformat(),
                                    "now": now.isoformat()})
    overview = get_json(base, "/v1/state/daily-overview?" + query)
    report = get_json(base, "/v1/reports/task-totals?" + query)
    require(overview["tasks"] == state["tasks"], "The daily overview tasks differ from the task list")
    require(overview["active_worklog"] == active, "The daily overview timer differs from the task list")
    require({key: value for key, value in overview.items() if key not in {"tasks", "active_worklog"}} == report,
            "The daily overview totals differ from the report route")
    require(overview["revision"] == state["revision"], "The daily overview revision differs from the task list")
    require(overview["total_us"] == sum(row["duration_us"] for row in overview["rows"]),
            "The daily overview total differs from its rows")
    require(get_json(base, "/v1/tasks")["tasks"] == state["tasks"], "The task route differs from the task list")
    require(get_json(base, "/v1/tracking")["active_worklog"] == active, "The tracking route differs from the task list")
    require(set(get_json(base, "/v1/worklogs")) == {"worklogs", "next_cursor", "revision"},
            "The worklog route has an unexpected shape")


def free_port():
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        return reservation.getsockname()[1]


def started_state(base, version, database):
    wait_until_ready(base, version)
    return database_state(database)


def smoke_test(host, directory, copied):
    port = free_port()
    with (directory / "server.log").open("w") as log:
        server = subprocess.Popen([str(host.candidate), "serve", "--bind", f"127.0.0.1:{port}",
                                   "--db", str(copied)], stdout=log, stderr=log)
        try:
            base = f"http://127.0.0.1:{port}"
            after = started_state(base, host.version, copied)
            verify(base, after)
            require(database_state(copied) == after, "Reads changed the copied database")
            return after
        finally:
            server.terminate()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()


def remote_preflight(host, accept_changes):
    host.check_archive()
    require(host.candidate.exists(), f"{host.candidate} has not been built")
    directory = pathlib.Path(tempfile.mkdtemp(prefix=f"smoke-{host.short}-", dir=host.build))
    copied = directory / "tt-server.db"
    try:
        backup_database(host.database, copied)
        before = database_state(copied)
        require(before["schema"] <= host.schema,
                f"The database has schema version {before['schema']}, newer than the candidate's {host.schema}")
    except BaseException as error:
        try:
            shutil.rmtree(directory)
        except OSError:
            raise DeployError(f"{error}. Could not remove the preflight directory {directory}") from error
        raise
    try:
        after = smoke_test(host, directory, copied)
    except Exception as error:
        raise DeployError(f"{error}. The server log and database copy are in {directory}") from error
    shutil.rmtree(directory)
    changes = check_migration(before, after, host.schema, accept_changes)
    manifest = {"commit": host.revision, "sha256": digest(host.candidate),
                "source_archive_sha256": host.archive_hash, "protocol_version": host.version,
                "schema_version": host.schema, "database_before_sha256": fingerprint(before),
                "database_after_sha256": fingerprint(after), "migration_changes": changes,
                "preflight_tasks": len(after["tasks"]), "preflight_worklogs": len(after["worklogs"]),
                "preflight_at": utc_now().isoformat()}
    host.manifest.write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"preflight": "passed", **manifest}), flush=True)


def sudo(*arguments):
    run("sudo", "-n", *arguments)


def install_binary(source, staged, live):
    sudo("install", "-o", "root", "-g", "root", "-m", "0755", str(source), staged)
    sudo("mv", staged, str(live))


def restore_database(backup, live, expected):
    backup_database(backup, live)
    require(database_state(live) == expected, f"The restored database differs from {backup}")


def read_preflight_record(path):
    try:
        record = json.loads(path.read_text())
    except FileNotFoundError as error:
        raise DeployError(f"{path} does not exist; run the preflight first") from error
    except json.JSONDecodeError as error:
        raise DeployError(f"The preflight record {path} is not valid JSON ({error}); run the preflight again") \
            from error
    require(isinstance(record, dict), f"The preflight record {path} must contain a JSON object")
    missing = sorted({"commit", "sha256", "database_before_sha256", "database_after_sha256"} - set(record))
    require(not missing, f"The preflight record {path} is missing: {', '.join(missing)}; run the preflight again")
    return record


def remote_deploy(host, keep_backup):
    manifest = read_preflight_record(host.manifest)
    require(manifest["commit"] == host.revision and manifest["sha256"] == digest(host.candidate),
            "The preflight record does not match the candidate binary; run the preflight again")
    require(os.access(host.database, os.W_OK) and os.access(host.database.parent, os.W_OK),
            f"{host.database} and its directory must be writable to restore the database on rollback")
    service = host.config["service"]
    staged = f"{host.live}.new-{host.short}"
    require(not pathlib.Path(staged).exists(), f"{staged} already exists")
    backup = host.root / "backups" / f"deploy-{host.short}-{utc_now().strftime('%Y%m%dT%H%M%SZ')}"
    backup.mkdir(mode=0o700, parents=True)
    shutil.copy2(host.live, backup / f"{BINARY}.previous")
    sudo("install", "-o", "root", "-g", "root", "-m", "0755", str(host.candidate), staged)
    stopped = replaced = False
    try:
        sudo("systemctl", "stop", service)
        stopped = True
        backup_database(host.database, backup / host.database.name)
        before = database_state(host.database)
        (backup / "database.before.json").write_text(json.dumps(before, indent=2) + "\n")
        require(fingerprint(before) == manifest["database_before_sha256"],
                "The live database changed after the preflight; run the preflight again")
        sudo("mv", staged, str(host.live))
        replaced = True
        sudo("systemctl", "start", service)
        after = started_state(host.config["base_url"], host.version, host.database)
        require(fingerprint(after) == manifest["database_after_sha256"],
                "Startup changed the live database differently from the preflight")
        verify(host.config["base_url"], after)
        require(database_state(host.database) == after, "Reads changed the live database")
        sudo("systemctl", "is-active", "--quiet", service)
        require(digest(host.live) == manifest["sha256"], "The live binary differs from the candidate")
    except BaseException:
        try:
            if stopped:
                if replaced:
                    sudo("systemctl", "stop", service)
                    restore_database(backup / host.database.name, host.database, before)
                    install_binary(backup / f"{BINARY}.previous", staged, host.live)
                sudo("systemctl", "start", service)
            if not replaced:
                sudo("rm", "-f", staged)
        except BaseException:
            print(f"Deployment failed and the rollback did not finish. The backup is in {backup}",
                  file=sys.stderr, flush=True)
            raise
        print(f"Deployment failed and was rolled back. The backup is in {backup}", file=sys.stderr, flush=True)
        raise
    result = {"deployment": "passed", **manifest, "deployed_at": utc_now().isoformat(),
              "backup_directory": str(backup), "tasks": len(after["tasks"]), "worklogs": len(after["worklogs"])}
    if not keep_backup:
        shutil.rmtree(backup)
        result["backup_files_removed_at"] = utc_now().isoformat()
    record = json.dumps(result, indent=2) + "\n"
    (host.build / f"deployment-{host.short}.json").write_text(record)
    (host.build / "deployment-latest.json").write_text(record)
    print(json.dumps(result), flush=True)


# Command line


def workstation_parser():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--host", required=True, help="SSH destination of the server host")
    parser.add_argument("--root", required=True, help="Deployment directory on the host, containing deploy.json")
    parser.add_argument("--revision", default="HEAD", help="Git revision to deploy, default HEAD")
    parser.add_argument("--preflight-only", action="store_true",
                        help="Build and check the candidate without changing the live service")
    parser.add_argument("--rebuild", action="store_true", help="Build again even if the candidate binary exists")
    parser.add_argument("--accept-migration-changes", action="store_true",
                        help="Approve the schema and row changes that the candidate's migrations make")
    parser.add_argument("--keep-backup", action="store_true",
                        help="Keep the previous binary and database backup after a successful deployment")
    return parser


def host_parser():
    parser = argparse.ArgumentParser(prog="deploy-server.py remote")
    parser.add_argument("step", choices=["build", "preflight", "deploy"])
    parser.add_argument("--root", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--archive-sha256", required=True)
    parser.add_argument("--protocol-version", required=True, type=int)
    parser.add_argument("--schema-version", required=True, type=int)
    parser.add_argument("--rebuild", action="store_true")
    parser.add_argument("--accept-migration-changes", action="store_true")
    parser.add_argument("--keep-backup", action="store_true")
    return parser


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    try:
        if argv[:1] == ["remote"]:
            arguments = host_parser().parse_args(argv[1:])
            host = Host(arguments.root, arguments.revision, arguments.archive_sha256, arguments.protocol_version,
                        arguments.schema_version)
            if arguments.step == "build":
                remote_build(host, arguments.rebuild)
            elif arguments.step == "preflight":
                remote_preflight(host, arguments.accept_migration_changes)
            else:
                remote_deploy(host, arguments.keep_backup)
        else:
            deploy_from_workstation(workstation_parser().parse_args(argv))
    except (DeployError, subprocess.CalledProcessError, OSError, json.JSONDecodeError) as error:
        print(f"Deployment stopped: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
