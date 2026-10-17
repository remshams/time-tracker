# ADR 0020: Observe session-owned dialog presentations

- Status: Accepted
- Date: 2026-10-11
- Issue: #31

## Context

Six native dialog stores forwarded session commands and read presentation values from TrackerSession. TrackerPresentationObserver compared each dialog's values and availability, then called a store callback to publish changes. Adding a dialog required edits to both layers.

The portable package supports Swift 5.9 and macOS 13. Native Observation starts at macOS 14, so adopting it would raise the package's deployment requirement. Linux portable tests cannot import Combine.

## Decision

TrackerSession owns stable TrackerDialogPresentation instances for creation, rename, individual and bulk archiving, correction, and movement. Each instance contains its current dialog state, availability and retained sheet content. It publishes only when the state or availability changes. On Apple platforms, conditional Combine conformance lets SwiftUI observe the instance directly. On Linux, the same Foundation-backed model and session behavior remain testable without Combine. Supported deployment and language versions stay unchanged.

Views send commands to TrackerSession. Dialog models cannot execute commands or update themselves outside the portable module. TrackerStore exposes the shared session and its models to the native views. It still owns native preferences, menu integration and lifecycle adapters.

The generic model retains the last presented content when the session dismisses or resets a dialog. The next presentation replaces that content. Creation and rename use this rule too, so draft text does not disappear while a sheet animates closed.

TrackerPresentationObserver retains its list, activity, elapsed-text and daily-total comparisons. Those adapters have separate publication needs. Clock ticks do not change dialog models or invalidate dialog subscribers. The menu presentation observer remains responsible for native menu snapshots and status-item updates. TrackerTaskPresentationCoordinator still chooses one window to present the shared dialogs. Its publisher subscriptions read authoritative session state because Combine emits before the model snapshot changes.

Command serialization, connection generations, stale-response rejection, uncertain-write intent and sleep/lock handling stay in TrackerSession and its feature state.

## Validation

Portable tests cover availability changes, stable model identity, drafts, errors, pending intent and sheet content during cancellation and successful completion. Apple-platform package tests also check Combine notification counts for timer ticks, equal drafts, validation errors and editor availability changes. Existing native UI and layout suites run unchanged because this change preserves user-visible behavior.
