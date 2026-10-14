# 0015: use GitHub-hosted runners for CI

## Status

Accepted

## Date

2026-10-07

## Context

The project needs GitHub Actions for Rust, portable Swift, and native macOS checks. We initially chose Blacksmith, with Linux runners for checks that do not require macOS.

The GitHub repository belongs to the personal account `remshams`. [Blacksmith's quickstart](https://docs.blacksmith.sh/introduction/quickstart) states that its runners support GitHub organizations only, so they cannot run jobs for this repository. Keep the personal repository and use GitHub-hosted runners instead of moving it to an organization.

## Decision

Use standard GitHub-hosted runners for all workflows.

- Run portable checks, dependency audits, and mutation tests on `ubuntu-24.04`.
- Run Rust macOS tests on `macos-26`.
- Run native builds and layout checks on `xcode-27`. [GitHub's image manifest](https://github.com/actions/runner-images/blob/main/images/macos/xcode-27-arm64-Readme.md) lists macOS 27, Xcode 27.0, and the required macOS 27 SDK.
- Bound each native job to 30 minutes and its UI step to 25 minutes. The UI runner allows 24 minutes overall, stops after three minutes without output, and enables a six-minute XCTest execution allowance per test. Timeout remains a failure. Retain stack samples, process names, and test timings with the native diagnostics. Reuse the Debug build directory for UI tests.
- Run every regular check for every pull request, push to `main`, and manual CI run. Do not filter checks by changed files. The focused Swift mutation job is pull-request-only and selects production files from the PR diff.
- Run the focused portable Swift mutation check as a required pull-request-only job. It always runs the Muter regression checks and mutates the selected complete production files with the full package test suite. Documentation, native UI, Rust, and CI-tooling-only changes use an explicit empty production scope. Broad dependency or shared test-support changes, deleted production files, and changed tests without a curated mapping return `review-required`; the gate stays red until the selector or mapping defines a bounded scope instead of triggering a full run.
- Keep full Rust and portable Swift mutation suites manual only, through `workflow_dispatch`. Do not schedule them or include full-suite runs in pull-request and push workflows.
- Run dependency audits weekly and on manual request.

Retain the existing checks, pinned tool and action versions, caches, artifacts, and the required `CI passed` aggregate check.

This replaces ADR 0014's plan to introduce routine scheduled full mutation runs when CI is set up. ADR 0014's local mutation scope and failure rules remain accepted, with focused Swift mutation checks added to pull request CI.

## Consequences

CI requires no Blacksmith installation or organization membership. [GitHub's standard runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) are free for this public repository. Private repositories use their account's included minutes and billing limits.

Runner hardware and cache performance differ from Blacksmith. The native `xcode-27` image is in public preview, so its first CI run must confirm the builds and desktop layout checks work. Existing caches and report uploads use GitHub's services.

Full mutation coverage depends on someone starting the manual workflow. Pull request CI runs the selected Swift mutations and their regression checks; regular CI also checks tests, coverage, and complexity on every run.

The 14 October 2026 run at `c7fd82a` showed why native execution needs limits.
[The menu job](https://github.com/remshams/time-tracker/actions/runs/38527835921/job/115732307693)
exceeded one hour after the app's main thread stopped answering accessibility
requests. A 15-second element wait did not bound the underlying XCTest calls.
Successful native UI steps took 10 to 21 minutes; two pagination cases took about
four minutes each. The six-minute case budget retains room for those existing
tests while bounding stalled cases. The runner records future timings so changes
can be compared against the actual test workload.
