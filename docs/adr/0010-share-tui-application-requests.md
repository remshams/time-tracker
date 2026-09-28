# 0010: share TUI application requests across local and remote modes

## Status

Accepted

## Date

2026-09-28

## Context

The TUI's screen actions call a synchronous application service and handle each result in the same function. Local mode runs those actions in the terminal loop. Remote mode runs the same `App` on a worker thread because its HTTP adapter uses blocking requests. This keeps network waits away from terminal input, but the worker must also own presentation state and render frames for the terminal loop to display.

Changing the HTTP calls to `async` without changing screen actions would still leave input waiting whenever an action awaits a response. The TUI needs one way to start an application operation and handle its result after it completes.

## Decision

- Keep screen `Command` values for keyboard intent. Introduce typed application requests and outcomes for calls to application use cases. `App` creates a request from a command and applies its outcome when it completes. An outcome may lead to another request, such as reloading a worklog page after a write.
- Use this request and outcome path in local and remote TUI modes. The local executor calls the existing application service and can complete when first polled. The remote executor awaits HTTP. Local requests incur no intentional redraw-tick or channel wait.
- Keep backend execution and transport details out of screen state and rendering. Keep Tokio and reqwest out of the domain and synchronous application-service contracts.
- Capture identifiers, timestamps, and expected values when an action creates a request. Apply confirmed backend state after a result. Associate screen-specific reads with their originating view so a late result cannot replace the data for another task or report period.
- Serialize remote writes. Do not start a duplicate write while its result is pending. Retain the existing revision checks, stable retry data, and remote-only storage behavior.

## Consequences

Screen actions need separate request and result handling, but local and remote modes no longer need different presentation flows. The remote adapter remains responsible for protocol validation, retries, and its confirmed snapshot. Local SQLite calls still execute synchronously when polled; this decision adds no worker thread for them.
