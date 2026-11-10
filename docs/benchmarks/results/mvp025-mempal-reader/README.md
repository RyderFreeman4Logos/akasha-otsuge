# Read-only mempal snapshot conversion evidence

The production conversion source is `41933018a3835538b3fbba33adc40a60b364c2e4`
(tree `8a7c5efc799a7251cca42422ae358c9d1c62006e`). The source, candidate projection
and output reconciliation commits are recorded separately. This archive keeps
controlled fixtures, real private-data acceptance and historical PR #59 CI distinct.

`runs.json` retains every focused attempt, including the missing-command and
projection REDs, the SQLite-limit compile failure, initial repeat refusal and the
dangling-manifest RED. Complete original logs are gzip-compressed; their terminal
receipts hash the decompressed bytes. Admissions retain each attempt's actual
parent HEAD, dirty patch digest, command and environment. They are not relabelled
as clean final-HEAD gates. Final canonical/review/CI results will be separate.

The projection phase passed 60 related tests with one existing opt-in ignore.
The recovery phase passed 20 archive tests and a supervisor that actually killed
five children with SIGKILL, then recovered and imported each archive. Its one
ignored child is run by that supervisor. Formatting and all-target Clippy passed.
These are controlled fixtures, not machine power-loss or ENOSPC claims.

`real-conversion-summary.json` records the actual schema21/fork28 immutable
snapshot conversion: all 3,525,645 retained rows in 29 tables matched independently
computed typed SQLite row digests, including IDs, original fields and relation
rows. Every native candidate's source span and pending review was checked;
12 sampled original fields matched exact reassembled chunks. The original
11,741,294,592-byte input retained its full hash and file identity under a kernel
read-only mount. The resulting neutral archive is 5,984,986,114 bytes.

The unchanged independent validators are included as
`mvp025-mempal-real-audit-r1.py` and `mvp025-mempal-archive-oracle-r1.py`.
Their typed-cell/row self-checks can run without private input. Full private runs
require the separately retained acquisition/config/schema/row-digest inputs.
Controlled fixture receipts include a corrupt-blob negative control.
Private contents, source paths, original IDs and source/archive digests are
excluded from this public bundle. Counts are not a public copy of the corpus.

The source layout was mapped against mempal commit
`4593a5c5e6a9ea1140256cd42fa5838511a24e14`; the installed producer's exact commit is
unknown. No absent documents, approval or historical versions were reconstructed.
Legacy claims remain unverified, candidates inactive, and queued payloads inert.
Unknown event dates and unmapped constraints are counted rather than invented.

Different-source incremental application remains an explicit issue #25 follow-up.
This evidence does not establish the #24 resource/load envelope, authenticated
Claude execution, provider/model quality, or final release acceptance.

`real-roundtrip-summary.json` records two empty native imports, one duplicate
import and two exports whose entire files match the input archive, preserving
27,511 original commits and 1,687,022 revisions. `real-runtime-summary.json`
records 35 actual CLI commands over two isolated daemon starts: three distinct
candidates retain identical historical records, clocks, original chunks and
open reviews; they remain inactive and no legacy jobs appear. The first runtime
probe failed because the harness expected the wrong enum casing. Its corrected
run passed without a product edit, and both attempts stopped their daemons.
