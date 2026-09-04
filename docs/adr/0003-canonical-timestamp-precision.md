# 0003: canonicalize timestamps to microseconds

## Status

Accepted

## Date

2026-09-04

## Context

`chrono::DateTime<Utc>` can represent nanoseconds, while the SQLite schema stores task and worklog timestamps as integer microseconds. Keeping a nanosecond value in an application snapshot after persistence would make the returned value differ from the value loaded later. Two values inside one microsecond could also appear distinct before a restart and equal afterward.

## Decision

The application boundary canonicalizes every client-provided task and tracking timestamp to UTC microsecond precision before applying domain rules or persistence operations.

SQLite continues to store integer microseconds. Repository results and in-memory application snapshots use the same canonical value. Values that differ only below one microsecond intentionally represent the same instant for Time Tracker.

## Consequences

Task ordering, worklog intervals, returned outcomes, and values recovered after restart agree exactly. A client cannot rely on sub-microsecond distinctions, which the current storage model could not preserve anyway.

Future storage and HTTP adapters must use the same precision. Changing precision requires a superseding ADR and a data migration.
