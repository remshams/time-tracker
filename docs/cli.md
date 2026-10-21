# Command line interface

`tt-cli` exposes the TUI's task, timer, worklog, and report operations to scripts and external agents. Each invocation executes one command and exits. It never opens an interactive terminal or asks for input.

Build or install it with:

```sh
cargo run -p tracker-cli -- --help
cargo install --path apps/cli
```

## Storage and output

Without storage options, `tt-cli` uses the TUI's local `tt.db`. New local and server databases start with no tasks. Existing data remains intact. Use `--db PATH` for a different local database or `--server URL` for a running `tt-server` instance. These options are mutually exclusive. Remote commands never fall back to local storage. An explicit database path requires an existing parent directory. Database paths must be valid UTF-8 so backend tokens identify them without replacing filename bytes.

```sh
tt-cli tasks list
tt-cli --db /tmp/agent.db tasks create "Build release"
tt-cli --server http://127.0.0.1:8765 tracking status
```

Successful commands print one JSON object to stdout:

```json
{"ok":true,"data":{}}
```

Command failures print one JSON object to stderr and leave stdout empty. Help and version requests print plain text and exit successfully. Consumers should branch on the error code and exit status, rather than matching the message. If stdout fails during a write, it may contain partial output; the CLI returns status 4 and reports the output failure through stderr when that stream is still writable. A mutation may already have committed before an output failure.

| Exit status | Meaning |
|---|---|
| 0 | Success, help, or version |
| 2 | Invalid command or input |
| 3 | Application rule or concurrency conflict |
| 4 | Storage or output failure |
| 5 | Remote endpoint, connection, or protocol failure |

Errors include `code`, `message`, and `recovery_failed`. Codes include `invalid_input`, `operation_failed`, `storage`, `remote`, `worklog_not_found`, `worklog_changed`, `worklog_history_changed`, `worklog_overlap`, `active_worklog`, `active_task`, and `inactive_candidates_changed`. If recovery also fails, the original conflict code remains available and `recovery_failed` becomes true. The exit status then reflects the storage or remote failure.

Identifiers are UUID strings. Timestamps are RFC3339 instants with explicit UTC offsets, such as `2026-10-04T09:00:00Z` or `2026-10-04T11:00:00+02:00`. Storage preserves microsecond precision. Keep the returned timestamp values when constructing concurrency guards. Bare local timestamps are rejected.

The global `--at TIME` option supplies the command's operation time instead of the current instant. It also sets the report's current time and the inactive-task preview time. Bulk archive confirmation retains the token's original preview time. The application still rejects invalid intervals and tracking transitions.

## Tasks

```text
tt-cli tasks list [--state active|archived|all] [--sort worked|updated|created] [--search QUERY]
tt-cli tasks get TASK_ID
tt-cli tasks create NAME
tt-cli tasks rename TASK_ID NAME
tt-cli tasks archive TASK_ID
tt-cli tasks restore TASK_ID
tt-cli tasks preview-inactive
tt-cli tasks archive-inactive --preview JSON_OR_FILE --yes
```

Listing defaults to active tasks ordered by most recently worked. Search uses the TUI's case-insensitive subsequence matching, so `bre` matches `Build release`. Search results use recent activity order, regardless of `--sort`. Mutations select tasks by identifier, so duplicate names are safe. `rename` and `archive` send no expected name and act on the task's current state in local and server mode, so another client's rename in between does not cause a conflict.

`preview-inactive` returns a token for tasks with no work in the preceding 14 days whose creation and metadata update are also older than 14 days. A running timer protects its task. Pass the returned token unchanged to `archive-inactive --preview`, either as a JSON argument or as `@PATH` to a file containing the token. The token belongs to its selected backend and retains the preview time. Local tokens also retain the candidate IDs. `--yes` explicitly confirms the bulk archive. A malformed token fails with `invalid_input` and exit status 2 before the CLI opens the database or contacts the server. A remote token fails this way when its preview time is not microsecond-canonical, its count differs from its candidate IDs, a candidate ID is invalid or repeated, or its period is not 14 days. Server mode doesn't send a remote token's candidate IDs, so changing one valid ID to another goes undetected. A remote token saved under protocol 5 still contains `revision` and fails the same way, so generate a new preview.

Archiving rechecks eligibility in one transaction at the token's original preview time, as the TUI does. Waiting to confirm does not move the eligibility window. Running timers and recorded positive-duration work starting at or after the preview time prevent affected tasks from qualifying. Local mode requires the candidate set to match the preview, and confirmation cannot precede the preview time. Server mode archives the tasks that match the token's preview time and period when the command runs. It reports the actual archived count, which can differ from the preview if another window or machine changed tasks or worklogs in between. This trade-off is accepted. If confirmation fails, generate a new preview. Remote previews expire when their preview time is more than 15 minutes from the server clock. Restoring a task preserves its ID and worklogs.

