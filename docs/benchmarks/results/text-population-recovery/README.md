# Full-source query preparation recovery

`spikes/data/check_text_population_recovery.py` interrupts the real source replay
after at least 16 durable selections, confirms the exact native process exited,
then explicitly resumes. Both runs stopped after 17 selections and reproduced
all 512 canonical queries, SHA-256
`87a07d022c43c378365422d1494c011802fe225e4176975037476ef6b9443546`.
Completed reuse changes no preparation artifacts. The original 42 corpus shards
are read in place; no expanded stream or additional corpus copy is retained.

The first run passed the source assertions but sealed its result before updating
terminal progress, leaving one stale artifact hash. Its original result, executed
harness and separate terminal audit are retained. After fixing publication order,
the second full-source run completed in 558.077 seconds and every recorded
artifact hash verified. `archive.json` independently hashes all 40 retained files
and the compressed archive (about 223 kB).

This accepts preparation recovery only. The preparer used a separate 8 GiB
resource group; its RSS is not a storage-engine acceptance result. Full database
search, file-cache-cold behavior and physical power loss remain untested here.
