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
