# Incremental import review and repair

Historical canonical R2 and CI42823400965 passed at 0856401, but independent
whole-range R1 found two HIGH regressions. Cleanup could delete a directory
substituted between its pathname check and open; ordinary export/daemon startup
could bypass a failed post-exchange parent fsync. Source attribution, exact CLI
interleavings, original failures and the complete reconstructed FAIL are retained.
The original full report was overwritten by a wrapper using the report path for
its final-message output. Its bytes are lost; the reconstruction uses only the
retained test log, probe sources and frozen source. It is published at
https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/61#issuecomment-6769172067.

The repair binds deletion to the opened old inode, validates pending markers on
ordinary opens, requires parent synchronization before admission, uses retained
directories for open/restore, and keeps every native handle's directory alive.
A real native checkpoint first failed after the Storage owner dropped; the same
regression passes with shared tree/directory custody.

The final focused R3 run passed 68 Rust tests, with five intentional supervisor
child ignores, plus formatting, Clippy and diff checks. Public R3 uses the actual
test-built CLI: cleanup substitution is refused with foreign data retained;
post-exchange EIO rejects export and daemon startup; recovery permits local
commits and exact full-tail replay; ordinary-root substitution leaves foreign
contents untouched. Probe sources/hashes and owned-child/scratch cleanup are in
the compressed logs. Repeated public probes follow the final custody change.

Preserved failed attempts include an immutable-Evidence fixture setup error,
an unsafe fixture-parent mode, old corruption fixtures that now correctly fail
at ordinary open, and a synthetic manifest field-order comparison. Their
corrected classifications are explicit; none is relabelled a product RED.
The first RED collector incorrectly claimed three causal failures; its separate
correction records two causal failures, one setup error and a passing control.

Earlier archive source pins describe their historical candidate, not this repair:
the incremental archive matches 6e692d6dbb0ac329372166a65d3029c913621049;
the incident fixture archive matches 085640133221975943ec4ebc7d28cc86d5df879b;
the mempal-final archive matches b98eec93cd2a2d6ca525eaa56d681864280bed05.
The manifest here binds the repaired source bytes. Successor exact-HEAD canonical,
whole-range review and CI must pass before merge. Issue #25 remains open for
changed mempal snapshot identity/update/deletion and complete acceptance.