Remote clients and servers require `/v1` protocol version 6. Update every client and the server together, then regenerate saved remote preview tokens. Saved history cursors remain valid. Health checks reject incompatible versions before loading resources.

## Tracking

```text
tt-cli tracking status
tt-cli tracking start TASK_ID
tt-cli tracking stop --expected-active WORKLOG_ID
```

`start` sets the desired active task. In server mode, it first reads the current timer and sends it as the expected active worklog, so a timer change by another client in between causes a conflict. It starts a timer while idle, atomically stops the old timer and starts another when switching tasks, and leaves the timer running if that task is already active. Starting a previously stopped task creates a new worklog.

Read the running worklog ID from `status` and pass it to `stop`. Running status also reports elapsed seconds at its `as_of` instant. A different active worklog causes a conflict instead of stopping another client's timer. Repeating a stop after tracking becomes idle succeeds without another write. Exiting any CLI command leaves a running timer active, just as quitting the TUI does.

## Worklogs

```text
tt-cli worklogs list [--task TASK_ID] [--cursor JSON]
tt-cli worklogs correct WORKLOG_ID --expected-start TIME --expected-end TIME_OR_RUNNING [--start TIME] [--end TIME_OR_RUNNING]
tt-cli worklogs move WORKLOG_ID DESTINATION_TASK_ID --expected-task TASK_ID --expected-start TIME --expected-end TIME_OR_RUNNING
tt-cli worklogs delete WORKLOG_ID --expected-task TASK_ID --expected-start TIME --expected-end TIME_OR_RUNNING --yes
```

Without `--task`, history includes all active and archived tasks. Each page contains at most 50 worklogs, newest first. Pass the returned `next_cursor` JSON unchanged as `--cursor` to load older rows. A null cursor marks the end. Cursors retain the anchor ID and ordering revision in their task or global backend token. Storage resolves the anchor timestamp; equal starts use ID ascending as the tie-break. A deleted or moved-out anchor requires a fresh first page. If another client invalidates a cursor, restart with the newest page.

For mutations, copy the expected start and end from the previously read worklog. Pass `running` when the old end is null. Moves and deletion also require the previously read task ID. The application compares these values with storage before writing. A stale selection fails instead of replacing the guard with fresh values.

Correction preserves any omitted replacement field and cannot change whether a worklog is running. Moving preserves the worklog's ID, timestamps, and running state. The application rejects same-task overlap and archived destinations. Completed worklogs can be permanently deleted with `--yes`; running worklogs cannot be deleted.

## Reports

```text
tt-cli reports [today|yesterday|week|month|year] [--timezone IANA_NAME]
tt-cli reports --from YYYY-MM-DD --to YYYY-MM-DD [--timezone IANA_NAME]
tt-cli reports --start RFC3339 --end RFC3339
```

Reports default to today. Presets cover the selected local calendar period, including Monday through Sunday for a week and the full calendar month or year. Date ranges include both supplied dates. The application converts calendar dates to a half-open UTC interval using the selected timezone, so daylight-saving days retain their actual length. Explicit timestamp ranges include the start and exclude the end.

Timezone selection uses `--timezone`, a valid `TZ` IANA name, the system timezone, then UTC. Use `--timezone` for repeatable automation across machines. Report output includes the effective interval, timezone, per-task durations, and total duration in integer microseconds. Reports include archived tasks and clip running work at the command's current time.

## Recovery and retries

On a concurrency conflict, read the current task, timer, or history before preparing a new command. Do not replace old values automatically just to make a mutation pass.

A remote write may commit even if its response is lost. A failed state refresh after a local write can also leave the operation committed. Check current state after an unconfirmed outcome before repeating a mutation, especially task creation. The remote transport resends a failed write once with the identical body and request ID. The server replays a successful receipt only while it stays in its in-memory cache of 1,024 receipts. After a server restart or eviction, an old request can run again if its expected values still match. A new CLI invocation is a new request and does not provide cross-invocation deduplication. See [ADR 0031](adr/0031-remove-server-revisions-from-writes-and-reads.md).

The CLI uses the server's existing trust model. See [remote mode](../README.md#remote-mode) for binding and network requirements.

Completed worklogs require an end strictly later than their start after microsecond canonicalization. A stop or switch at the active start fails and keeps the timer running. Schema migration 7 deletes existing zero-duration worklogs. They cover no time, so totals do not change. Back up the database before upgrading if you want to keep those entries. See [ADR 0024](adr/0024-id-only-worklog-cursors-and-positive-durations.md) for cursor compatibility and migration details. Regenerate saved history cursors and remote preview tokens after upgrading all clients and the server.
