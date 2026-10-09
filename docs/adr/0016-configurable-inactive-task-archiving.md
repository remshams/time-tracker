# Configurable inactive task archiving

## Status

Partially superseded by [ADR 0017](0017-separate-http-resources-from-client-refreshes.md); all other decisions remain accepted. ADR 0017 replaces the separate fixed/configurable HTTP routes, embedded candidate task metadata and protocol-2 compatibility requirement. The configurable-period domain and native-dialog decisions remain accepted.

## Date

2026-10-07.

## Context

The macOS application needs the bulk archive workflow already available in the TUI, with an editable inactivity period defaulting to 14 days and a complete candidate list. The existing Rust storage query fixes the period at 14 days. The TUI must retain its current behavior and contracts.

## Decision

Represent a positive inactivity period in the domain model. Add a public application capability and repository port for configurable preview and archive operations. SQLite applies one shared eligibility query for both configurable operations and the existing 14-day methods. The new application capability composes coherent preview reads and atomic archive writes without moving persistence rules into views or transport adapters.

Add HTTP endpoints and DTOs for the configurable operation. Keep the existing protocol version, endpoints, and fixed-period DTOs intact. Preview returns all candidate task metadata, the captured time, the chosen period, and the server revision. Confirmation reuses that time and period, checks the revision, and recomputes current eligibility. Keep request deduplication and the existing local candidate-ID check. Do not introduce candidate fingerprints or preview IDs.

The Rust bridge retains the current preview within its connection handle. The native client owns the editable dialog state and serializes its reads and writes through the existing session. Changing the period or cancelling invalidates the displayed preview. Late responses cannot restore a cancelled or replaced preview. Submission uses the reviewed context, reports the actual count, and requires a fresh preview after failure. An uncertain result refreshes authoritative state without automatically submitting another archive request.

## Consequences

The native client can show every eligible task and choose a positive inactivity period. Running tasks remain protected, and archiving preserves their worklog history. The TUI, its 14-day behavior, and its E2E tests remain unchanged. Existing clients continue to use the old endpoints; the configurable operation requires an updated server. Storage arithmetic remains checked for periods that cannot produce a representable cutoff.
