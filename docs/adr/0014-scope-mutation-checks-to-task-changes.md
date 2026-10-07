# 0014: scope mutation checks to task changes

## Status

Partially superseded by ADR 0015; all other decisions remain accepted.

## Date

2026-10-07

## Context

Full mutation checks after every task repeat builds and tests for code unrelated to the change. This delays local feedback as the Rust workspace and portable Swift client grow. CI is not set up yet, so scheduled full runs cannot replace this work today.

## Decision

Use focused mutation checks for task handoffs. Select their scope from the entire task's committed and uncommitted changes.

- For Rust production changes, mutate the changed source files. Use affected crates when file selection is unclear. Keep unit and integration tests enabled, and include workspace tests when selected mutants rely on tests in other crates.
- For portable Swift production changes, select affected production files with the existing mutation runner's `--files` option.
- Include production code exercised by changed tests, including test-only changes. Expand the scope for shared code, dependencies, features, and build configuration. Run the full Rust workspace or Swift package when the impact cannot be bounded.
- Skip mutation checks for documentation-only changes and other changes that cannot affect production behavior, tests, or build inputs. Report the selected scope or reason for skipping at handoff.
- Keep changed-location filtering and reuse of previously caught results as diagnostic shortcuts. They do not replace the required file or crate checks because they can miss reduced test coverage in unchanged code.
- Defer routine scheduled full runs until CI is set up. Do not add a repository metadata file tracking the last run or a local full-run scheduler for this change.

Retain the existing failure rules, pinned tool versions, named exclusions, and other validation checks. The commands and scope rules live in [AGENTS.md](../../AGENTS.md).

## Consequences

Local mutation checks spend less time on unrelated code. Existing tools support file and crate selection, so this policy needs no runner changes.

Scope selection requires judgment, especially when tests or shared code change. Focused checks can miss regressions outside the selected files and crates. Until CI adds routine full runs, those regressions may remain undetected unless an explicitly requested or broadly scoped check finds them. A focused result must therefore state its scope and must not be reported as a full run.
