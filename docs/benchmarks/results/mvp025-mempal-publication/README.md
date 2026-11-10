# Descriptor-owned metadata publication

Whole-range R2 at `7f8902487a0a745cbd63765a9d85ad1adcbbf189` failed with
one HIGH sibling of the original source-loss defect: the final replacing rename
could overwrite a source moved into conversion.json or manifest.json after
admission. The complete [R2 FAIL](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/60#issuecomment-6765654883)
and fresh focused log remain unchanged here. The prior exact canonical PASS and
historical CI42798698625 PASS are separate evidence, not a waiver.

After two HIGHs in the same mutation path, the repair inventories all writes and
replaces the metadata publication protocol. Identity and manifest use a new
anonymous O_TMPFILE descriptor, file sync and a non-replacing link into a pinned
directory. No named temporary is truncated, renamed or unlinked. All recognized
pre-existing output files use the same private-file/source-inode check; data
append remains on validated descriptors. Legacy temporary members are preserved.
Unsupported anonymous-file/link operations return an I/O error without a
replacing fallback. The local filesystem capability probe passed; see the
[Linux open](https://man7.org/linux/man-pages/man2/open.2.html) and
[link](https://man7.org/linux/man-pages/man2/link.2.html) contracts.

The new shared-publisher regression failed on both final-name occupants before
the repair. Afterward, 21 archive tests and three mempal unit tests passed, with
one intentionally ignored supervisor child, all five actual SIGKILL phases,
formatting and all-target Clippy. The original path and moved-inode assertions
remain intact.

A tiny fsync interposer then exercised both actual CLI binaries with the same
four controlled source moves: identity/manifest final names and their old
named-temporary counterparts. It moves the fixture source at the real metadata
sync boundary and leaves the product call/result unchanged. Each marker proves
the move happened. Both normal controls passed. All four frozen baseline cases
lost or relocated the moved source; all four repaired cases preserved its exact
moved pathname, bytes and inode. Both occupied final names caused a nonzero
candidate exit. A manifest-temporary source move occurs after the source check;
its candidate may succeed while preserving that externally moved source. This
is not a claim that external actors cannot rename an input themselves.

The baseline release hash and candidate debug hash are pinned in the receipt.
The probe ran before the final release rebuild, while that baseline still occupied
the shared release path. Its script intentionally checks that hash; it must not
be relabelled as a baseline replay after the shared binary is replaced. The
controlled SQLite fixtures, interposer library and all subprocesses were cleaned.
No private/live database or full corpus was used.

`manifest.json` pins the repaired writer bytes. Final exact canonical R3,
whole-range R3, current-head CI and merge are subsequent stages. The original
full private-data acceptance remains bound to source `4193301`; no new full
private-data execution is claimed. Different-source incremental migration stays
unfinished in issue #25.
