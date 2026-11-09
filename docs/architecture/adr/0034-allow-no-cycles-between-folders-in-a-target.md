# 0034: allow no cycles between folders in a target

## Status

Accepted. It partially supersedes the clauses of ADR 0026, ADR 0030, ADR 0032 and ADR 0033 listed under [Superseded clauses](#superseded-clauses). All other decisions in those records remain accepted.

## Date

2026-11-09

## Context

[ADR 0030](0030-replace-the-session-with-feature-stores.md) lets features depend on each other without a cycle and says only `App/` composes features at runtime. It doesn't say whether `App/` counts in that check, or whether the app and the package share one dependency graph. [ADR 0032](0032-move-the-dialogs-into-feature-stores.md) reads it as "Features don't depend on `App/`".

The macOS app doesn't follow that. `App/` depends on feature folders, and `Features/Connection`, `Features/MenuBar`, `Features/Settings`, `Features/TaskCatalog`, `Features/Tracking`, `Features/WorklogHistory` and `Infrastructure/MacLifecycle` form one cycle with it. The dependencies back into `App/` go through three types: `TrackerStore`, `TrackerTaskPresentationCoordinator` and `TrackerLaunchConfiguration`. Most of them are `TrackerStore`, which owns the menu settings and forwards calls to `TrackerSync`, `TrackingActions` and `MenuBarStore` in the package. Without those dependencies the app target has no cycle. The `TrackerClient` package has no folder cycle.

## Decision

### Rule

- Folders in a target must not form a cycle.
- `App/` counts as an ordinary folder. A feature may use something in `App/` when that creates no cycle.
- Each of `App/`, `Shared/` and `Contracts/` is one node in the check, and so is each folder under `Features/` or `Infrastructure/`.
- Review checks each target separately. The package can't see the app, so a dependency from the app into the package can't complete a cycle. Same-named feature folders in the two targets are separate nodes. The app's `Features/Tracking` may use the package's `App/TrackingActions`, even though the package's `App/` depends on the package's `Features/Tracking`.
- The rule covers Swift only. The Rust crates keep the rule in `AGENTS.md`.

### Checking the rule

Review checks the rule, because Swift compiles a target as one module and doesn't reject cycles between its folders. Add a script if cycles creep back.

### Removing the existing cycle

The app target breaks the rule until these steps of [issue #358](https://github.com/remshams/time-tracker/issues/358) merge.

- [#360](https://github.com/remshams/time-tracker/issues/360) moves the menu settings out of `TrackerStore` into a store in `Features/MenuBar`.
- [#361](https://github.com/remshams/time-tracker/issues/361) removes the feature views' reads of `TrackerTaskPresentationCoordinator` and moves `TrackerLaunchConfiguration` to `Infrastructure/System`.
- [#362](https://github.com/remshams/time-tracker/issues/362) lets views and the status item use `TrackerSync` and `TrackingActions` directly.
- [#363](https://github.com/remshams/time-tracker/issues/363) deletes `TrackerStore`.

### Rejected alternative

- **Banning feature-to-feature dependencies.** It would push most code into `Shared/` and leave thin features, as ADR 0030 says.

### Superseded clauses

This record replaces these clauses:

- ADR 0030, "Feature dependencies": "Only `App/` composes features at runtime." `App/` holds the composition code today, but the rule above lets other folders use it.
- ADR 0030, "Feature dependencies": "Features may depend on other features as long as the dependencies form no cycle." For Swift, the rule above replaces it.
- ADR 0030, "Feature dependencies": "It is not an architecture primitive, so `docs/architecture/architecture.md` doesn't list it." The architecture document states the rule.
- ADR 0032: "Features don't depend on `App/`, so a dialog store can't call `TrackerSync`." Dialog stores keep their closures because of the cycle described under Consequences.
- ADR 0032, rejected alternatives: "The first breaks the rule that only `App/` composes features". The first alternative still fails, because it would form that same cycle.
- ADR 0032, "Reloads and selection": "Views call the last two through `TrackerStore`." After #362, views call `changeTab(_:)` and `selectTask(_:)` on `TrackerSync`.
- ADR 0032, "Views": "The coordinator and the app menus read them through `TrackerStore` properties that point at `features`. `TrackerStore` otherwise keeps its Mac settings and forwarders." After #363, they read the feature objects directly. The menu settings move to `Features/MenuBar` in #360, and #363 deletes `TrackerStore` with its forwarders.
- ADR 0026: "Make `TrackerStore`, `ConnectionDraftStore`, and `TrackerTaskPresentationCoordinator` observable." This no longer applies to `TrackerStore` after #363. The other two stay observable.
- ADR 0033, Context: "Features can't call `TrackerSync`". A package feature can't call `TrackerSync` when that forms a cycle. `TrackerFeatures` in `App/` creates every package store, so this holds for all of them, including the dialog stores.

## Consequences

Dialog stores keep their reload closures. A dialog store that called `TrackerSync` would create a cycle in the package, because `TrackerFeatures` in the package's `App/` creates the dialog stores.

A feature in the app target can read `TrackerSync` and `TrackingActions` from the package without creating a cycle, so the forwarders in `TrackerStore` have no reason to stay.

`App/` depends on most features in its target, so few of them can use `App/` without forming a cycle.

A reviewer has to follow a new dependency through the folders of its target, and can't only look for imports of `App/`. A reviewer can still miss a cycle.
