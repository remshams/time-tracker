# 0013: remove remote archive candidate fingerprints

## Status

Accepted

## Date

2026-10-07

## Context

Remote bulk archives compare a candidate fingerprint as well as the server revision. The fingerprint adds transport fields and validation for an operation that already recomputes eligible tasks before writing.

## Decision

Remove the candidate fingerprint from remote previews and archive requests. Retain server revision checks and request deduplication. Recompute eligible tasks at the preview's original time and archive them atomically. Return the actual archived count, even when it differs from the preview.

Local confirmation retains its expected candidate IDs and exact-set check. Running-task protection, eligibility rules, and remote preview expiry remain unchanged.

Use protocol version 2 because strict JSON decoding makes the removed fields incompatible with older clients and servers.

This replaces ADR 0012's remote exact-candidate-set guarantee. Its local archive policy and other decisions remain accepted.

## Consequences

A server revision change still rejects an archive. Storage changes outside the server can change the eligible set without changing that revision; confirmation uses the recomputed set. Clients report the committed count. Update remote clients and servers together and regenerate older remote CLI preview tokens.
