# MVP-023 R3 reference-plugin validation repair

Candidate `24ca2818d4ada599f6d91409cf38d85a84acd47d` passed its complete
canonical gate: 535 Rust passes including installed repeat, 155 daemon passes
per run, 33 Python cases, lifecycle/UID checks and zero TCP sockets. Independent
R3 confirmed the preceding recovery repairs and all requested focused checks,
but found one LOW invalid-input preservation defect. The unchanged full FAIL
report was published and read back on PR #55.

The causal RED invokes the real file plugin with a valid-shaped update to a
missing message and `--fail-first`. It proves the marker changed durable bytes
before the update target was validated. The repair moves the target check ahead
of demonstration-marker persistence, under the existing lock and after the
receipt-replay check. The regression also checks a mismatched target and valid
existing-receipt replay while the demonstration flag is enabled.

All five standalone plugin cases and the actual daemon's 100-event grouped
reference-plugin integration pass in 10.769 seconds. The first test is an
executed behavioral failure, not a compile/setup failure. The retained prior
canonical and R3 FAIL remain bound to their original candidate. Current source
hashes bind this small repair; fresh exact canonical, whole-range R4 review and
new-head hosted CI are separate requirements. Prior-head CI was still running
when this archive was made and is not represented here as passed.
