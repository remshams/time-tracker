# 0011: use one TUI event loop with a fixed redraw tick

## Status

Accepted

## Date

2026-09-28

## Context

Remote mode currently has a terminal input loop and a worker loop. They exchange commands and rendered buffers through channels. The worker uses `TestBackend` to lay out a frame before sending it to the terminal loop. Local mode has its own loop.

The interface also changes without input or an HTTP response. A running worklog displays elapsed seconds, clipboard confirmation expires after three seconds, reports can change while time passes, and remote mode checks the server periodically. Waiting only for keys and outstanding HTTP responses would leave these updates idle.

## Decision

- Use one TUI event loop for local and remote modes. It owns terminal input, presentation state, and drawing to the real terminal. It waits for terminal events, pending application outcomes, and timers. Remote HTTP runs as asynchronous futures on the same Tokio runtime.
- Keep a fixed 250 ms redraw tick. It updates time-dependent presentation without maintaining separate next-change deadlines. Input and completed requests wake the loop and draw immediately; they do not wait for the tick.
- Keep the one-second remote refresh schedule and the delayed busy indicator as timer events in that loop. Coalesce refreshes while another remote operation is pending.
- Remove the remote worker, its input and output channels, transferred frame buffers, and production use of `TestBackend` when the shared loop is in place. Preserve terminal restoration and focus handling.

## Consequences

The TUI redraws up to four times per second while idle. This is a deliberate simplicity tradeoff, not a requirement of Ratatui or the HTTP protocol. A later change to deadline-based redraws would require a new decision record. The server process continues to run its own HTTP event loop; this record concerns the TUI process.
