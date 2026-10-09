# Remove TrackerSnapshot and compose resources in client workflows

## Status and prerequisite

Proposed on 9 October 2026. This branch contains the plan only. Implementation is a separate refactoring and must wait for [issue #8, Refactor HTTP routes and migrate clients to independent resources](https://github.com/remshams/time-tracker/issues/8), to be completed and validated on `main`.

The route refactoring is currently tracked by [PR #17](https://github.com/remshams/time-tracker/pull/17). Its merge and the required passing checks are part of the prerequisite. Publishing this plan does not authorize starting implementation before that prerequisite is satisfied.

Planning inspected the protocol-3 implementation at source revision `cce0bb13fe6b8be8841de5e11fad86dc0fb8b0fd`. The documentation branch starts from `main` revision `59c17bb66b90459ce63ef2cca5076e1a561bfe41`, which predates that implementation. Rebase the plan branch onto the completed route refactoring before implementation and confirm the affected contracts against the resulting source revision.

The existing API decision is [ADR 0017 at the inspected revision](https://github.com/remshams/time-tracker/blob/cce0bb13fe6b8be8841de5e11fad86dc0fb8b0fd/docs/adr/0017-separate-http-resources-from-client-refreshes.md). It separates HTTP resources while retaining internal transaction and consistency guarantees. This plan changes internal ownership and contracts after that route migration.

## Problem and goal

`TrackerSnapshot` combines the task catalog with active tracking. Removing `/v1/snapshot` did not remove the aggregate from the repository, remote adapter, native bridge or Swift client. Reports, history and command completion paths still carry broad state to refresh unrelated client views. The TUI has a similar universal `ApplicationSnapshot`.

Remove the Rust and Swift `TrackerSnapshot` types completely. Replace broad snapshot contracts with resource-specific reads, command outcomes and feature-specific presentation state. Each client decides which resources a workflow needs. Keep fetching, revision consistency and command recovery shared in Rust.

Do not replace the type with an equally broad `TrackerState`, `ClientState` or renamed snapshot. A generic wrapper around one resource and its revision is acceptable; a universal bundle of unrelated resources is not.

## Requirements

- Preserve local SQLite and remote mode behavior, tracking rules, worklog validation, archive protection, search, ordering, history pagination and daily totals.
- Preserve the protocol-3 HTTP routes, response shapes and compatibility number. Introduce no convenience endpoint or database schema change.
- Give each resource its own data, revision and loaded state. An unloaded cache must remain distinguishable from a confirmed empty task list or idle tracking state.
- Let SwiftUI and TUI application/controller code choose resources and compose the state required by a feature. Views and widgets receive prepared presentation state.
- Keep revision comparison, bounded reconciliation, command guards, identical retries and uncertain-write recovery shared in Rust.
- Preserve database transaction boundaries and publish consistent groups of refreshed client resources together.
- Return focused command outcomes without automatically attaching tasks, tracking, reports or history.
- Keep expected worklog values, reviewed archive previews and user timestamps intact across refresh and recovery. Fetching a newer revision must not silently authorize a changed intent.
- Replace snapshot types in production code and test fixtures. Keep existing E2E behavior assertions unchanged unless a requirement changes and the user explicitly consents to an E2E update.

## Ownership and interfaces

| # | Layer | Responsibility | Contract |
|---|---|---|---|
| 1 | `tracker-protocol` | HTTP resource representations and committed command receipts | Task, tracking, history and report DTOs; no aggregate snapshot DTO |
| 2 | `tracker-remote` resource access | Decode responses and retain independent resource observations | Typed task, tracking, history and report reads with their own revisions |
| 3 | Coordinator inside `tracker-remote` | Refresh selected resources, check consistency, guard commands and recover affected resources | Explicit resource selection and consistency requirements; no combined tracker model |
| 4 | Rust native bridge | Translate resource reads and command outcomes for Swift | Separate resource payloads and coordinator operations; no snapshot attachment |
| 5 | SwiftUI and TUI application/controllers | Select resource dependencies and compose feature state | Models specific to the task list, daily totals, history or another workflow |
| 6 | Views and widgets | Render state and emit user actions | Presentation values, without HTTP or revision retry logic |
| 7 | `tracker-application` and `tracker-storage` | Domain operations, transactional reads and adoption of committed changes | Explicit initialization reads and operation-specific results |

Keep the coordinator in the existing Rust client crate initially. A new crate is not required to establish this boundary. Resource reads return typed data; resource selection does not require string paths or an untyped JSON map.

### Resource observations

Separate the remote adapter's task catalog and active tracking fields. Remove `snapshot()` and replace the broadly named `refresh()` with explicit resource refresh operations. Individual task, worklog, report and history caches retain their existing scope and revision rules.

Expose data and revision together for each resource so callers cannot accidentally pair a value with another cache's token. Keep revision tokens opaque. The local source uses transaction guarantees rather than fabricated server revisions.

Audit and remove `SnapshotDto`, `SnapshotJson` and their conversion helpers once their production and fixture consumers migrate. Resource-specific DTOs can remain shared where they describe the actual wire contract.

### Shared consistency and command coordination

A caller identifies the resources it needs and whether they must describe the same server revision. The coordinator fetches those resources into temporary observations, validates them and compares the relevant revisions.

If revisions differ, retry reconciliation once, preserving the current bounded policy. Adopt the selected observations together only after the required checks pass. On failure, keep the previous confirmed observations and return a conflict or availability error. Report the refresh result without returning a universal aggregate.

Keep relation checks where a workflow requires them, such as a running worklog referring to an available, unarchived task. Do not require a timer-only tracking read to fetch the task catalog.

The coordinator also retains command preflights, revision guards, request-ID handling and resource recovery after uncertain outcomes or replayed receipts. A receipt's applied revision must not become the revision of a newer resource observation. Late receipts must not replace newer reads.

Controllers declare display dependencies. Command code owns the dependencies required for a safe write, so a controller cannot bypass a preflight by omitting a resource from its display refresh.

### Swift bridge and SwiftUI

Replace bridge snapshot exports with separate task, tracking, report and history reads. Commands return their domain outcomes and receipt information needed by the client. Remove aggregate state from creation, rename, archive, tracking, worklog and report results.

Expose shared coordinator operations through the bridge. Within the existing serialized Swift worker, perform a selected refresh and capture its required resource values without another bridge operation interleaving. Swift receives typed resource values and does not implement HTTP retries or reconciliation itself.

Move resource selection and presentation composition into the client application layer. Keep feature-specific state with the corresponding feature. `TrackerSession` coordinates publication where several features must update together. Avoid introducing another universal session response model.

Examples of resource dependencies:

| # | Workflow | Resources and consistency |
|---|---|---|
| 1 | Timer display | Tracking only |
| 2 | Task list with active-task indication | Tasks and tracking; require matching revisions when publishing a coherent view |
| 3 | Daily totals | Report and tracking; project running totals only when revisions and the active worklog anchor agree |
| 4 | History display | Requested history page; resolve task metadata separately when needed |
| 5 | Worklog correction or move | Exact original worklog values and relevant task reads; preserve the existing guarded intent |
| 6 | Bulk archive | Complete preview and task metadata; retain the original preview time, period and revision for confirmation |

Valid report totals must survive missing task metadata. Resolve labels separately and retain confirmed totals when running projection is unsafe.

### TUI and CLI

Replace the TUI's universal `ApplicationSnapshot` and snapshot attachment on every completed request with request-specific resource updates and feature state. Continue using the shared Rust coordinator for required consistency and recovery. Publish related task and tracking changes together when the workflow needs them.

Check both local and remote request dispatch, periodic refresh, task commands, reports and history consumers. Audit the CLI so removing shared types does not break its resource output or existing command behavior.

### Local application and storage

Replace `tracker_snapshot()` with an explicitly named initialization/recovery read that returns task items and active tracking as separate values from one SQLite transaction. A bounded pair at this repository boundary is acceptable; it must not become a reusable all-purpose client response type.

Remove embedded `TrackerSnapshot` from report, global history, inactive preview and bulk archive results. Carry only values required by each operation's domain guarantees and state adoption. Move catalog-wide display refreshes to explicit client workflows instead of attaching the catalog to every result.

Preserve reads that must occur under the same database transaction. Adopt committed command values directly where required; do not replace transaction results with post-commit reads that could observe another writer or misreport a successful command as a failure.

Trace existing local cache refresh behavior before narrowing each result. Where a workflow currently depends on refreshed related data, make the dependency explicit and preserve its consistency through an operation-specific transactional read.

## Migration sequence

1. Complete and validate route refactoring issue #8. Rebase this branch onto that source revision and confirm the inventory of snapshot consumers.
2. Split remote resource observations and extract explicit coordinator operations. Migrate Rust callers while retaining the existing guard and recovery policies.
3. Add resource-specific native bridge reads and focused command outcomes. Migrate the Swift worker, contracts and feature composition together, then remove `SnapshotJson` and Swift `TrackerSnapshot`.
4. Replace universal TUI completion state with request-specific updates. Migrate periodic refresh, reports and history. Confirm CLI compatibility.
5. Replace repository initialization/recovery reads and narrow operation-specific results while preserving transaction guarantees. Remove Rust `TrackerSnapshot`, protocol `SnapshotDto` and all obsolete helpers and fixtures.
6. Update affected architecture diagrams and method tables to show resource ownership, shared coordination and client composition. Record the implemented source snapshot and the scope and revision of retained quality measurements.
7. Run the required validation and confirm that no universal aggregate was introduced under another name.

Temporary migration adapters may exist between implementation commits, but remove them before handoff. Keep commits scoped to individual components or modules and use PascalCase commit scopes. Combine cross-module validation at the end.

## Validation and acceptance

- [ ] Issue #8 is complete, its implementation is on `main`, and its required checks pass before production changes begin.
- [ ] Rust and Swift production code and test fixtures contain no `TrackerSnapshot`; broad protocol, bridge and TUI snapshot contracts are removed too.
- [ ] No renamed universal tracker aggregate replaces the removed types.
- [ ] Independent reads do not fetch unrelated resources. Cover timer-only tracking and report reads without automatic catalog attachments.
- [ ] Resource-specific data and revision remain paired, including unloaded, empty and idle states.
- [ ] Selected coherent reads publish together. Cover a writer changing state between reads, successful reconciliation, exhausted reconciliation and failed refresh retaining confirmed state.
- [ ] Safe command dependencies remain shared in Rust. Cover stale intent, stop-then-archive races, exact worklog expectations and reviewed archive previews.
- [ ] Original command receipts, replay behavior, uncertain-write recovery and late-response protection remain correct.
- [ ] SwiftUI and TUI compose feature state without duplicating transport, revision retries or command recovery.
- [ ] Local SQLite reads retain transaction consistency and committed outcomes cannot be overwritten by a later writer's state.
- [ ] Existing CLI, TUI and macOS behavior passes in local and remote modes, including daily totals, history, reconnect and relaunch persistence.
- [ ] Architecture diagrams and method tables describe the implemented ownership and communication paths.

Prefer existing repository integration tests, remote-adapter tests, native bridge tests, portable Swift session tests and application E2E suites. Test visible outcomes and concurrency guarantees rather than the private structure of caches. Keep existing E2E tests unchanged for this behavior-preserving refactoring; obtain explicit consent if an actual behavior requirement requires edits.

At implementation handoff, run the required Python and Rust suites, coverage and CRAP checks, Swift style checks for Swift production or portable test changes, and scoped Rust and Swift mutation checks. Run native macOS acceptance on the existing supported runner. Apply the repository's pinned tool versions and treat missed or timed-out mutants and excessive CRAP scores as failures.

This plan branch changes documentation only. Run the repository's required Python, Rust, coverage and CRAP checks for its handoff; skip mutation checks because documentation cannot change production behavior, tests or build inputs.

## Advantages and drawbacks

| # | Advantage | Cost or risk |
|---|---|---|
| 1 | Resource contracts reflect the independent API and their own revisions | More typed contracts and bridge methods |
| 2 | Each feature declares its data needs and can avoid unrelated reads | Both clients compose presentation data, with some expected duplication |
| 3 | Revision checks and command recovery remain shared | The coordinator needs explicit resource and consistency requirements |
| 4 | The snapshot name no longer hides mixed cache revisions or unrelated attachments | Partial refreshes need careful publication and stale-state handling |
| 5 | Reports and commands stop driving implicit catalog refreshes | Existing consumers must declare refresh dependencies explicitly |
| 6 | Local domain guarantees remain independent of UI models | Narrowing repository outcomes requires careful concurrency regression checks |

## Out of scope

This work adds no routes, protocol version, database migration, server push, durable command deduplication, offline writes or new user-visible feature. It does not remove useful operation-specific domain results or weaken transaction guarantees. The current architecture diagrams continue describing implemented code until this separate refactoring is implemented.
