# Stdio notifier protocol v1

A configured `stdio` notifier is an independent executable. The daemon launches
the configured command without a shell, sends one JSON request plus a newline to
stdin, closes stdin, and reads one JSON response until process exit. Successful
responses require exit status zero. Requests are limited to 8 KiB; stdout and
stderr each have a 4 KiB limit. Stderr and unrecognized response text are not
copied into notification records.

## Capabilities

The daemon first starts the command with this stdin request:

```json
{"schema_version":1,"op":"capabilities"}
```

The response declares `send` (required true), and optional booleans `update`,
`idempotency`, `ack_callback` and `rich_text` (default false):

```json
{"schema_version":1,"send":true,"update":true,"idempotency":true,"ack_callback":false,"rich_text":false}
```

Unknown fields or another schema version are rejected. A capability is a plugin
declaration; it is not independent proof of the external provider's guarantees.
The daemon currently emits plain summaries and accepts operator acknowledgements
through `notify ack`; declaring callbacks or rich text does not activate a server.

## Delivery

A second invocation receives:

```json
{
  "schema_version": 1,
  "op": "deliver",
  "notification": {
    "notification_id": "stable-notification-id",
    "idempotency_key": "review-id:generation:destination-hash",
    "review_id": "source-review-or-incident-id",
    "priority": "normal",
    "summary": "review is open (1 reports)",
    "evidence_locator": "akasha-otsuge review show source-review-id",
    "action": "create",
    "external_message_id": null
  }
}
```

Return `{"schema_version":1,"external_message_id":"provider-message-id"}`.
Message IDs must be nonblank, contain no control characters and fit 256 bytes.
An update-capable plugin receives `action: update` and the previous external ID
when the notification advances. Otherwise it receives a new create request.
The generated summary excludes source reason, claims, chat and code.

The daemon persists the selected config generation, route fingerprint and exact
envelope before execution. It records `preparing` before the capability probe,
`sending` before delivery, and the returned provider ID on success. Plugin I/O
runs outside the daemon admission lock. One dedicated process slot is retained
until its owned guardian and descendants have been reaped.

Disable, route change, access revocation and daemon shutdown cancel the owned
plugin process. The existing native guardian also observes daemon death and
cleans up descendants and its private scratch directory. A configured
`credential_ref = "env:NAME"` supplies that value to the child as
`AKASHA_NOTIFIER_CREDENTIAL`; records retain neither the value nor command text.

## Retries and recovery

Transient process, I/O and timeout failures persist a retry deadline with 1, 2,
4 and 8 second delays, stopping after five attempts. Restart preserves the
original start, retry schedule, accepted envelope and idempotency key. An
interrupted `preparing` or `sending` attempt enters the same bounded recovery
path. Protocol errors are terminal; disable, revocation and shutdown block the
attempt until current policy permits another claim. Each attempt shares one
capability/send deadline, configured by `timeout` (default `30s`). The active
attempt uses one monotonic deadline across both processes. Reloading the timeout
affects later claims without extending an active attempt.

The inbox exposes the attempt number, original start, next retry time, last
error and `duplicate_risk`. A send interrupted before its response may already
have reached the provider. Recovery replays the same key; without declared
provider idempotency it explicitly flags possible duplicates. A changed route
uses a new destination key and does not reuse the previous provider's message
ID. No mechanism can retract a send completed before cancellation.

## Current acceptance boundary

Basic send/update, minimal envelopes, nonblocking admission and live cancellation
are covered by controlled executable-plugin tests. Persistent finite retry,
restart during backoff, and real SIGKILL before/after the plugin's durable effect
also pass, including both idempotent replay and exposed duplicate delivery.
Live priority, quiet-hours, digest and timeout controls also have config and
daemon tests. The distributable reference plugin and final whole-candidate
acceptance remain unfinished on MVP-023. Do not interpret a stored
idempotency key or capability declaration as universal exactly-once delivery.
