# Native startup memory controls

This is partial work for [#65](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65).
Full corpus, vector coverage, concurrent load and recovery acceptance remain open.

Three controls use source `2987eb1e744d2a2c55e642df3457b6156c8e2a88`, tree
`f8396cd1e5db8acbaf0d223a89eb735828c67bed`, and the same daemon and seeder:

- daemon SHA-256: `3ca6fe5960d421d2a3dae1010012f8ea5d1ed1b25be64724932449b30f1e3702`;
- seeder SHA-256: `f009f18a372add120f4c8994bb80844d6301f9eba8b2f3af8afca747c365ff3c`.

All use identical 8,192 real source records, 35,875,218 unique UTF-8 bytes,
8,322 expected claims and eight partition witnesses. Policy, input manifest,
binding, seed acknowledgements and expected claim oracle match the earlier
scale baseline byte for byte. Original 512/640/2048 MiB limits and deadlines
remain unchanged. Every resource observation confirms effective owned cgroups.

| Observation | FTS1/32 only | FTS1/32, arena2 | FTS1/1, arena2 |
| --- | ---: | ---: | ---: |
| Completed indexed claims | 3,622 / 8,322 | 8,322 / 8,322 | 8,322 / 8,322 |
| Complete claim / partition checks | unavailable | 8,322 / 8 | 8,322 / 8 |
| Complete indexing seconds | unavailable | 203.635 | 795.438 |
| Resource observations | 94 | 197 | 753 |
| Valid procfs RSS observations | 93 | 197 | 753 |
| Sampled RSS maximum, bytes | 692,056,064 | 406,609,920 | 282,107,904 |
| Nearest-rank sampled RSS P99, bytes | not an acceptance value | 405,094,400 | 277,876,736 |
| Samples above 640 MiB | 46 | 0 | 0 |
| Sampled kernel charged peak, bytes | 565,559,296 | 537,133,056 | 537,833,472 |
| Sampled OOM kills | 0 | 0 | 0 |

The first control failed the unchanged status IPC deadline. One raw observation
reports unavailable procfs measurement and remains unknown. Two initial read-only
summary attempts incorrectly assumed that group was non-null; the corrected
aggregation retains the unknown row. FTS tuning alone was insufficient.

The arena controls verify every current claim's hash, revision and authority,
then verify an expected lexical hit and scope on each partition. Their explicit
daemon environment was read back from procfs. The first control did not capture
inherited `MALLOC_ARENA_MAX`, so its historical value is unknown. No strict causal
allocator comparison is claimed. The two arena controls differ only in the
declared FTS partition setting, with complete configuration identities retained;
single sequential observations without cache eviction or independent repetition
do not establish a causal performance interval. Kernel charge and RSS differ.

The 32 MiB partition is selected for the production startup leaf: this measured
configuration stayed below the required 512 MiB target and completed sooner.
The 1 MiB setting is a retained tuning alternative. Compatible targets below
512 MiB do not gain a tested operating-envelope claim. The implementation uses
safe same-PID execution at the existing daemon entrypoint; it adds no dependency,
configuration option or alternate parser. `GLIBC_TUNABLES` and unrelated values
remain inherited, so explicit allocator overrides can supersede the arena hint.
Source-level settings are not a substitute for measured resource acceptance.

The second arena control's launch wrapper returned before its control-plane
receipt was consumed. Its durable primary identity was adopted once, the WD
relay/completion listener was attached, and no duplicate benchmark was launched.
The wrapper now waits for command completion before interpreting its exit code.

All three owned daemons, seeders, controllers and cgroups terminated. Complete
file hashes and scoped consumer checks preceded removal of exactly their data
and socket directories: 110,526,464, 127,340,544 and 140,582,912 allocated data
bytes respectively. No immediate physical-space gain is inferred on Btrfs.

[controls.zip](controls.zip) retains 126 members in 2,582,247 bytes, SHA-256
`1b6520290a3f566345e2544dae3429bf8a4cefbfa267a7e9e53f9e461e3c45b0`.
[The manifest](controls-manifest.json) hashes every member. Fifteen shared input
references point to the existing scale-comparison archive instead of duplicating
the corpus and oracles. All members and referenced bytes were verified.
No full-scale or new Daybreak result is claimed.

## Production entrypoint regression

The new actual-daemon regression first failed on the merged base because
`MALLOC_ARENA_MAX=2` was absent. With the 16-line startup change, the same
assertions pass for absent, larger inherited and already-bounded settings. They
also verify the spawned PID and an unrelated glibc tunable remain unchanged.
The test received formatting-only layout and a trailing comma after the RED;
the exact before/after source is retained. The first focused wrapper refused
that comma under an overly strict whitespace comparator before running tests.
That setup failure is preserved separately from the product RED.

The subsequent focused run passed 32 Rust tests: the new regression, all 26
daemon retrieval tests and five CLI tests. The actual detached entrypoint also
passed startup-setting readback, exact process identity/argv, private-path and
peer-UID controls, zero TCP sockets and normal stop. Clippy and formatting pass.
This smoke used the debug artifact, not an installed-release acceptance claim.

[regression.zip](regression.zip) and [its manifest](regression-manifest.json)
retain all source preimages, failure logs, per-command results and hashes.
The committed release's real-corpus pilot follows below. Exact canonical gate,
independent review and publication remain separate steps.

## Production defaults on the real corpus

Normal commit `92031ea46c6dcb33e4810954aba00b91c5d3357d` was built in release
mode. Its daemon SHA-256 is
`93c7abb6f2e1fbf2d90d631e66768e65c56bf4e82b8c1ba3111108d481d41082`;
the seeder retains `f009f18a372add120f4c8994bb80844d6301f9eba8b2f3af8afca747c365ff3c`.
The controller supplied no FTS or allocator overrides. After readiness, procfs
confirmed the product's arena2/FTS1/32 environment and unchanged PID/start identity.

All 8,192 sources, 8,322 indexed/current claims and eight scoped lexical witnesses
passed again with the same complete input/oracle identities. Seeding took 40.388
seconds and indexing 218.891. All 197 resource observations were complete and
confirmed original 512/640/2048 MiB enforcement. Sampled RSS maximum was
414,588,928 bytes; nearest-rank sampled P99 was 411,643,904. No sample exceeded
512 MiB. Observed kernel charged peak was 537,186,304 bytes, with zero sampled
OOM kills. This one bounded run does not establish continuous peaks or full-scale
performance. No ANN, 100-client load or complete native-index-body oracle is claimed.

The stopped store contained 543 files and 127,356,928 allocated bytes. Complete
file hashes, exact terminal identities, absent owned cgroup and consumer checks
preceded removal of only that data directory and socket root. Both paths are
absent; shared corpus and build cache remain.

[production-default.zip](production-default.zip) and
[its manifest](production-default-manifest.json) retain the complete build,
commands, output assertions, resource observations and cleanup receipts. Five
shared inputs reference the existing scale-comparison archive with verified hashes.
The following evidence-only commit preserves the exact tested implementation.
