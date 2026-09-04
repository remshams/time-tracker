# 0001: record architecture decisions

## Status

Accepted

## Date

2026-09-04

## Context

The project makes small architecture decisions continuously: naming, ordering rules, storage behavior, protocol choices. So far these have lived in `plan.md` as a settled-decisions table, which mixes direction with roadmap and grows without a history. A heavier formal ADR process would slow that kind of decision down without adding anything the project needs.

## Decision

Time Tracker records architecture decisions as lightweight ADRs in `docs/adr/`.

- Every record has five sections: Status, Date, Context, Decision, Consequences. No front matter, no template boilerplate beyond that.
- `docs/adr/README.md` is the index and links every record.
- Accepted records are immutable. A changed decision is a new record; the old record gets a Superseded note in its Status section and keeps its original text.
- Small decisions that do not change architecture (for example, a test name or a dependency bump) stay in commit messages and ordinary code review. Not everything deserves a file.

## Consequences

Decisions have a stable home and a reviewable history. When a later milestone contradicts an earlier decision, the contradiction shows up as a superseding record instead of a silent edit. `plan.md` keeps the roadmap and can link to records rather than restating them, so a decision has one authoritative text.

The cost is one more file per decision. That is acceptable at this project's decision rate.
