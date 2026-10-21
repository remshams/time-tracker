# 0025: fit native CI within the macOS runner limit

## Status

Accepted. Partially supersedes ADR 0015's macOS Rust test runner, native time limits, and rule against changed-file filters. All other ADR 0015 decisions remain accepted.

## Date

2026-10-17

## Context

The 100 successful CI runs from 7 to 16 October 2026 took a median of 31.8 minutes, with a 90th percentile of 61.8 minutes and a maximum of 98.8 minutes. The six native UI groups took 14 to 19 minutes each. Every other job took about two minutes.

GitHub-hosted macOS runners for this personal account run at most five jobs at once. Across all 200 CI runs in the same period, including cancelled and failed runs, no more than five macOS jobs ran at the same time. Each CI run requested seven macOS jobs: six UI groups on `xcode-27` and the Rust workspace tests on `macos-26`. Even a run with no competition queued two jobs. The last UI group in each run waited a median of 14 minutes for a runner, with a 90th percentile of 44 minutes. Pushes to `main` are never cancelled, so they compete with pull request runs for the same five runners.

Documentation-only pull requests, such as architecture revisions and agent guidance, also ran every native job.

## Decision

- Run the native UI tests in four groups: `core`, `editing`, `server`, and `mac`. Measured suite durations from 15 and 16 October 2026 balance each group at about 15 to 17 minutes, including the `mac` group's additional checks.
- Run the full Rust workspace tests on macOS in the `mac` native group on `xcode-27`, with its own Rust build cache. This replaces ADR 0015's "Run Rust macOS tests on `macos-26`". A CI run then needs at most four macOS runners.
- Bound each native job to 40 minutes and its UI step to 32 minutes. Group runs stop after 30 minutes overall. Pagination runs keep their 24-minute limit, and the per-test allowance and three-minute silence limit are unchanged. This replaces ADR 0015's 30-minute job, 25-minute step, and 24-minute runner limits for regular native groups.
- Skip the native jobs when a pull request or push to `main` changes only Markdown files, `docs/`, `.githooks/`, `scripts/tests/`, or the Linux-only tooling files listed in `scripts/ci-native-scope.py`. Run them for manual runs, pushes without a previous commit, failed comparisons, empty diffs, and any other path. Pull requests compare from the merge base; pushes compare with the previous commit. Moves list both their old and new paths. `CI passed` accepts skipped native jobs only when that scope found no affecting change. This replaces ADR 0015's "Do not filter checks by changed files" for the native jobs only. Every other regular check still runs for every change.

## Consequences

A CI run with no other runs in progress should take about 20 minutes instead of about 30. Two overlapping runs still request eight macOS jobs, so one of them queues. Documentation-only changes finish without macOS runners.

Each group runs more tests, so a failure or rerun repeats more work. The Rust workspace tests run on the macOS 27 preview image instead of the stable macOS 26 image, so CI no longer tests Rust on macOS 26.

The skip list in `scripts/ci-native-scope.py` needs review when a native build or test starts reading a listed path. Unlisted paths always run the native jobs.

These are projections from earlier measurements. Compare the queue and execution times of runs after this change with the figures above before accepting this record.
