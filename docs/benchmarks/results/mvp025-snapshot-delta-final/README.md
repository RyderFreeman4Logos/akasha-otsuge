# Completed mempal migration and snapshot-delta acceptance

Issue [#25](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/25#issuecomment-6776919316)
closed as completed at 2026-11-10T19:19:33Z. PR #62 merged as
a500d531b7a02c575a593a1cbad37b93dcf8cb0a, with the same tree as reviewed
014e15d6cc1c3119d6b16ab08d79a4b028e41249:
46ac6632cef510642c4653055aa2da6cf2446802.

- Exact canonical `just check`: exit 0 in 541.642 seconds; 599 Rust test
  executions passed, 35 intentional ignores, including installed repetition.
  Python/Node, format, Clippy, release, lifecycle and UID-isolation checks passed.
- Independent conventional whole-range review: PASS, zero actionable findings;
  90 changed paths, 66 archived members and 28 historical/current source pins
  verified. Four maintained delta tests passed freshly. Full report and
  publication readback are retained; no Daybreak or new red-team campaign.
- CI42878422485 passed at the reviewed head. Complete logs prove Cargo cache
  restoration and primary-key hit. The remote Linux job took 11m41s.
- The same release binary passed all six real unchanged-snapshot acceptance
  units in 1635.302 seconds. The input is 11,741,294,592 bytes and represents
  3,525,645 rows. Both invocations retained 27,511 commits and 1,687,022 revisions,
  with identical archive payloads and responses, unchanged input hashes and
  no new history. No SQLite source copy or provider call was made.
- Archive size here includes its manifest: 5,984,986,447 bytes. The earlier
  5,984,986,114-byte value counted the two payload files alone. Cleanup removed
  5,984,986,686 logical bytes including conversion metadata, plus the already
  closed disposable native store. This is not a filesystem free-space claim.

Changed-source lifecycle is covered by the independent structural fixture,
including three generations, key identity, update/deletion/reappearance,
withdrawal, history/review eligibility, prefix recovery and public-command
roundtrip. The real source in this final run was unchanged. Earlier full-row
conversion, two fresh imports, repeated import, two exports and 35 isolated
runtime CLI checks retain their distinct source identities in the
[reader archive](../mvp025-mempal-reader/README.md) and
[reader final archive](../mvp025-mempal-final/README.md).
Neutral export/import/runtime/prefix work is recorded in PRs #57 through #61.

The installed release binary SHA-256 was
4fe07ef3452254da7895d14b84c9d5a4b5ce810ebc9c75b983328f12667b0b19.
Actual acceptance ran on Linux 6.1.0-52-amd64 x86_64 with Rust/Cargo 1.99.1.
The original supported mempal source is 4593a5c5e6a9ea1140256cd42fa5838511a24e14,
schema21/fork28; the installed legacy binary's exact build source remains unknown.

Private source paths/content/fingerprints and reviewer transport are excluded.
Full reports, measured logs, sanitized unit receipts, and publication readbacks
are retained. Cleanup checks were scoped to readable same-UID processes:
384 were inaccessible inside the acceptance namespace, 94 during reviewer
transport cleanup. No global census is claimed. Historical failures and unknown
causes remain in their original archives.

The manifest pins production/test files to the historical reviewed commit,
not the later documentation commit. This closes migration/roundtrip acceptance,
not production resource stress (#24), packaging (#26), fixed-oracle evaluation
(#27), final release (#28), or native Claude authentication (#12).
