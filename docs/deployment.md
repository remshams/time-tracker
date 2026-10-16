# Deploy a server

`scripts/deploy-server.py` updates a `tt serve` instance that runs under systemd on another machine. Run it from a checkout on your workstation. It reaches the host through SSH, so the SSH destination must log in without a password prompt.

```sh
python3 scripts/deploy-server.py --host HOST --root /path/to/deployment
```

The script deploys `HEAD` by default; pass `--revision REV` to choose another commit. Only committed work is deployed, because the source comes from `git archive`.

## What the script does

1. Archives the revision and uploads it to `ROOT/build/source-<rev>.tar`. It reuses an identical archive and refuses to replace a different one.
2. Uploads itself to `ROOT/build/deploy-server.py` and runs the remaining steps on the host.
3. Builds the `tt` binary of `tracker-tui` in the configured Rust container. Docker is only the build tool; the service runs the binary directly. Cargo's registry and target directories are cached in `ROOT/build/cargo-home` and `ROOT/build/target`. The candidate is written to `ROOT/bin/tt-<rev>`, and the build log to `ROOT/build/build-<rev>.log`. An existing candidate is reused unless you pass `--rebuild`.
4. Runs a preflight. The candidate serves a copy of the live database on a local port. The script compares the task catalog, the running worklog, the daily overview, the report totals, and the task, tracking and worklog routes against the copied rows. It also checks that the health route reports the protocol version of the revision. The record is written to `ROOT/build/preflight-<rev>.json`.
5. Deploys. It backs up the live binary and the database, stops the service, installs the candidate, starts the service, and runs the same checks against the live server. It then confirms that every task and worklog row is unchanged. If any step fails, it restores the previous binary and starts the service again; the backup stays in `ROOT/backups/deploy-<rev>-<time>`.
6. Removes the backup after a successful deployment, unless you pass `--keep-backup`. The result is written to `ROOT/build/deployment-<rev>.json` and `ROOT/build/deployment-latest.json`.
7. Checks the health route from your workstation.

Use `--preflight-only` to build and check a revision without changing the live service.

The deploy step uses `sudo -n` to install the binary and control the service, so the SSH user needs passwordless sudo for those commands. The preflight and deploy checks read the SQLite tables directly. Update the script when the schema or the checked routes change.

## Host configuration

The host keeps its settings in `ROOT/deploy.json`, so this repository holds no host addresses:

```json
{
  "service": "time-tracker.service",
  "live_binary": "/usr/local/bin/tt",
  "database": "data/tt-server.db",
  "base_url": "http://<tailscale-ip>:8765",
  "rust_image": "rust:1.98.1-bookworm"
}
```

- `service`: the systemd unit that runs `tt serve`.
- `live_binary`: the binary path in the unit's `ExecStart`.
- `database`: the server database, relative to `ROOT`.
- `base_url`: the address clients use. The server rejects DNS names in the Host header, so use the bind IP address.
- `rust_image`: the container image used for the build. Pick one whose C library matches the host's.
