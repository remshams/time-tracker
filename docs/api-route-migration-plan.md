# API route refactoring and migration plan

Implement [ADR 0017](adr/0017-separate-http-resources-from-client-refreshes.md) without changing tracking or worklog domain rules. Split HTTP resource reads, remove automatic snapshot attachments and migrate the CLI, TUI and macOS clients to explicit resource composition.

## Status and scope

Updated on 9 October 2026 after acceptance of a coordinated server-and-client cutover. The source basis is `main` revision `2d8b96cff1d097c18c31e06eadbd665616c0a54d`. Basic application E2E coverage is merged and passing on the previous `/v1` contract. Production still uses protocol 2; implementation will replace its responses under `/v1` and require protocol 3. This branch contains documentation only. The ADR owns the target contract. This plan owns the implementation sequence, client changes and acceptance checks.

Keep local SQLite mode, offline rejection in remote mode, existing search/ranking policies, tracking timestamps and domain validation. Introduce no database schema change, task deletion, manual worklog creation, push subscription or combined convenience read.

## Route mapping

| # | Existing contract | Target contract |
|---|---|---|
| 1 | `GET /v1/health` | `GET /v1/health`, protocol number 3 |
| 2 | `GET /v1/snapshot` | Independent `GET /v1/tasks` and `GET /v1/tracking`; fetch only what the feature needs |
| 3 | Cached task lookup after snapshot | `GET /v1/tasks/{id}` or the task cache |
| 4 | `GET /v1/tasks/{id}/worklogs` | `GET /v1/worklogs?task_id={id}` |
| 5 | `GET /v1/worklogs` | `GET /v1/worklogs` |
| 6 | Cached worklog lookup | `GET /v1/worklogs/{id}` for inspection or guarded preflight |
| 7 | `GET /v1/reports` | `GET /v1/reports/task-totals`, with IDs and durations only |
| 8 | `GET /v1/tasks/inactive-preview` and `GET /v1/tasks/inactive-candidates` | One `GET /v1/tasks/inactive-preview`, optional `inactive_days` defaulting to 14; complete candidate IDs and separately resolved task metadata |
| 9 | `POST /v1/tasks/archive-inactive` and `POST /v1/tasks/archive-inactive-candidates` | One `POST /v1/tasks/archive-inactive`, reusing original preview time, period and revision |
| 10 | Other existing guarded writes | Same command routes; narrow command receipts replace snapshot responses |

Remove `/v1/tasks/{id}/worklogs`, `/v1/reports`, `/v1/snapshot`, `/v1/tasks/inactive-candidates` and `/v1/tasks/archive-inactive-candidates` at cutover. Keep the `/v1` prefix for the replacement contract; add no legacy aliases or parallel API version. Route mapping compares protocol 2 with protocol 3, not two simultaneously supported contracts.

## Prerequisite: basic application E2E coverage

