# ADR-0001: Use rendered scroll cycles as a display completion condition

- **Status:** Accepted
- **Date:** 2026-09-10

## Context

Clients need to request that a horizontally scrolling image remains visible for
a minimum number of complete passes and a minimum amount of time. A duration
estimated from image width and an assumed scroll speed is unreliable because
the server alone knows the prepared image width, configured interval, actual
render timing, and remaining request deadline.

The API and existing clients already use `duration_seconds`. Compatibility with
servers that predate cycle-based playback must be preserved during upgrades and
rollbacks. The worker also has a dequeue-based deadline that covers eye-catch
loading and playback, decoding, preparation, and main display work.

## Decision

Add the backward-compatible `scroll_cycles` and `min_display_seconds` fields to
`SendImageRequest` without renumbering existing fields or introducing a `oneof`.
A positive `scroll_cycles` takes precedence over `duration_seconds` on supporting
servers. Clients may keep a positive duration in the request so older servers
can fall back to duration-based playback.

The server counts cycles from actual horizontal offsets of the image after it is
scaled to the panel height. A cycle completes after every offset from zero through
the final prepared-image offset has been rendered successfully and held for the
configured scroll interval. Repeated refreshes at one offset do not advance the
count, and delayed rendering never skips offsets to catch up.

Cycle-based playback completes only when both the requested cycle count and the
minimum main display time are satisfied. If the cycle count is already complete,
playback stops when the minimum time is reached, including partway through an
additional cycle. It does not wait for another cycle boundary or hold a static
frame. Eye-catch playback and preparation are excluded from the minimum main
display time and from the cycle count.

The worker deadline and shutdown remain authoritative and may interrupt playback
before either requested minimum is achieved. The worker timeout is not estimated
or extended from image width or requested cycles. `SendImage` continues to
acknowledge queue admission rather than display completion; no progress or
completion RPC is added.

## Alternatives considered

- **Estimate duration from image width.** Rejected because client-side estimates
  duplicate server configuration and cannot account for render delays or the
  prepared width.
- **Hold the image statically after completing the requested cycles.** Rejected
  because the request explicitly selects scrolling and short images should keep
  moving until the minimum time is reached.
- **Wait for a cycle boundary after reaching the minimum time.** Rejected because
  the minimum is a lower bound, not a request for additional complete cycles.
- **Extend the worker timeout automatically.** Rejected because the timeout is a
  system-wide bound covering all request work and shutdown responsiveness.
- **Add a display-completion RPC.** Rejected because admission acknowledgement and
  completion reporting are separate concerns, and clients do not require progress
  reporting for this feature.

## Consequences

- Wide images or requests with large cycle counts may be interrupted by the worker
  deadline before completion.
- Older servers ignore the new fields and use `duration_seconds`, so they cannot
  guarantee the requested cycles or minimum display time.
- Clients do not need the panel dimensions or server scroll interval.
- Completion and interruption reasons are available in structured server logs,
  while the RPC response retains its queue-admission meaning.
- Cycle counting and display-limit handling remain shared display logic; hardware
  backends continue to implement only the `LedDisplay` interface.
