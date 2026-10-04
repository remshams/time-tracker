# 0012: share client query and failure policies

## Status

Accepted

## Date

2026-10-04

## Context

The CLI exposes the same task, timer, worklog, and report capabilities as the TUI. Its first implementation copied calendar and search helpers from the TUI, inspected repository errors to classify failures, and constructed a temporary worklog with a fabricated task ID to validate corrections. Local bulk archive confirmation also used a different operation time from the TUI.

Both clients need the same policies without depending on each other's presentation code. New databases also no longer need example tasks.

## Decision

- Keep correction rules in the domain. Expose timestamp validation on `WorklogTimes` so clients can validate proposed corrections without inventing entity identities. `Worklog::corrected` uses the same rules. The application still checks the actual stored worklog and optimistic concurrency guards.
- Put calendar report selection and task search policies in application capability modules. CLI arguments and TUI navigation state translate to neutral application types. Report aggregation remains an application use case, and storage remains behind repository ports.
- Expose failure origin and recovery status through `ApplicationFailure`. Preserve the original semantic category when authoritative-state recovery fails. Remote adapters attach transport origin to each error, so clients do not infer it from mutable connection state. Presentation adapters choose exit codes and text without inspecting repository variants.
- Confirm bulk archives at the preview's original timestamp. Storage rechecks candidates under one write transaction and rejects recorded positive-duration work starting at or after that timestamp. Running worklogs, changed metadata, and changed candidate sets remain protective guards. Remote confirmation also retains its server revision and preview expiry.
- Start new local and server databases empty. Remove automatic example insertion. Preserve every task and worklog already stored. Tests create their own data explicitly.

## Consequences

The domain remains independent of transport and presentation. The application adds timezone support for calendar queries, while clients keep their input parsing, formatting, and interactive state. Shared tests cover search ordering and calendar boundaries, including daylight-saving transitions and missing local dates.

Invalid CLI corrections still fail before opening storage or connecting to a server. UTC microsecond precision, task and worklog identity, active/completed state, and stale-selection protection remain unchanged. Backend diagnostic text stays out of client-visible failures and HTTP responses.

Waiting to confirm an archive cannot add tasks that aged into eligibility after the preview. Intervening work can reject the entire archive, requiring a fresh preview. Local tokens have no expiry; remote tokens keep the existing server-clock limit. This decision does not add durable request deduplication.
