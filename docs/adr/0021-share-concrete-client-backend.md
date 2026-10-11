# 0021: share the concrete client backend

## Status

Accepted. Partially supersedes ADR 0010's separate production local and remote TUI executors. Its request and outcome ownership rules remain accepted.

## Date

2026-10-11

## Context

The CLI, TUI and Swift bridge each selected a local application or remote application and repeated their task, tracking, history and report dispatch. Report DTO conversion and inactive-task preview reads also appeared in several clients. These copies made a shared operation change require several client edits.

Each client still has different presentation and lifecycle needs. The TUI polls requests beside terminal input. The bridge serializes calls through its owned current-thread runtime and classifies errors for Swift. The CLI validates input before opening a backend and preserves its JSON schema and exit codes.

## Decision

- Add `tracker-client` with one concrete `TrackerBackend` enum for `TrackerApplication<SqliteRepository>` or `RemoteApplication`. Its async methods dispatch directly to the selected backend. Capability modules group task, tracking, history and report methods.
- Share operation arguments, reviewed-name and reviewed-active variants, selected resource refreshes, cached observations, report conversion and inactive preview reads. Preserve remote failure origin, write guards and command receipts. Never select local storage after a remote failure.
- Return report rows with typed task IDs and durations. Optional label resolution stays with the TUI, while the CLI can require labeled totals. The bridge retains its report JSON format.
- Keep endpoint and database selection, preview-token validation, C ABI ownership, failure presentation, refresh scheduling and client runtimes in the adapters. The synchronous application and repository contracts gain no Tokio dependency.
- Use the shared backend for production TUI requests. Retain the synchronous application-service executor only for existing presentation tests with `TestService`.

## Consequences

The clients depend on another small workspace crate, but common backend dispatch has one owner. The facade uses no backend registry, async trait, dependency-injection container or local fallback. Local operations still finish synchronously when their futures are first polled.

The Swift bridge now owns a current-thread runtime for local handles too, so both selected backends use the same call path. Runtime construction errors return through the existing open failure. Its serial worker still owns handle disposal, and the refactor starts no background task.

Shared workflow tests exercise both SQLite and a real HTTP server. Existing CLI, bridge and TUI tests retain their output, stale-intent, missing-metadata and lifecycle assertions. The request-and-outcome split and cancellation rules from ADR 0010 remain unchanged.
