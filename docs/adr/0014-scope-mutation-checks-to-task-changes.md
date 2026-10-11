# 0014: scope mutation checks to task changes

## Status

Partially superseded by ADR 0015; all other decisions remain accepted.

## Date

2026-10-07

## Context

Full mutation checks after every task repeat builds and tests for code unrelated to the change. This delays local feedback as the Rust workspace and portable Swift client grow. CI now runs a focused portable Swift mutation check for each pull request; full Rust and Swift mutation suites remain manual.

## Decision

Use focused mutation checks for task handoffs. Select their scope from the entire task's committed and uncommitted changes.

- For Rust production changes, mutate the changed source files. Use affected crates when file selection is unclear. Keep unit and integration tests enabled, and include workspace tests when selected mutants rely on tests in other crates.
- For portable Swift production changes, select affected production files with the existing mutation runner's `--files` option.
- Include production code exercised by changed tests, including test-only changes. Expand the scope for shared code, dependencies, features, and build configuration. Run the full Rust workspace or Swift package when the impact cannot be bounded.
- Skip mutation checks for documentation-only changes and other changes that cannot affect production behavior, tests, or build inputs. Report the selected scope or reason for skipping at handoff.
- Keep changed-location filtering and reuse of previously caught results as diagnostic shortcuts. They do not replace the required file or crate checks because they can miss reduced test coverage in unchanged code.
- Run the focused Swift mutation check on every pull request, selecting scope from the full merge-base-to-head diff. Always run the Muter regression checks. An explicitly empty production scope applies to documentation, native UI, Rust, and CI-tooling-only changes.
- Return `review-required` when broad dependencies, shared test support, deleted production files, or changed tests without a curated production mapping make scope uncertain. Keep the gate red until the selector or mapping defines a bounded scope. Do not fall back to a full Swift package run in pull request CI.
- Keep full Rust and Swift mutation suites manual through `workflow_dispatch`; do not schedule them or add a repository metadata file tracking the last run.

Retain the existing failure rules, pinned tool versions, named exclusions, and other validation checks. The commands and scope rules live in [AGENTS.md](../../AGENTS.md).

## Consequences

Local mutation checks spend less time on unrelated code. Pull requests also check the full contents of selected Swift production files with the complete package test suite. The scope report records changed files, mapped tests, and an explicit empty production scope where appropriate.

Scope selection still requires judgment when tests or shared code change. The curated mapping and fail-for-review cases keep uncertain changes from receiving an incomplete green result. Focused checks can miss regressions outside selected files and crates, so full mutation coverage remains available through the manual workflow. A focused result must state its scope and must not be reported as a full run.
