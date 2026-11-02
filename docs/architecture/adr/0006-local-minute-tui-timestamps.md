# 0006: local-minute TUI timestamps

## Status

Accepted

## Date

2026-09-09

## Context

The TUI showed correction fields and worklog history with RFC 3339 timestamps, offsets, seconds, and fractional seconds. That exposed storage details that users did not need for ordinary corrections and made the interface harder to read.

## Decision

The TUI resolves one IANA timezone-rules snapshot when it starts and uses it for history, correction opening, parsing, ambiguity resolution, and nudges. A valid `TZ` IANA override takes precedence, including a leading colon or a common zoneinfo path. Otherwise Linux and macOS use the OS IANA timezone. If neither source yields a recognized zone, the TUI uses UTC and reports that fallback in the status line.

The TUI displays worklog timestamps in that timezone as `YYYY-MM-DD HH:MM`, with no seconds, fractions, or visible offset. Correction input uses the same local format. Chrono-valid years outside `0000` through `9999` use chrono's signed expanded form, such as `-0001` and `+10000`.

A changed field is interpreted in local time and aligned to local wall-clock second 00 before conversion to UTC. A historical sub-minute offset can therefore produce a UTC instant with nonzero seconds. An unchanged field preserves its stored UTC seconds and microseconds.

A local time skipped by an offset change is rejected. When a local time occurs twice, the editor chooses the occurrence whose offset matches the field's original timestamp. If neither occurrence matches, the editor reports the ambiguity and keeps the draft open. A nudge first resolves the displayed local minute, intentionally discarding hidden precision for that changed field. `j` and `k` then adjust that instant by five minutes; `J` and `K` adjust it by one hour. These are absolute adjustments across offset changes, including repeated local times. The TUI rejects an adjustment that cannot round-trip exactly through its displayed local minute in the same timezone and chosen offset.

The timezone snapshot is immutable for the app session, so correction drafts do not sample offsets or compare the current process timezone. If a UTC timestamp's offset would put its local `NaiveDateTime` outside chrono's range, history uses a safe range marker and correction stays closed with an "outside editable range" error.

Durations and live timers remain `HH:MM:SS`.

This record supersedes only the timestamp-presentation clauses in ADR 0004 and ADR 0005. Every other decision in both records remains accepted.

## Consequences

History rows and correction fields use fewer columns. Tests can use UTC for deterministic local display. Corrections still store UTC values, but direct edits intentionally discard hidden local seconds and fractions for the edited field. Leaving a field untouched does not discard that precision. Absolute nudges remain exact or fail without changing the draft.
