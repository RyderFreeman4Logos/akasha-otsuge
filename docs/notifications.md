# Notifications and stdio plugins

The daemon projects review and incident revisions from the durable storage outbox
into a local inbox. Source revisions and their outbox entries commit atomically;
projection can resume after a crash without creating a second notification.
The inbox remains available when channels are disabled. A configured local
channel records delivery in that inbox:

```toml
[notifiers.inbox]
kind = "local"
enabled = true
```

Reload with `akasha-otsuge config reload`. No local channel is enabled implicitly.
`deliveries.inbox` records the delivered notification generation, selected config
generation and stable local message ID. An absent entry means this channel has
not delivered the notification; a smaller delivery generation means an update
is waiting. Disabling a channel prevents subsequent deliveries and updates,
while retaining the inbox and earlier delivery records. Re-enabling or reloading
an already successful generation does not resend it.

## Read and acknowledge

Use the same JSON scope file as `review list`:

```sh
akasha-otsuge notify list --scope scope.json
akasha-otsuge notify list --scope scope.json --after NOTIFICATION_ID
akasha-otsuge notify show NOTIFICATION_ID
akasha-otsuge notify ack NOTIFICATION_ID --generation 1 --idempotency-key operator-ack-1
```

Responses use the normal CLI JSON envelope. List results contain `items` and
`next`; continue while `next` is non-null, including after an empty filtered page.
Pages examine at most 32 records and return at most 1 MiB of serialized items.
Read, acknowledgement and delivery all check
the current local read grant and scope. Revoking access also stops local delivery
of affected records; restoring access makes them eligible again.

Each item includes `notification_id`, `review_id`, `source_revision`, `priority`,
`summary`, `evidence_locator`, `occurrences`, `generation`,
`acknowledged_generation` and per-channel `deliveries`. The locator identifies
whether `review show` or `incident show` owns the source ID. Summaries contain only
the source type, state and count; they omit chat, code, claims and free-form
report text. Read the evidence through the normal permission-checked interface.

Ack applies to exactly the generation supplied by the operator. Replaying its
idempotency key returns the same durable receipt; using that key for another ID
or generation fails. Ack changes neither the review state nor memory adoption.
A source state or priority change advances the generation, so an earlier ack
does not acknowledge the new information. Repeated same-group incident reports
update the count without producing a generation for every report. Source
grouping and priority rules remain those documented in [incidents](incidents.md).

## Bounds and recovery

The worker processes bounded outbox and inbox pages. It advances past its own
records without repeatedly committing idle cursor updates. Startup replays only
uncheckpointed outbox entries, using each item's recorded source revision to
avoid duplicate projection. Local delivery and its success record are one storage
commit, so the local channel has no external-send/receipt ambiguity.

Channel names contain 1–128 bytes without control characters. Each notification
retains at most 64 distinct channel histories, including removed channels.
Requesting another channel sets visible `delivery_error: CHANNEL_HISTORY_LIMIT`
for that notification. Existing histories stay intact and new notifications can
still use the new channel. Reuse a retained channel name for the same logical
destination; changing configuration alone does not resend a completed generation.
Each item also retains at most 64 displaced stdio destination histories. A
new destination beyond that bound sets `DESTINATION_HISTORY_LIMIT`; returning to
a retained destination swaps its history with the current one without eviction.
Delivery claims reserve space below the storage record's 128 KiB limit. A
candidate over 120 KiB sets `DELIVERY_RECORD_LIMIT` and leaves its existing
deliveries intact. This reserves room for results, errors and source updates;
other notifications continue normally.

The [stdio protocol](notifier-protocol.md) supports external send/update, live
cancellation and persistent retry/crash recovery.

## File plugin example

The executable [file notifier](../examples/notifier-file.py) uses Python's standard
library and writes a private JSON inbox. Configure absolute paths on the daemon
host, then reload:

```toml
[notifiers.file]
kind = "stdio"
command = ["/absolute/path/notifier-file.py", "--state", "/absolute/private-dir/inbox.json", "--fail-first"]
timeout = "30s"
```

The state directory must be owned by the daemon UID with no group/other access;
the plugin can create it with mode 0700. It creates mode-0600 state and lock files.
`--fail-first` persists one demonstration failure before the first message. The
daemon retries automatically. Remove the option for ordinary use; its marker
survives subsequent invocations either way.

Inspect `inbox.json`: `messages` contains the latest minimal envelope per provider
ID, while `receipts` binds each accepted idempotency key to its exact input hash
and message ID. Reusing a key with different input fails. A process lock plus
fsynced atomic replacement keeps the effect and receipt together. Concurrent
replays return the same ID. Updates replace the existing message and retain
earlier receipts. Loading validates every message and receipt, their references
and the current message's input hash. Malformed state or a receipt without its
message fails without changing the state file; an old valid receipt can still
replay after that message was updated. This is a bounded example inbox: it refuses writes exceeding
2 MiB and preserves the previous state; it does not expire receipts automatically.
Plugin acknowledgement remains separate from `notify ack` and review state.

## Live channel policy

Each channel accepts these optional settings:

```toml
min_priority = "normal"  # normal, high or urgent
quiet_hours_utc = [22, 7] # inclusive start hour, exclusive end; UTC, no DST
digest_seconds = 60      # 0..86400; default 0
timeout = "30s"          # positive ms, s, m or h; stdio attempt budget
```

Quiet hours apply to all priorities; `[0, 24]` pauses delivery for the whole day.
The digest delay coalesces normal-priority changes per notification, measured
from the start of its current generation. Repeated reports do not restart it.
High/urgent escalation bypasses the digest delay, while still obeying quiet hours
and the priority threshold. The inbox always retains the underlying items.

These policies apply at each new claim or retry. Policy-only changes preserve
destination identity and do not cancel an active send. Command or credential-ref
changes cancel its process, then subsequent claims use the current destination.
Changing the value of an inherited environment variable requires a daemon
restart; changing the configured `env:NAME` reference takes effect on reload.

## Verification

`cargo test --locked --test daemon notifier_tests::` exercises real daemon/CLI
delivery, ack replay and independence, SIGKILL before delivery, restart, live
disable/re-enable, scope revocation, bounded pagination, channel-history limits,
idle storage stability, and 100 grouped reports with priority escalation. The
incident fixture uses a controlled registered Caller; it starts no model Runner.
It also drives the independent file plugin through one persisted failure,
100 grouped reports, important upgrades and stable external IDs.
`python3 scripts/test_notifier_file.py` checks standalone capability/delivery,
deduplication, update, concurrent replay and invalid-input/state preservation.
The [focused evidence](benchmarks/results/mvp023-stdio/README.md) preserves passing
checks and earlier failures; canonical gate, independent review and CI are
separate publication requirements.
The [R1 repair evidence](benchmarks/results/mvp023-stdio-r1-fixes/README.md)
retains the rejected candidate's gate/review and both causal regressions.
