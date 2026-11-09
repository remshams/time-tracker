# 0033: report bulk archive preview failures through a closure

## Status

Accepted. It partially supersedes the clause of ADR 0032 listed under [Superseded clause](#superseded-clause). All other decisions in that record remain accepted.

Partially superseded by [ADR 0034](0034-allow-no-cycles-between-folders-in-a-target.md) for the context statement that features can't call `TrackerSync`; all other decisions remain accepted.

## Date

2026-11-09

## Context

`BulkTaskArchivingStore` loads a preview of the tasks it would archive. A failed preview can mark the connection stale, and that check lives in `TrackerSync`. Features can't call `TrackerSync` ([ADR 0032](0032-move-the-dialogs-into-feature-stores.md)), so dialog stores receive a reload closure instead. That closure doesn't fit here: a preview is a read, so no reload follows it.

## Decision

`BulkTaskArchivingStore` takes a second closure, `recordPreviewFailure: @MainActor (Error) -> Void`. The code that creates the store passes `TrackerSync.recordReadFailure(_:)`, which recovers the connection when the failure calls for it. The store runs the closure only for its newest preview request, so a late failure from an older request changes nothing.

### Superseded clause

This record replaces this clause:

- ADR 0032: "`TaskCreationStore` is the one store with a second closure". `BulkTaskArchivingStore` has one too. The rest of that paragraph, including `showTask`, stays.

## Consequences

The feature still doesn't depend on `App/`. Two dialog stores now take a second closure, and each one names what it reports or does.