Complete. The baseline was merged independently in [PR #10](https://github.com/remshams/time-tracker/pull/10) at revision `80f0daacbdb9e0c5a9137c09bdedeeb837736c16`. Its [main CI run](https://github.com/remshams/time-tracker/actions/runs/37927003001) passed all six native macOS groups, Rust workspace tests on Linux and macOS, portable Swift tests, Python tests, coverage and CRAP checks against `/v1` protocol 2.

The [native coverage map](../apps/swiftui/e2e-coverage.md) records task creation/display, start/switch/stop, history, daily totals, refresh/reconnect, relaunch persistence and retained bulk archive scenarios in local and real-server modes. Fixtures drive the real app and production bridge, use isolated preferences and databases, and verify server/storage outcomes. Existing TUI E2E tests remain part of workspace CI. Native diagnostics include logs and Xcode result bundles.

This evidence satisfies the start prerequisite for protocol/server changes, cache refactoring and client migration. Select a passing source revision for implementation; the latest `main` build at `2d8b96c` was queued when this plan was updated. Keep the existing E2E baseline as regression coverage. This documentation change does not modify E2E tests; obtain explicit consent before implementation changes them.

Migration-specific E2E scenarios for stale previews, concurrent changes, receipt recovery and `/v1` resource composition are part of implementation acceptance. Extend the established baseline for those cases as the affected features migrate.

## Implementation sequence

### 1. Protocol and server reads

- Replace DTOs in `tracker-protocol` and bump `VERSION` from 2 to 3. Preserve strict decoding and the health response shape; server and all remote clients must agree on protocol 3. Do not retain protocol-2 DTOs or add a second route version.
- Add task collection, single-task, tracking, worklog collection, single-worklog and task-total report handlers in `tracker-server`.
- Flatten task activity metadata into task responses. Remove snapshot, global tracking and catalog fields from history and reports.
- Capture each response's data and revision under the same operation lock. Keep repository transaction boundaries for report calculations and coherent domain reads.
- Dispatch the worklog collection to existing task or global history queries according to optional `task_id`. Validate task existence before returning filtered history.
- Preserve the 50-entry page bound, current ordering and all-or-none continuation parameters. Bind cursors to their original collection/filter, reject cross-filter reuse and reset client pagination when `task_id` changes. The existing task and global cursor models can stay internal; include nullable `task_id` in the continuation DTO. Filtered continuations also submit `after_task_id`; require it to match the current `task_id`, and reject it on unfiltered requests. Reject incomplete or mismatched scope rather than silently applying a cursor to another collection.
- Preserve normalized report parameters, half-open clipping, running cutoff, archived-task contributions and microsecond precision.
- Consolidate both inactive preview routes. Accept `as_of` and optional positive `inactive_days` defaulting to 14; return echoed time/period, all candidate task IDs, count and revision. Preserve eligibility and checked cutoff arithmetic. Resolve complete candidate labels through task reads rather than embedding task metadata.

### 2. Commands and idempotency

- Replace `/v1` write responses while retaining the application commands, operation lock and guarded write history.
- Return original command receipts with request ID, applied revision, replay flag and focused domain outcome. Capture task activity metadata under the command lock.
- Cache the complete original receipt. A replay after another command must retain its original revision and result. Do not attach a newly observed revision to a cached result.
- Preserve command fingerprinting and request-ID reuse checks within the new contract. Restart the server at cutover to clear the in-memory cache; no cross-version deduplication is needed.
- Preserve the current successful-command cache bound and exact expected source values. A transport retry reuses the identical request body, ID and captured timestamp.
- Consolidate both bulk archive commands. Confirmation uses the reviewed preview's original time, period and revision, recomputes eligibility and returns the actual archived count. Keep active-task protection, preview expiry and no automatic resubmission after uncertain results.
- Preserve structured HTTP errors and recovery after a write that might have committed. No durable deduplication is added.

### 3. Remote adapter and cache coordination

- Split `tracker-remote` reads and caches into tasks, tracking, worklogs and reports. Associate each cache entry with the revision that produced it.
- Make health checks independent of catalog/tracking initialization. Fetch only the resources required by a command or active feature.
- Replace automatic snapshot adoption after reports, history and writes with explicit adoption of the resource actually returned.
- Use the preflight table below to construct guarded intent. Compare relevant freshly read values with the user's reviewed values before adopting a new guard. Preserve rename drafts, selected tasks, original worklog values and click timestamps.
- Bound reconciliation of mismatched resource revisions to one immediate retry. Keep marked cached state and retry later when ongoing writes prevent coherence.
- Apply a receipt immediately only while it still belongs to the in-flight command and unchanged pre-command cache. Prevent delayed receipts from overwriting newer resource reads. Reread affected resources after an uncertain write or replay.
- Mark dependent caches stale. Worklog changes can affect task activity ordering, history and reports; a bulk archive can affect the task catalog. Refresh the dependencies used by the current feature.
- Retain existing transport limits, retry count, request-size restrictions and no-fallback behavior.

| # | Command | Reads needed to form the guarded intent |
|---|---|---|
| 1 | Create task | Task catalog revision; no existing task fields are copied |
| 2 | Rename, archive or restore | Target task and its revision; the server independently enforces active-task protection |
| 3 | Start, switch or stop | Tracking; also the target task for start or switch, at the same revision |
| 4 | Correct or delete worklog | Target worklog and its revision, retaining its exact expected values |
| 5 | Move worklog | Source worklog and destination task, at the same revision |
| 6 | Archive inactive tasks | Complete preview, reviewed candidate metadata as needed, and original revision, `as_of` and `inactive_days` |

### 4. CLI and TUI

- Migrate `tracker-cli` to the resources required by each command. Tracking status needs tracking; task browsing needs tasks; reports need task totals and names only when rendered.
- Keep task search, recent/updated/created sorting, archived views and inactive-preview confirmation behavior.
- Migrate TUI refresh effects to explicit tasks and tracking reads. History uses the filtered worklog collection; reports use task totals without refreshing unrelated caches.
- Preserve current task/worklog editing flows, navigation and timestamp semantics. Continue using shared application requests in local mode.
- Exercise concurrent clients, stale guards, cursor invalidation and possibly committed writes through the migrated remote adapter.

### 5. Swift bridge and macOS client

- Migrate `tracker-swift-bridge` and the portable `TrackerClient` session to the split resource methods. In-process bridge JSON may retain a composed client view model; it need not duplicate the HTTP envelope.
- Replace report snapshot adoption with independent task, tracking and daily-total caches. Validate report row IDs, uniqueness, durations and total without requiring immediate task-cache membership.
- Resolve missing task names through task reads. Preserve valid totals while metadata is unavailable.
- Anchor local running-total projection to a matching report/tracking revision and active worklog identity. Use echoed report `now`; freeze extrapolation on a mismatch until a coherent pair arrives. The independent timer may still advance.
- Preserve sidebar, Today menu, visible/hidden refresh behavior, focus refresh, lock pause/resume and pending-write reconciliation.
- Preserve configurable bulk archive dialog behavior from ADR 0016. Resolve all candidate metadata, preserve the reviewed time/period/revision, discard cancelled or replaced previews, serialize refresh/confirmation and report the actual count. Missing candidate metadata must not silently shorten the reviewed list.
- Update portable Swift tests and the established basic E2E scenarios and migration-specific remote scenarios after obtaining consent. Run the existing native local bulk archive suite unchanged when its observable requirements remain the same. Validate native rendering and lifecycle behavior on macOS.

### 6. Documentation and rollout

- Update architecture diagrams, method tables and communication examples in the implementation changes. Label snapshots with the protocol number as well as `/v1`, because the URI prefix is retained across the breaking change. Historical examples must state protocol 2 and their source revision.
- Build and validate the server, CLI, TUI, Rust bridge and macOS client as one release requiring protocol 3. Keep health decoding stable and verify clients reject an incompatible compatibility number before adopting resource data.
- Stop remote client activity, replace the server and every supported client, then restart them together. Remove the old routes and bundled DTOs at cutover; do not deploy a server supporting both contracts.
- Verify an old client rejects the new server and a new client rejects an old server. Treat mixed-protocol operation as unsupported. Verify updated CLI, TUI and macOS clients can safely operate concurrently against the new server.
- Roll back server and clients together if needed. The database schema stays unchanged; restart discards receipt history, so retain the existing possibly-committed-write recovery rules.
- Measure request counts, bytes, refresh latency and behavior under concurrent writes after migration. Use those results to assess optional aggregate endpoints separately.
- ADR 0017 is accepted. Earlier records identify the superseded HTTP clauses; preserve their local-mode and domain rules.

## Current use-case coverage

| # | Use case | Resource composition |
|---|---|---|
| 1 | Task list, archived list, search and sorting | Tasks with recent-activity metadata; client search/sorting |
| 2 | Task creation, rename, archive and restore | Task commands and affected task/catalog refresh |
| 3 | Fixed and configurable bulk archive | One complete inactive preview with default or chosen period, task metadata resolution and guarded confirmation |
| 4 | Timer, start, switch, stop and lock pause/resume | Tracking plus target task when needed |
| 5 | Task and global history | Worklog collection with or without `task_id` |
| 6 | Worklog correction, move and deletion | Individual worklog, destination task for moves, guarded command |
| 7 | Historical reports | Task totals and metadata for returned task IDs |
| 8 | macOS sidebar and Today menu | Tasks, tracking and current-day task totals |
| 9 | Connection and compatibility checks | Health only |

## Acceptance and validation

| # | Check | Required behavior |
|---|---|---|
| 0 | Start prerequisite | Satisfied by merged PR #10 and passing main CI on `/v1` protocol 2; retain baseline regression coverage during implementation |
| 1 | Response boundaries | Reports and history have no snapshot/global attachments; writes return focused receipts; no overlapping `/v1` collection aliases |
| 2 | Filtered history | Filtered and unfiltered pages preserve ordering, bounds and no omissions/duplicates; missing task is 404, empty existing task is an empty page; cross-filter cursor reuse is rejected |
| 3 | Totals | Overnight, boundary, zero-duration, archived and running worklogs retain current aggregate semantics, including completed ends later than `now` |
| 4 | Revision capture | Data and revision describe the same locked observation; a partial read cannot authorize a command based on stale cached resources |
| 5 | Domain and conflict rules | One active worklog, atomic switch, archived protection, overlap rejection, exact source checks, preview expiry and stale-write rejection remain |
| 6 | Stop then archive | A concurrent restart between stop and archive cannot silently authorize archiving the newly active task |
| 7 | Receipts and retries | Replay after intervening writes retains original result/revision; late receipts cannot replace newer reads; identical retries retain original ID and timestamps |
| 8 | Failure and restart recovery | Possibly committed writes trigger explicit rereads; restart changes the epoch; eviction/restart limitations remain documented; no local fallback |
| 9 | Client composition | Names resolve independently; mismatched report/tracking revisions stop total extrapolation; coherent pairs resume it without counting time twice |
| 10 | Coordinated compatibility | Server and every supported client use `/v1` protocol 3; incompatible clients/servers are rejected through health checks before resource adoption; no legacy routes or dual-version support remain |
| 11 | Current workflows | CLI, TUI and macOS use cases above work through the migrated remote adapter; local mode retains its behavior |
| 12 | Application E2E | Existing TUI and native local bulk archive suites pass; real-server native scenarios verify changed `/v1` workflows; logs and result bundles are available in CI |
| 13 | Configurable archive | Default and chosen periods, complete candidate labels, invalid input, refresh/cancel, stale preview rejection, actual count, active timer and relaunch persistence remain correct |

Use behavior and concurrency tests rather than tests that merely restate DTO construction. Portable Swift state tests and server integration tests complement application E2E coverage; they do not replace native UI-to-server scenarios. Keep existing E2E tests unchanged for this documentation branch, and obtain explicit consent before implementation adds or modifies E2E tests.

Run the repository's required checks for each implementation handoff:

```sh
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
cargo test
cargo llvm-cov --lcov --output-path lcov.info
cargo crap --workspace --lcov lcov.info
```

For native application E2E checks, run on macOS with Xcode and an active desktop session:

```sh
apps/swiftui/check-native-ui.sh
```

Retain the current native macOS CI job and its `.build/native-ui.log` and `.build/native-ui.xcresult` artifacts. Extend the same automation to the planned real-server scenarios during implementation; a Linux-only test pass cannot verify native application E2E behavior.

Use pinned tool versions and mutation scopes from [AGENTS.md](../AGENTS.md). Mutate all changed Rust production files and affected portable Swift files, including production code exercised by changed tests. Expand the scope for shared dependencies/build changes. Missed or timed-out mutants and CRAP scores above the configured threshold are failures. Documentation-only changes skip mutation checks.

## Response examples

The examples describe one task at 11:00 UTC. UUIDs are illustrative. All three reads observed the same server revision. Task and tracking reads could also observe different revisions; the cache coordination steps above define how clients handle that.

`GET /v1/tasks`

```json
{
  "tasks": [
    {
      "id": "019a6650-0000-7000-8000-000000000101",
      "name": "Investigate communication",
      "archived": false,
      "created_at": "2026-10-07T09:00:00Z",
      "updated_at": "2026-10-07T09:00:00Z",
      "latest_work_start": "2026-10-08T10:30:00Z"
    }
  ],
  "revision": "019a6650-0000-7000-8000-000000000106:7"
}
```

`GET /v1/tracking`

```json
{
  "active_worklog": {
    "id": "019a6650-0000-7000-8000-000000000105",
    "task_id": "019a6650-0000-7000-8000-000000000101",
    "start": "2026-10-08T10:30:00Z",
    "end": null
  },
  "revision": "019a6650-0000-7000-8000-000000000106:7"
}
```

`GET /v1/reports/task-totals?start=2026-10-08T00:00:00Z&end=2026-10-09T00:00:00Z&now=2026-10-08T11:00:00Z`

```json
{
  "start": "2026-10-08T00:00:00Z",
  "end": "2026-10-09T00:00:00Z",
  "now": "2026-10-08T11:00:00Z",
  "rows": [
    {
      "task_id": "019a6650-0000-7000-8000-000000000101",
      "duration_us": 7200000000
    }
  ],
  "total_us": 7200000000,
  "revision": "019a6650-0000-7000-8000-000000000106:7"
}
```

The 2-hour total comes from 30 minutes after midnight, a completed 1-hour worklog and 30 minutes of the running worklog. The [historical protocol-2 response comparison](https://github.com/remshams/time-tracker/blob/2ee9ce88ef7fe431044cb77f24b9df9301dffab2/docs/client-server-communication.html#read-examples) shows the individual intervals and the larger protocol-2 response shapes.

### Stop command receipt

The following receipt acknowledges stopping the worklog from the read examples. It contains no task catalog or separate active-tracking field.

```json
{
  "request_id": "019a6650-0000-7000-8000-000000000107",
  "applied_revision": "019a6650-0000-7000-8000-000000000106:8",
  "replayed": false,
  "result": {
    "kind": "worklog",
    "value": {
      "id": "019a6650-0000-7000-8000-000000000105",
      "task_id": "019a6650-0000-7000-8000-000000000101",
      "start": "2026-10-08T10:30:00Z",
      "end": "2026-10-08T11:00:00Z"
    }
  }
}
```

The receipt describes the original command outcome. A replay keeps `applied_revision` and `result`, changes `replayed` to true, and requires explicit recovery before claiming current resource state.
