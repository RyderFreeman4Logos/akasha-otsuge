# MVP-010 development receipts

These are byte-identical originals from focused development, including failed
compilations, harness mistakes and behavioral regressions. They ran in the dirty
feature worktree; their admission HEAD is the base, not an exact final candidate.
The final committed-HEAD gate and independent review are reported separately.
See ../../../retrieval.md for the behavioral baseline and limitations.

The source-union/output, scoped-metadata, progress, generation identity, deadline
and failure-trace RED/GREEN runs are retained without relabeling compile errors
as behavioral RED. The concurrency-validity run used one scheduler slot; only
concurrency-red2 is the two-slot lock regression. The two live-coverage initial
compile errors are harness setup failures. The final-focused collector overstated
the daemon count by one; its separate correction preserves the actual 27 passed
and 2 ignored result. The final empty-library control adds one public test.

All model inputs and responses are synthetic protocol fixtures. No real model
quality or full-scale production resource result is inferred from these logs.
