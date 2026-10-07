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
- Run every regular check for every pull request, push to `main`, and manual CI run. Do not filter checks by changed files.
- Keep the full Rust and portable Swift mutation suites manual only, through `workflow_dispatch`. Do not schedule them or include them in pull-request and push runs.
- Run dependency audits weekly and on manual request.

Retain the existing checks, pinned tool and action versions, caches, artifacts, and the required `CI passed` aggregate check.

This replaces ADR 0014's plan to introduce routine scheduled full mutation runs when CI is set up. Its local mutation scope and failure rules remain accepted.

## Consequences

CI requires no Blacksmith installation or organization membership. [GitHub's standard runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) are free for this public repository. Private repositories use their account's included minutes and billing limits.

Runner hardware and cache performance differ from Blacksmith. The native `xcode-27` image is in public preview, so its first CI run must confirm the builds and desktop layout checks work. Existing caches and report uploads use GitHub's services.

Full mutation coverage depends on someone starting the manual workflow. Regular CI still checks tests, coverage, and complexity on every run.
