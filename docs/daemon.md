# Local daemon and management protocol

The MVP-008 daemon is merged and validated; exact gate, independent review and
installed acceptance are tracked in [the TODO](plan/todo.md). Provider jobs and
[retrieval jobs](retrieval.md) reuse this management protocol. Host adapters,
research workflows and full-scale product acceptance remain separately tracked.

The Linux daemon owns the single Storage writer and immutable configuration
manager. It listens only on a filesystem Unix socket. The runtime and data
roots must belong to the effective UID and have mode 0700; the socket and lock
have mode 0600. Existing unsafe paths are refused. A locked runtime file prevents
two daemons from replacing each other's socket. Stale socket cleanup occurs only
after obtaining that lock; normal shutdown removes only its own socket inode.
Every accepted peer must have the daemon's effective kernel UID. These controls
do not isolate mutually untrusted processes sharing that UID or protect files
that the UID can read directly.

## Lifecycle and paths

```
akasha-otsuge --config /absolute/config.toml daemon start
akasha-otsuge --socket /absolute/run/service.sock daemon status
akasha-otsuge --socket /absolute/run/service.sock doctor --json
akasha-otsuge --socket /absolute/run/service.sock daemon stop
```

`daemon start` launches the same executable through `/usr/bin/nohup`, with null
stdin, a separate process group and private `daemon.log`. `daemon run` is the
foreground form for a service supervisor. Stop/SIGTERM/SIGINT stop intake, finish
the current bounded capture batch, retain unfinished accepted jobs for restart,
and close Storage. Listener/scheduler errors take the same drain path and preserve
the first error. A request timeout does not discard its mutation owner: accepted
reload, submit, cancel, permission and delivery writes remain owned until collected.
Storage closes before the socket/lock owner is released. A stopped daemon yields `DAEMON_UNAVAILABLE` promptly; no
second database writer or silent daemonless mutation is created.

Defaults use `$XDG_CONFIG_HOME/akasha-otsuge/config.toml`,
`$XDG_DATA_HOME/akasha-otsuge`, and
`$XDG_RUNTIME_DIR/akasha-otsuge/service.sock`. The HOME fallbacks are `.config`,
`.local/share`, and `.local/run`. Relative environment path values are ignored.
`--socket` locates the running service without parsing a damaged candidate config;
use it to inspect/reload the retained generation after a rejected file update.
`--profile NAME` selects the startup profile; the daemon retains that selection
for validation, explicit reload and watching. With an explicit `--socket`, remote
management uses the running daemon's profile rather than a client-side override.
Startup requires a valid configuration. Changing storage/socket paths requires
explicit drain and restart; the daemon never migrates the old database implicitly.

## Frames and bounds

Each frame is a four-byte unsigned big-endian byte length followed by UTF-8 JSON.
The server first sends `protocol_version`, `capabilities`, and
`max_message_bytes` (2 MiB). A full connection pool sends `RESOURCE_BUSY` in that
initial greeting, before accepting a request. There are 16 connection slots and
a ten-second management exchange timeout. At most 16 mutation operations can
remain owned, including requests whose clients disconnected; excess work returns
`RESOURCE_BUSY`. Long jobs return durable handles;
this transport timeout is not a job/provider/runner deadline.

A request contains `protocol_version: 1`, `request_id`, optional absolute
`deadline_ms`, and `command`, tagged by `op`. Unknown fields, unsupported versions,
invalid/expired requests and oversized frames are refused. Replies contain
`schema_version: 1`, the request ID, `sequence: 1`, and either `data` or a fixed
`error.code`/`error.message`. Raw argument values are not error messages. Regular
CLI JSON has a trailing newline. Exit codes are 0 success, 2 invalid input,
3 daemon unavailable, 4 busy, and 1 other failure.

History/state/diff pages examine at most 32 memberships/revisions and one lookahead
key, and return at most 1 MiB of entries. They expose fixed read points and bound
cursors; callers must follow `next`, not treat an empty page as exhaustion.
Socket framing enforces its separate reply ceiling as well.

## Configuration

`config validate`, `config reload`, `config status --json`, and
`config show --effective` use the running manager. A 500 ms watcher requires two
matching content observations; it handles atomic rename and routes changes through
the same reload method. Complete candidates are parsed, validated and prepared,
then committed durably before publishing active or pending state. Rejection keeps
the prior active generation. Repeating an unchanged candidate preserves its
previous applied/rejected/restart-required outcome.

