# Source-backed candidate extraction

Extraction runs asynchronously after raw sources are durably captured. Select an
explicit evidence unit containing the question, answer and relevant tool evidence;
submit only newly captured units or a deliberate re-extraction. This version has
an explicit submission API, not an automatic scanner of every captured message.
It never modifies raw sources or treats a model response as human approval.

Configure one text profile with `retrieval.extraction = "PROFILE"`, or use an
existing authenticated CLI profile without additional model endpoints:

```toml
[retrieval]
extraction = "executor:existing"

[executors.existing]
kind = "codex"
binary = "/absolute/path/to/codex"
native_home = "/home/you/.codex"

[access]
read_roots = ["/home/you/.codex"]
```

Use the corresponding installed adapter for Pi or Hermes; see the host guides
and [executor configuration](configuration.md). Extraction pins the configured
executor and native home at admission. It uses that executor once and gives it
only the supplied source text; additional research and delegation tools are
denied. Text and native paths share the original workflow deadline. With no
selected extractor, submission returns `MODEL_NOT_CONFIGURED`.

Write a plan using exact `SourceSpan` objects from captured normalized evidence
or a recall receipt. Replace the example identity, revision, hash and offsets
with the real recorded values; offsets are UTF-8 byte boundaries.

```json
{
  "scope": {"user":"uid:1001","project":"alpha","branch_or_version":null,"session":null},
  "sources": [{"event_id":"recorded-source-id","revision":123,"content_sha256":"recorded-sha256","start":0,"end":36}]
}
```

```sh
akasha-otsuge extract run /absolute/evidence-unit.json --idempotency-key extraction-1
akasha-otsuge job show extraction-1
akasha-otsuge job cancel extraction-1
akasha-otsuge show SOURCE_ID --revision REVISION --range BYTE_START BYTE_END
```

Plans contain 1–8 source spans totaling at most 12 KiB, inside a 16 KiB plan file.
Overlarge units are rejected instead of truncating away their conditions. Scope
must fit every supplied source and current read permission. Internal sources
with `origin: extraction` or `auto_grill` cannot be extraction input.

The versioned `extraction-v1` prompt asks for atomic claims with conditions,
negation, quoted attribution, temporary scope, uncertainty and original citations.
Its strict schema allows at most 16 candidates and 64 KiB of output. Unknown
fields, authority claims, expanded scope, invalid source indices, hashes, ranges
or UTF-8 boundaries fail validation before publication. Model meaning still needs
review: schema validation does not prove that a paraphrase is faithful.

Publication follows the review policy pinned at admission: manual keeps candidates,
policy selectively activates tool-backed facts, and automatic permits delegated
activation subject to the [adoption rules](domain.md). None creates human approval.
New candidate and automated-conclusion revisions enter the [review inbox](review.md)
in the same transaction. Extraction jobs saved before this policy integration
retain their original manual contract. Generator metadata records the text
profile/model or executor, plus prompt version. Native profile defaults are
reported honestly as `native-profile-default`; the host's actual model identity
belongs in its native acceptance receipt.

Exact kind/scope/claim/conditions/assumptions/validity identities share one memory
ID. Repeated delivery reuses the durable job; repeated exact candidates do not
create another revision under the same policy mode. A new operation after a mode
change can re-evaluate a pending candidate; already adopted or reviewed records
are preserved. Distinct source occurrences merge without losing their
individual identities. Conflicting claims remain separate. Revised, reviewed or
non-extraction records are preserved. A union exceeding 32 sources fails with
`EXTRACTION_SOURCE_LIMIT`; it does not discard older evidence. This is exact
identity merging, not a semantic paraphrase clustering system.

After full validation, the daemon archives the model response before publishing
any candidate. Restart resumes a saved response, including partially published
batches, without repeating the model request or already published revisions.
An interrupted request without a saved response becomes a durable failed job;
use a new idempotency key for an explicit retry. Model failure never deletes raw
sources. Publication checks the original deadline before each write. A failure
after some candidates committed preserves those candidates and the checkpoint;
it is not an atomic all-or-nothing batch.

Approved records use the existing [index lifecycle](retrieval.md). Unapproved
candidates are excluded from active-memory retrieval. See the
[MVP-017 evidence](benchmarks/results/mvp017/README.md) for controlled crash and
boundary tests and the actual single-Pi extraction sample.
