# Deploy a server

`scripts/deploy-server.py` updates a `tt-server` instance that runs under systemd on another machine. Run it from a checkout on your workstation. It reaches the host through SSH, so the SSH destination must log in without a password prompt.

```sh
python3 scripts/deploy-server.py --host HOST --root /path/to/deployment
```

The script deploys `HEAD` by default; pass `--revision REV` to choose another commit. Only committed work is deployed, because the source comes from `git archive`.

## What the script does

1. Archives the revision and uploads it to `ROOT/build/source-<rev>.tar`. It reuses an identical archive and refuses to replace a different one.
2. Uploads itself to `ROOT/build/deploy-server.py` and runs the remaining steps on the host.
3. Builds the `tt-server` binary of `tracker-server-app` in the configured Rust container. Docker is only the build tool; the service runs the binary directly. Cargo's registry and target directories are cached in `ROOT/build/cargo-home` and `ROOT/build/target`. The candidate is written to `ROOT/bin/tt-server-<rev>`, and the build log to `ROOT/build/build-tt-server-<rev>.log`. An existing `tt-server-<rev>` candidate is reused unless you pass `--rebuild`. Old `tt-<rev>` candidates are never reused.
4. Runs a preflight. The candidate starts with `tt-server --bind 127.0.0.1:PORT --db PATH`. It serves a copy of the live database on a local port and runs migrations on startup. The script compares the task catalog, the running worklog, the daily overview, the report totals, and the task, tracking and worklog routes against the migrated rows. It also checks that the health route reports the protocol version of the revision and that the schema version ends at the revision's last migration. If startup changed the schema version or any task or worklog row, the preflight stops and prints a summary such as `schema version 7 -> 8; worklogs: 1 removed`. See [Migrations](#migrations). The record is written to `ROOT/build/preflight-<rev>.json`, with SHA-256 fingerprints of the database before and after startup. The copy and the candidate's `tt-server.log` live in `ROOT/build/smoke-<rev>-*`. The script removes that directory after a passed check. If the candidate fails to start or a check fails, the directory stays and the error names it, so you can read the log. Delete it once you are done.
5. Deploys. It refuses to continue if a staged binary from an earlier run still sits next to the live binary. It backs up the live binary, stages the candidate next to it, stops the service, backs up the database, and checks that the database still matches the preflight's "before" fingerprint. It then swaps in the candidate, starts the service, checks that the migrated database matches the "after" fingerprint, and runs the same route checks against the live server. If any step fails after the swap, it stops the service, restores the database from the backup, reinstalls the previous binary, and starts the service again. The backup stays in `ROOT/backups/deploy-<rev>-<time>`.
6. Removes the backup after a successful deployment, unless you pass `--keep-backup`. The result is written to `ROOT/build/deployment-<rev>.json` and `ROOT/build/deployment-latest.json`.
7. Checks the health route from your workstation.

Use `--preflight-only` to build and check a revision without changing the live service.

## Migrations

The server migrates its database when it opens it, so a revision with a new migration changes the schema version and can change rows. Migration 7, for example, deletes zero-duration worklogs. The preflight shows these changes on the copy and refuses to continue until you approve them:

```sh
python3 scripts/deploy-server.py --host HOST --root /path/to/deployment --preflight-only
python3 scripts/deploy-server.py --host HOST --root /path/to/deployment --accept-migration-changes
```

The first command shows the summary without touching the live service. The second records the approved summary and fingerprints in the preflight record and deploys. The deploy step requires the live database to produce exactly the approved result. If someone writes to the server between the preflight and the service stop, the "before" fingerprint no longer matches, and the deploy stops before it swaps the binary. Run the script again to take a new preflight.

The old binary rejects a database with a newer schema version. On rollback, the script restores the backup through SQLite's backup API while the service is stopped, so the restored file keeps its owner and permissions, and SQLite handles any journal or WAL files. The SSH user needs write access to the database file and its directory; the deploy step checks this before it stops the service. If the restore fails, the service stays stopped and the script prints the backup directory, so you can restore it by hand.

The deploy step uses `sudo -n` to install the binary and control the service, so the SSH user needs passwordless sudo for those commands. The preflight and deploy checks read the SQLite tables directly, and the script counts the entries in `MIGRATIONS` in `crates/tracker-storage/src/migrate.rs` to find the expected schema version. Update the script when the table columns, the migration list, or the checked routes change.

## Protocol 6 cutover

Protocol 6 removes the server revision from writes and reads. The server and every remote client must run protocol 6, so upgrade them together; mismatched versions fail the health check. A protocol-5 write that still carries `expected_revision` fails with HTTP 400 `invalid_request`. No database migration is needed.

Saved CLI history cursors keep working. Saved remote archive previews contain `revision` and fail to load, so generate a new preview after the upgrade.

The server keeps its receipt cache in memory, so the restart during deployment clears it. The cache holds at most 1,024 successful receipts, and eviction also ends replay protection for older requests. After that, a client that resends an old command runs it again if its expected values still match. Check current state after an uncertain outcome before retrying. [ADR 0030](adr/0030-remove-server-revisions-from-writes-and-reads.md) describes this accepted limit with an example.

## Host configuration

To set up a new host, create `ROOT` and `ROOT/deploy.json`. The script creates `ROOT/build` and `ROOT/bin` on its first run. The host keeps its settings in `deploy.json`, so this repository holds no host addresses:

```json
{
  "service": "time-tracker.service",
  "live_binary": "/usr/local/bin/tt",
  "database": "data/tt-server.db",
  "base_url": "http://<tailscale-ip>:8765",
  "rust_image": "rust:1.98.1-bookworm"
}
```

- `service`: the systemd unit that runs the HTTP server.
- `live_binary`: the binary path in the unit's `ExecStart`.
- `database`: the server database, relative to `ROOT`.
- `base_url`: the address clients use. The server rejects DNS names in the Host header, so use the bind IP address.
- `rust_image`: the container image used for the build. Pick one whose C library matches the host's.

## Existing units and rollback

The script installs `tt-server` at the configured `live_binary` path, even if that path is named `tt`. Existing units can keep `ExecStart=<live_binary> serve --bind ADDRESS:PORT --db PATH`. The new binary accepts the leading `serve` word as a compatibility form with the same validation and defaults. This preserves both deployment and rollback to the previous TUI binary without editing the unit.

For new units, use `ExecStart=<live_binary> --bind ADDRESS:PORT --db PATH` and a path such as `/usr/local/bin/tt-server`. A rollback to an older TUI binary requires the compatibility form in the unit. Running `tt serve` from the terminal now reports that the command moved to `tt-server`.
