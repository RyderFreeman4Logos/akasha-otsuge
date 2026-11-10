# R2 recovery class matrix

Frozen failed candidate: 03acdccb102daf3d6556ed21d19c125b3648b8bb. R2 FAIL published unchanged on PR55.

| Boundary | Required behavior | Evidence to run |
| --- | --- | --- |
| resolve/spawn/guardian ownership unavailable | notifier transient unavailable; preserve accepted input and five-failure budget | actual missing executable, restart/restore, permanent absence |
| native pipe read / child wait / nonzero exit | notifier transient failure, no raw stderr | typed error matrix + existing failure/timeout real processes |
| output limit / protocol / unsupported capability | terminal, no retry | existing malformed/oversize/unsupported real processes + mapping matrix |
| disable / route change / shutdown / ReadGrant denial | blocked, no failure-budget reset | existing fifth-claim and live-control cases |
| same destination after five failures | no automatic renewed budget, including restart | missing-executable and existing failure cases |
| failed A to B | new destination key and no A provider ID | live failed-A to held-B case |
| A to B to A | retain A accepted payload, failure budget and provider identity | return to exhausted A; return to pending A after source generation change |
| any successful current generation, destination reload | no resend | failed-A/B-success/A-return case + existing live policy |
| newer source generation | new key/budget after old accepted pending request settles | returned-A pending generation then update |
| bounded history / full record | refuse new destination, keep existing entries; no eviction-based retry | small state capacity unit + existing real byte-bound IPC/record tests |

Changes stay within notifier scheduler/stdio adaptation, its persisted record, focused tests and user docs. Shared Runner error semantics and storage record limits stay authoritative. Historical destination records are bounded and never silently evicted. Missing credentials/configuration and invalid protocol remain operator/terminal conditions.
