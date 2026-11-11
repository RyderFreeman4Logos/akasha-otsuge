# Production corpus pilot and bounded native mutations

This is a partial MVP-024 result. [#65](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65)
remains open for the unchanged full-scale acceptance.

[evidence.zip](evidence.zip) preserves 201 members in 997,080 compressed bytes.
[manifest.json](manifest.json) lists every member's original byte count and
SHA-256, plus the ZIP digest. All members were decompressed and hash-verified.
The archive contains all attempts and compile failures, complete commands and
terminal receipts, two pilots' acknowledgements/current-claim oracles/resource
samples/search receipts, retained-source hashes, executable identities, committed
source preimages and cleanup evidence. `scope-and-custody.json` distinguishes
dirty-attempt parent identities, unavailable intermediate trees and unproven
launcher-error causality. No native store, whole raw corpus copy or private
session/configuration state is included.

The 42 retained licensed source shards (43,986,181,099 bytes) were freshly
rehashed. Each pilot streamed the same 128 DBpedia and 896 unique FineWeb
records into the production Domain, then used the real daemon CLI. The input
manifest, policy TOML, observed-time binding, complete 1,042-claim oracle and
all acknowledgements are byte-identical across arms. Every current claim hash
and eight lexical partition witnesses passed. Both processes in each arm exited
normally before their disposable stores and socket directories were removed.

| Observed value | Before | Bounded native batches |
| --- | ---: | ---: |
| Daemon source | `8442871` | `b179076` |
| Seeder source (same executable) | `b191b01` | `b191b01` |
| Unique source UTF-8 bytes | 4,449,979 | 4,449,979 |
| Memory chunks / current-claim checks | 1,042 | 1,042 |
| Seed seconds | 3.798 | 4.017 |
| Index seconds | 32.330 | 10.559 |
| Search allocated bytes | 149,250,048 | 86,781,952 |
| Search files | 3,843 | 912 |
| Native versions, counted before cleanup | 1,109 | 132 |
| Authority allocated bytes | 12,750,848 | 12,722,176 |
| Daemon RSS sampled peak, bytes | 463,224,832 | 489,836,544 |
| Daemon cgroup charged peak, bytes | 521,965,568 | 473,677,824 |
| Resource samples | 32 | 12 |
| Observed OOM kills | 0 | 0 |

The original daemon SHA-256 is
`9b058b37553f9cf590d7f0062ad2baaec29e4a18b5f35e1a429b753c8df868f1`;
the successor is
`d8e23202ac1ae06fb04f321de3529fe86c09ee13539b39631a78469b7539a5ea`.
The unchanged seeder is
`3cd4f4f497af240805506bdcdd6ad3c8caf15e413e266d996ae31dc560e65146`.
The archived build and admission receipts distinguish each executable's source
from the surrounding documentation commit.

Two native regressions first failed because 16 rows produced 18 lexical and 19
vector versions, exceeding the predeclared eight-version budget including index
initialization. They pass unchanged after batching. The committed focused set
passes eight library retrieval tests (one existing child helper ignored), 23 real
daemon retrieval tests and three seed tests. The latter preserve UTF-8 evidence,
delegated authority, replay and bounded input. Buffer controls exercise both
delete/write orders, byte-triggered flush and the 256-key bound. Clippy passes;
the final canonical gate, independent review and publication are tracked separately.

These are single paired observations, with no statistical interval or file-cache
eviction. RSS increased; this is not a memory-improvement claim. Kernel charges
and RSS are distinct measures. Current-claim hashes and partition witnesses do
not prove every native index body byte, million-vector coverage, 100-client
latency or full recovery. Thirty-seven whole-document embeddings exceed the
current 4,096-byte input policy and still need a verified solution. Derived-index
disk growth/reclamation also needs further measurement before the full corpus.
No new Daybreak or adversarial campaign was run.

The final SSD `dua` audit reports 44,552,318,976 bytes for retained corpus/tooling,
18,639,011,840 bytes for the shared build target and 226,627,584 bytes for the
worktree before this archive. It follows the actual SSD roots. Recorded
filesystem free space did not immediately change after store deletion; no exact
physical-space gain is inferred from removed logical allocations.
