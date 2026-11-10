# MVP-025 imported runtime final acceptance

PR [#59](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/59) merged as
`31d4c61ecf6672ae52e3b84bbd00b630bceed3f8`. Its tree exactly matches reviewed
`fa801c91d241582ee8e8d93dddf10f12397076ca` (`8ebd432fa7e6719556adccab59c69bbb354dac4e`).

- Unchanged canonical `just check` passed in 446.892s: 561 Rust passes and 33
  ignored including installed repeat, 159 daemon cases per run, 34 Python
  tests, Node checks, formatting, Clippy, release and installed lifecycle/UID
  isolation with zero TCP listeners.
- Independent whole-range R1 found zero actionable introduced defects and
  freshly passed all 60 selected tests. The complete unchanged report was
  [published and read back](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/59#issuecomment-6762120374).
- Exact-head [CI 42777693037](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/42777693037)
  passed with the Cargo cache restored/hit. Post-merge main CI is a separate run.
- The merge receipt and [issue progress comment](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/25#issuecomment-6762258106)
  preserve issue #25 as OPEN.

The code establishes fresh destination execution authority while retaining the
original memory, scope, revision history and recall outputs. Four import and
four initialization SIGKILL phases are covered, plus real CLI/daemon history,
inert old notification delivery and controlled Pi re-registration after import.
SIGKILL is not power loss or real ENOSPC; controlled host protocol is not actual
provider authentication. Cross-UID remapping is not automatic.

A separate 367.076s read-only audit of the one real private mempal snapshot passed
32 units: SQLite quick_check, typed row/table digests and counts for 29 retained
tables, seven selected relationship checks and final file-hash agreement with
acquisition. It found 3,525,645 rows, 1,358,141,546 UTF-8 bytes and 3,031,040 blob
bytes. The public summary contains counts and sizes only; source content, paths,
IDs and private digests remain local. This audit is preparation, not conversion
or target equivalence. Unknown installed-binary provenance and untested logical
relationships remain explicit.

Actual mempal conversion, incremental application of different archives and full
real-sample migration/round-trip acceptance remain required before closing #25.
Source hashes below bind the frozen reviewed source, independently of this later
documentation archive. Initial focused failures and fixes remain in
`../mvp025-import-runtime/`; their history is not rewritten here.
