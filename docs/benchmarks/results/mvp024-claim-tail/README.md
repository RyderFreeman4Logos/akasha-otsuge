# Byte-preserving claim tails

The complete production-shape census rejected exactly 13 source records because
UTF-8-safe splitting left an all-whitespace claim. The exact-body diagnostic
found the same boundaries in all 13: `[0,32767)`, `[32767,65535)` and
`[65535,65536)`. Only the final byte was whitespace. No record was removed.

Commit `eb2157f9a3f9843bddd5af3170913ddb5ee1522b` moves the preceding
claim's last non-whitespace character into a short blank tail when both claims
remain nonempty and within 32,768 bytes. The repaired real-body boundaries are
`[0,32767)`, `[32767,65534)` and `[65534,65536)`. The Domain validator is unchanged.
The maintained regression covers an ASCII and a multibyte final character,
complete source payloads, exact spans, concatenated byte equality, stable replay
and an irreducible blank tail that still returns `INVALID_MEMORY`.

The identical single-test command first executed one failure on the old split,
then one pass after the repair. The test module SHA-256 was unchanged. All five
seeder tests passed, including a fresh run on the committed source.

The release seeder then admitted all 13 original bodies, totaling 851,968 UTF-8
bytes, and produced byte-identical acknowledgements on replay. The actual daemon
returned all 39 current claims. Their contents, revision identities, delegated
authority, scopes and source spans matched the original bytes. The parent
independently rechecked all 39 hashes against the retained bodies and fixed
expected boundaries; it did not reuse the production splitting function.

The original verification process exited **1** during final cleanup: its socket
directory allowlist omitted the daemon's `daemon.lock`. All functional checks
had passed, all three children had exited and the data directory was already
removed. A subsequent read-only reconciliation initially lacked `TMPDIR`; that
failure is also retained. Running the unchanged checker with the original SSD
temporary directory verified absent consumers, acquired the remaining lock and
removed the socket directory. No import or daemon read was repeated. The final
qualification is `FUNCTIONAL_PASS_CLEANUP_RECONCILED`, not a rewritten zero-exit
result for the original driver.

The release daemon SHA-256 for this bounded verification is
`c8caa9b73e1febe55e0ee076af8a5a80e2abdbbd812f12e847b96982ee901c9e`;
the repaired seeder is
`78a4dc62e9b78c958fbee0f7e1d958bfee0d373d5a5127ea42aad63b87282284`.
Canonical-gate artifacts have separate provenance. This verification does not
establish full corpus import, indexing, load or crash/recovery acceptance. The
original full preparation admission failure remains unchanged.

`diagnostic.zip` contains 27 members and 340,167 bytes, SHA-256
`24fa64d83cc22689c63f5546d8eb90ff769e5c49bd450b72c793f5033cd2b460`.
It retains all 13 public source bodies compressed to 315,526 bytes, their pinned
FineWeb source attribution/license metadata and original rejection identities.
`diagnostic-manifest.json` records every member hash.

`evidence.zip` contains 75 members and 79,209 bytes, SHA-256
`43f04a35f1b3a14f20280f6d75d059fdc7ccb96ad2b4d009dcce3fc183a73470`.
`manifest.json` records all source snapshots, tests, build, acknowledgements,
readback/oracle hashes, original failures and cleanup receipts. Neither archive
contains a native database or an additional full corpus copy.

## Final publication

[PR #75](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/75) merged as
`a177d6e23568b627ec9eef593d88289bc8634041`, with the reviewed `73a0911` tree.
The exact canonical gate passed 652 Rust executions, 35 Python tests and seven
Node tests. Independent review reported zero findings. Hosted CI44016475601 passed.

`final-publication.zip` contains 235 members and 839,936 bytes, SHA-256
`cc5d898942aaf3541c05cd871f61830d3ba5f30971a74b57d7574c7f259c55de`.
Its manifest records each member hash. The archive retains full gate, review,
CI and publication receipts. Full production import and indexing remain separate.
