# MVP-024 resource observability leaf

This is partial production work for issue #24, which remains open. It reports
requested budgets, actual engine-open budgets, and on-demand process-tree memory.
Memory admission/draining, managed cgroups, continuous peaks and full-data stress
are not implemented or accepted by these receipts.

Two baseline runs at a036033 reproduce three missing public behaviors: effective
limits, actual daemon status/reopen identity, and accounting for a guarded notifier
and detached grandchild. The first implementation attempt stopped with E0063 in
two backup restore constructors; it is a compile failure, not behavioral GREEN.
After supplying the same opening-budget field in all constructors, the focused
run passes 24 config, 1 measurement unit, 2 configuration checkpoint units, 2 real
daemon tests and all-target Clippy in 40.735 seconds.

Each run retains its admission, terminal, collected receipt and compressed log.
The final source byte hashes below bind the focused candidate; this archive does
not claim an exact committed-head canonical gate or independent review. Formatting
ran before the focused tests and is recorded in the command; pre-format admission
diff identity is not mislabeled as the final source identity.

The prior issue #23 post-merge CI42750140146 is also retained: SUCCESS on
37d4d77ec4517d9a35eff7eb85704de6a656d739, cache restored/hit. This does not validate
the new #24 code.
