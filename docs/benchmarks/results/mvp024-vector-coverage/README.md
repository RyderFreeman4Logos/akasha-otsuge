# Explicit vector coverage

This functional leaf adds optional `retrieval.embedding_scopes`. Lexical indexing
remains unchanged. The default omits the new field from serialized configuration
and status and retains the previous vector generation identity.

The first daemon regression accepted the new configuration before filtering was
implemented. It indexed three vectors instead of the selected two. The same test
bytes then passed after production wiring; this is not evidence that the prior
release accepted the previously unsupported configuration key.

Related checks pass 67 Rust tests with one existing intentional ignore, plus
all-target Clippy, formatting and whitespace checks. The daemon tests exercise
the real index and recorded HTTP provider fixture: selected and excluded scopes,
unchanged lexical rows, old-generation query selection during reload, all-memory
identity restoration and restart, empty vector tables, and every excluded
completion in the contiguous prefix. The fixture returns synthetic embeddings;
these tests do not claim real inference or million-vector acceptance.

Two earlier expanded-fixture attempts failed before executing tests: the first
called a crate-private hash function; the second assumed the installed SHA-256
output implemented `LowerHex`. The final fixture reuses the existing byte-to-hex
pattern. Production visibility and dependencies were unchanged.

[evidence.zip](evidence.zip) preserves the failed and passing logs, command/source
receipts, initial regression bytes and pre-filter patch. [manifest.json](manifest.json)
binds every archive member. The canonical gate and independent review remain
pending at this checkpoint. Full acceptance stays open in
[#65](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65).

## Final gate and publication

PR #72 merged as `c51f97b1b0eb36d3e6d3b254cf1b8eebc04c57c2`, preserving
reviewed tree `2529fe1d86f3879b3ee8f96f2c2661647f873df5`. The exact canonical
retry and [CI 43978197739](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/43978197739)
each passed 650 Rust executions / 39 intentional ignores, Python 35, Node 7,
and installed lifecycle/UID/no-TCP checks. The first canonical gate failed
in an unchanged Codex caller with `HistoryGap`; its isolated selector passed,
but the original cause remains unclassified.

The [complete R1 report](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/72#issuecomment-6932339514)
has zero actionable product findings and 67 fresh Rust passes / one ignore.
Its overall **FAIL** remains: the reviewer overwrote the original parent scope
receipt and left a stale final report-hash attestation. Original scope bytes
are unavailable. The [R2 PASS](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/72#issuecomment-6932340485)
is a bounded independent recertification of the retained product/gate evidence
and exact-candidate promotion eligibility, with isolated outputs and verified
final custody. It does not reconstruct the lost receipt or replace R1's verdict.

[final-publication.zip](final-publication.zip) and
[its manifest](final-publication-manifest.json) retain both reports, incidents,
canonical outcomes, fresh checks, CI and publication receipts. Native child
transcripts are excluded. Full corpus import, load and recovery remain pending.
