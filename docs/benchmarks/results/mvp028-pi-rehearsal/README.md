# Single-host Linux package rehearsal

This rehearsal uses the downloaded x86_64 package from source `4e7b3a2799ebc2055750eb044b056cc1d887625a` and Pi 1.4.0. The archive SHA-256 is `4cc09f4c5d0cb51f7d778d181bdf74747bf4876402c2ee136e8b5652b07509a8`. The driver runs in new Linux mount, process and network namespaces with a private home. Only Pi is available as an agent host. A deterministic loopback peer drives native Pi and extraction requests. No external inference, paid subscription, human approval, or release tag is involved.


## Observed results

The [run receipt](pi-linux.json.gz) and [Git bundle](software.bundle) come from one 32.1-second invocation of the downloaded archive. The archive installation, native Caller, Runners, journal, compiler and notifier all ran in the isolated namespace.

| Step | Observed result |
| --- | --- |
| 1. Capture | Pi persisted the user's explicit offline lookup condition and the CLI read its exact 57 UTF-8 bytes through the scoped source span. |
| 2. Compact and consult | Native Pi persisted a compaction with a fixture-supplied summary. New turns with no hint and with `offline lookup` each returned `found` through distinct native jobs. |
| 3. Extraction, Grill and review | Manual extraction retained a candidate. After a policy change, automatic extraction adopted the same source-backed Memory as `delegated_approved`. No human approval was asserted. The native Pi interviewer, expert and reviewer completed one bounded Grill with a resolved report. |
| 4. Change and build | The caller used the keyword choice from the post-compaction consult answer. Its stored receipt selected revision 91. The matching invocation's outcomes link the receipt, git state, real GCC build, test output and artifact SHA-256. The offline test exited 1 before the change and 0 afterwards. |
| 5. Investigate | Two deliberate builds failed on an obsolete function name. The native Pi investigator read the immutable outcome and both compiler log handles. Its controlled answer reported insufficient causal evidence. The incident stayed open at high priority. |
| 6. Notify | The two reports shared an incident. The packaged stdio notifier persisted one transient failure and delivered the affected notification on claim 2. The incident's local and stdio deliveries succeeded and its generation acknowledgement replayed idempotently. |
| 7. History | The native state diff links candidate-era commit 65 to adopted revision 91. The archived original and both the initial and post-compaction recall payloads kept their hashes across the later work. |
| 8. Hot reload | The local model HTTP peer held an admitted extraction request open while the daemon applied its new settings. The old job retained manual review and the new job used automatic review. |
| 9. Restore and migrate | The daemon was stopped for a backup, restarted and killed with SIGKILL. On reopening, both recall payloads and the incident survived. The offline restore preserved 341 commits and 573 revisions. A separate schema-21/fork-28 Mempal fixture, including one explicit conversation row, converted and round-tripped all 33 native revisions. Both round trips matched the three neutral file hashes. |

## Reproduce

Use a Linux x86_64 machine with Bubblewrap, Python 3.11+, GNU tar, Git, GCC, the archive and its sibling checksum file. Provide an installed Pi 1.4.0 distribution containing `node_modules/.bin/pi` and its supported Node executable. The command requires a fresh output directory and retains the JSON receipt and small Git bundle there. It deletes the disposable home, extracted binary, transient stores and build outputs.

```sh
python3 -B scripts/check_linux_rehearsal.py \
  --archive "$archive" --commit 4e7b3a2799ebc2055750eb044b056cc1d887625a \
  --sha256 4cc09f4c5d0cb51f7d778d181bdf74747bf4876402c2ee136e8b5652b07509a8 \
  --pi-install "$pi_install" --node "$node" --output-directory "$fresh_output"
git clone "$fresh_output/result.bundle" "$fresh_worktree"
```

The two Git commits preserve the exact pre-change and post-change C program. The result also records the compiler, artifact SHA-256, execution plans, original source, selected Memory revision and immutable recall identity. The incident records a deliberately introduced build error. Its investigation reads actual CLI outcome and log handles. Its scripted classification leaves causal attribution open.

Each primary operation uses the same live application store. Restoring the backup and converting the schema-21/fork-28 Mempal SQLite input use fresh destinations as required by the archive interface. Archive comparisons check every logical commit, timestamp, revision and content byte. Mempal conversion also writes a provenance summary and a converter lock outside the three neutral files. The exercise measures package behavior under controlled replies. Model quality, the other native hosts, the scale gates and publication of a final release remain separate acceptance work.
