# MVP-004 native storage verification

Verification date: 2026-10-27 UTC. Linux x86_64, Rust 1.99.1,
SurrealKV 1.0.0 commit `4338a790c8edc56ef5ea1e46157427af4815be3c`,
Zstd crate 0.13.3. No model, external host or provider is involved.

`cargo test --workspace --locked` passed: 14 storage integration tests, one
crash-supervisor test, 17 configuration tests and four CLI tests. The only
ignored unit is the supervisor-owned crash child, invoked six times explicitly.

| Contract | Observed result |
| --- | --- |
| Atomic native persistence | Evidence, memory, state and job revisions, heads, receipt, outbox and sequence survive native close/reopen together |
| Idempotency | Simultaneous retries and retries after restart return the original receipt; reordered changes normalize identically; changed content is rejected without a second commit |
| Optimistic revisions | A stale batch leaves no staged evidence, revision, head, receipt or outbox; exactly one competing expected-head update succeeds |
| Immutable history | Corrections append revisions and preserve previous content/hashes and the original successful receipt; original evidence cannot be overwritten |
| Bounds and corruption | Oversized/duplicate/invalid requests do not advance sequence; tampered content/key/sequence fails read validation; unknown/missing format and missing data/metadata fail admission |
| Checkpoint round trip | Backup at sequence 2 remains at 2 after source advances to 3; restored original/history content hashes, revision identities, receipts and outbox match, including after reopening |
| Negative backup cases | Corrupt/missing manifest, changed bytes, unlisted files, forged relative paths and symlinks are rejected; no destination authority is created for these invalid inputs |
| Existing destinations | Creating another backup at the same path, backing up inside the authority, or restoring over a live authority is rejected without overwriting it |
| File independence | Published backup files have one link; restore adds no backup links; overwriting a backup SST leaves the reopened source and restored stores readable and unchanged |
| Cancellation | Eight submissions dropped after the first future poll produce checkpoints that restore a complete, internally consistent committed prefix |
| Ownership and inventory | A second native owner is refused; fixture files contain neither SQLite names nor SQLite database magic; the locked dependency graph has no SQLite package |

The isolated real SIGKILL run reported:

| Checkpoint | Recovered sequence | Retry sequence |
| --- | --- | --- |
| Before staging | 1 | 2 |
| After first revision staged, before its head | 1 | 2 |
| Before native commit | 1 | 2 |
| Native commit future polled to Pending | 1 | 2 |
| After durable commit, before return | 2 | 2 |
| After returned receipt | 2 | 2 |

Every case checks all four namespaces plus receipt/outbox/sequence, the unchanged
baseline and no third commit on retry. The Pending case accepts either complete
outcome; the observed 1 is not a guarantee about every interrupted native commit.
Every child is killed and reaped, including assertion-failure cleanup paths.

The tests first exposed missing implementation, duplicate effects, stale writes,
missing admission/checksum checks, silent empty-store recreation, native Tree
clone/drop closure and checkpoint hard-link aliasing. They passed after the
corresponding bounded fixes. The two native API pitfalls are handled in the
adapter without changing upstream SurrealKV.

The final exact-HEAD canonical gate, independent review, CI, merge and completion
receipt are recorded on [Issue #4](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/4).
At this development checkpoint that publication tail is still pending. These
small native fixtures do not replace the product 50 GB/100-client acceptance in
MVP-024. SIGKILL is not a hardware power-cut test. Migration, backup space needs
and the immutable-source assumption are specified in [storage.md](storage.md).