Durable snapshots contain typed configuration and provenance, not raw file comments
or resolved credential values. Existing credential-reference and diagnostic
redaction rules remain in force. Snapshots are split into bounded immutable
Storage chunks and a checked pointer; the pointer and first publication of each
generation commit together. Restart restores the generation and accepted jobs'
original configuration identity. Starting with a changed valid configuration
publishes the complete new generation before serving requests, including a pending
socket change in the same data directory. Old job generations remain archived.
Starting with the unchanged active configuration preserves any pending candidate.
Pending restart settings are never half-applied.

## Capture jobs

```
akasha-otsuge --socket /absolute/run/service.sock capture reconcile \
  /absolute/binding.json /absolute/capture.jsonl --idempotency-key import-1
akasha-otsuge --socket /absolute/run/service.sock job show import-1
akasha-otsuge --socket /absolute/run/service.sock job list
akasha-otsuge --socket /absolute/run/service.sock job cancel import-1
```

The explicit binding JSON contains `source`, `scope`, `stream`, and `coverage`
from [the capture contract](capture.md). This is an administrator-declared
interchange source, not authenticated host identity. Captured content remains
unverified ingress. Canonical paths must remain under current `access.read_roots`,
and the current read grant must cover the declared scope. Both are checked again
at execution. The default owner principal and scope user are `uid:<kernel UID>`.

Acceptance atomically persists the job and bounded active index before returning.
The idempotency key binds the source/binding and canonical path; reuse with another
plan is a conflict. A job pins configuration generation/digest, acceptance/queue
clock and execution budget. Reconciliation uses the configured `timeouts.tools`;
future provider and runner clocks are separate. The hard envelope is 256 active
jobs and 16 executing jobs, additionally restricted by current configuration.
Capture batches share the admission boundary so cancellation, permissions and
configuration cannot be changed halfway through a batch.
The current concurrency cap is checked while claiming a queued job under that
same boundary; lowering it allows existing jobs to drain without admitting extra
work from a stale scheduling decision.

Running jobs persist lease owner, expiry, attempt and original execution deadline.
Restart holds the sole daemon lock before reclaiming old leases. Only this
idempotent capture operation is recoverable here; ambiguous external model or
process effects are not automatically replayed. Capture source-change/revision
conflicts have at most three total attempts with bounded backoff; restart does
not reset the original deadline. After a physical batch finishes, the original
deadline is checked again before success; a late batch retains its committed
checkpoint/result but the job records `JOB_TIMEOUT`. A completed reconcile can still report a partial
tail, unavailable source, rotation/truncation loss or invalid frame. Inspect the
checkpoint coverage and pending/loss fields; job completion is not proof of full
source coverage. Accepted archive events/checkpoints are reused without duplicate
effects.

## History, receipts and live access

`memory show ID [--at COMMIT]`,
`memory history ID [--from COMMIT] [--to COMMIT] [--cursor JSON_FILE]`,
`state show --scope SCOPE_JSON --at COMMIT [--cursor JSON_FILE]`,
`state diff FROM TO --scope SCOPE_JSON [--cursor JSON_FILE]`, and `receipt show ID` call the real Domain
APIs. A state diff compares native adoption revisions at both points with bounded
scope-index traversal. It retains before/after bodies, including retirement,
without replaying an unbounded event log.
Save the returned `data.data.next` object as the cursor JSON file and keep the
same read point/range/scope on subsequent pages; changed queries reject it.
When history omits `--to`, the response's `data.read_seq` is the fixed upper point
to use for subsequent pages. Unknown or repeated pagination flags are rejected.

`access grant SCOPE_JSON` narrows/restores the authenticated local owner's grant;
`access revoke` revokes it. Restart does not recreate a revoked grant. Historic
reads and receipt replay still check the current grant. These management commands
cannot assert another UID or a verified human/host origin.

`receipt replay ID` explicitly replays archived output bytes, not a current
recommendation. The CLI checks the body hash, writes and flushes every byte to
stdout, and sends an acknowledgement bound to that receipt and hash. Only this
acknowledgement moves Prepared to Delivered; failed stdout writes leave it
Prepared. Lost acknowledgements remain uncertain, and a repeated successful replay
is idempotent. Receipt viewing itself does not assert delivery.
