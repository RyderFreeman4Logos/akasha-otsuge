# Auto-Grill

Auto-Grill explores a question against an explicit, bounded set of original
source spans. Configure three roles using one installed CLI or a mixture of
supported executors. Each call gets a fresh context and the existing native
authentication; no additional text/embedding/reranker endpoint is required.

```toml
[executors.existing]
kind = "codex"
binary = "/path/to/supported/codex"
native_home = "/path/to/existing/profile"

[access]
read_roots = ["/path/to/existing/profile"]

[grill]
interviewer = "existing"
expert = "existing"
reviewer = "existing"
max_questions = 4
max_rounds = 2
max_runner_starts = 8
max_tokens = 128000
```

For mixed execution, define another enabled executor and select it for a role.
Each role uses that executor's model, thinking arguments and timeout. See the
[configuration contract](configuration.md) for native profile and version rules.
Pi and Hermes require their installed Akasha adapter; installation in a disposable
profile is sufficient for isolated testing. No fallback is implicit.

Create `grill.json` with `question`, `scope` and `sources`. Scope and exact source
span objects have the same format as [extraction](extraction.md). The question is
limited to 4096 UTF-8 bytes. Supply 1–8 original spans totaling at most 12 KiB;
source revisions/hashes, UTF-8 boundaries, scope and current permissions are
validated. Internal Extraction/Auto-Grill sources cannot seed a new discussion.

```sh
akasha-otsuge grill run grill.json --idempotency-key discussion-1
akasha-otsuge job show discussion-1
akasha-otsuge job cancel discussion-1
akasha-otsuge grill resume discussion-1 --idempotency-key discussion-2
```

The interviewer selects a finite dependency graph in prerequisite order. Expert
and reviewer answer independently in round zero; neither sees the other's answer.
Later rounds exchange the latest answers for cross-questioning, alternatives and
checks against the supplied originals. One round means one expert/reviewer pair;
the interview consumes a separate start. Reusing one model in separate roles
does not imply statistically independent judgments.

The service checks citation identity and byte ranges, question dependencies and
output schema. These checks do not prove that a model's interpretation is true.
Unsupported statements, unresolved dependencies and disagreements remain explicit
assumptions in hypothesis candidates. A matching claim with different conditions
or kinds is still disagreement. Each candidate retains its strongest opposition
and exact original/discussion sources. Every discussion source is `auto_grill`,
and no result becomes human-confirmed from model agreement. Eligible resolved
conclusions use the configured review policy; manual review is not hardcoded.

Jobs pin configuration at admission and share a finite start/token/output budget
and the original workflow deadline across all roles. Stop reasons distinguish
resolved questions, unchanged reports (`no_new_evidence`), round/start/token or
other resource limits, cancellation and native errors. Tokens include cached input.
The shared native transport-output cap is 1 MiB, including streamed events, so
verbose hosts can reach it before the configured round limit. Native usage can
arrive after consumption, so the observed overshoot is retained
and further calls stop. This is not a provider-specific currency estimate or a
hard pre-billing quota. Runtime pause blocks new admission/claims; use job cancel
to stop an active discussion.

Prompts and completed responses are immutable source events. `job show` returns
their spans in `result.discussion` and the latest attempted prompt in
`result.attempt_prompt`; read them with `show EVENT --revision REV --range START END`.
The private checkpoint also retains exact reports and usage. Failed native calls
leave completed phases and originals available. A committed response or partial
candidate publication resumes after daemon restart without another model call or
candidate revision. A running call without a saved response fails as
`RUNNER_INTERRUPTED`; it is not silently replayed.

Explicit `grill resume` creates a new job from a terminal discussion, retains its
question graph and latest role answers, and links the parent. It uses a newly
authorized budget/configuration and a new job clock. The parent's clock, usage,
errors and original sources remain unchanged. Retrying an idempotency key returns
the same job; changing its plan is a conflict. Resuming an unfinished job or
changing the inherited question/evidence is rejected. New candidates are separate
proposals; they never overwrite a previously reviewed or edited record.

[Acceptance evidence](benchmarks/results/mvp018/README.md) includes real Pi and
Pi/Codex samples, precise source identities and the retained failures.
