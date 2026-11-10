# MVP-025 neutral export focused evidence

This leaf adds offline production `archive export`, a versioned JSONL/blob
format, exact receipt/clock/revision/original-byte preservation, private output
and final manifest publication. It does not implement import or complete #25.

The initial public CLI tests compiled and failed twice with INVALID_ARGUMENT
because the command did not exist (17.058s). First GREEN passed two archive,
five CLI and 17 storage cases plus all-target Clippy (27.133s). Two additional
boundary controls then produced one causal relative-path RED and three passes
(1.081s). The real EFBIG blob-write case passed: no manifest, prior source intact.
It is not evidence of ENOSPC or power-loss acceptance. Final GREEN after the
relative-parent repair passed four archive, five CLI and 17 storage cases plus
all-target Clippy (23.296s). Test counts, failures and original logs remain distinct.

Source hashes below bind final bytes after formatting. Admission receipts bind
the earlier head plus each working candidate; they are not exact committed-head
canonical receipts. Full canonical, independent review and hosted CI are still
required before publication.

The mempal report is retained byte-for-byte in gzip (including original Markdown
whitespace). It is read-only source discovery at frozen
4593a5c5e6a9ea1140256cd42fa5838511a24e14. Its fourteen complete source-blob hashes
and decision-driving version/open/admission excerpts were independently checked.
It did not read live data, generate a real sample or establish migration success.
The source format has schema21/fork28 and does not retain every old full text.

The main CI42755182650 receipt/log is separate historical post-merge evidence
for c059fda, not a gate for this candidate. It passed with Cargo cache restored/hit.
All disposable test fixtures were removed by their owners; shared target/corpus
and compact audit receipts remain.
