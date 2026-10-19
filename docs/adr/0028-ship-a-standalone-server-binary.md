# ADR 0028: Ship a standalone server binary

## Status

Accepted

## Date

2026-10-19

## Context

The HTTP server owns a separate SQLite database, but `tt serve` starts it from the terminal client. Headless deployments build and ship ratatui and crossterm with the service. This couples server packaging to terminal UI dependencies.

Existing systemd units use `ExecStart=<live_binary> serve --bind ADDRESS:PORT --db PATH`. The deployment script replaces that configured executable path and can restore the previous binary after a failed deployment.

## Decision

Ship `tt-server` from a new workspace package, `tracker-server-app` in `apps/server`. It owns command parsing and database selection, then calls `tracker_server::run`. It depends on `tracker-server` and `tracker-storage`, with no terminal UI dependency.

The canonical command is `tt-server --bind ADDRESS:PORT [--db PATH]`. Preserve explicit loopback or Tailscale bind validation and the default `tt-server.db` beside the local client's `tt.db`. Reject missing values, empty database paths, unknown flags, and repeated flags.

Accept one optional leading `serve` word with identical parsing, validation, and defaults. Document this compatibility form in help. Existing units retain their arguments through deployments and rollbacks; the script installs the new server at the configured `live_binary` path. Preflight uses the canonical command. Candidate and log names identify `tt-server`, so an old TUI candidate is not reused.

Remove server startup and the production `tracker-server` dependency from `tracker-tui`. `tt serve` exits with an error directing the user to `tt-server`. TUI remote E2E tests retain a test-only dependency and start the server library in a child test process. Native UI fixtures build and launch `tt-server`.

## Consequences

Server deployments build a headless executable. The server library continues to own HTTP behavior, its multithreaded runtime, application state, and SQLite database. Protocol and schema versions are unchanged.

Install `tt-server` separately when running a server. New units can use the canonical command; units that must roll back to an older TUI binary retain the leading `serve` word. Keep compatibility parsing covered by command and process tests.
