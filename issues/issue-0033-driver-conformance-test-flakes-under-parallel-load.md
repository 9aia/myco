+++
id = "I-033"
title = "driver conformance test flakes under parallel load"
status = "done"
created = "2026-11-20"
priority = "med"
done = "2026-11-20"
+++

myco-driver::conformance::driver_passes_pavo_conformance intermittently fails in full-suite runs: first 'view' op times out after 10s, cascading into view_shape/query/malformed/session_alive violations. Passes in isolation (~3s) and in a --no-fail-fast full run. Seen twice on 2026-11-20 across unrelated changes — likely spawn/timing sensitivity of the wire driver under CPU contention. Consider raising the per-op timeout in the harness or serializing the conformance test.

## Resolution

Fixed on main (cde02274): pavo-testkit's hardcoded 10s per-request timeout starved the debug driver under full-suite parallel load. Switched to check_driver_with(CheckOptions{timeout:30s}) — assertions unchanged. Verified: full nextest run 788/788 green.
